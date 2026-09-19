// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads one GitHub issue for the to-do import (admin studio spec section 10): a one-time copy of
//! its title and text through the public API, with an optional token for a private repository.

use std::time::Duration;

use nw_login::install_crypto_provider;
use reqwest::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::Deserialize;

/// The web address of an issue: `https://github.com/<owner>/<repo>/issues/<number>`.
const ISSUE_URL_PREFIX: &str = "https://github.com/";
const ISSUES_SEGMENT: &str = "issues";
/// GitHub refuses a request without a user agent.
const AGENT: &str = "blighted-admin";
const GITHUB_JSON: &str = "application/vnd.github+json";
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// One issue of one repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRef {
    /// The owner (user or organisation).
    pub owner: String,
    /// The repository.
    pub repo: String,
    /// The issue number.
    pub number: u64,
}

/// What the import copies.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Issue {
    /// The title.
    pub title: String,
    /// The text; GitHub sends `null` for an empty one.
    pub body: Option<String>,
    /// The web address.
    pub html_url: String,
}

/// True for a GitHub owner or repository name: letters, digits, `-`, `_` and `.`.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// The issue of a web address; `None` for anything else. A `#` or `?` part is ignored.
pub fn parse_issue_url(url: &str) -> Option<IssueRef> {
    let rest = url.trim().strip_prefix(ISSUE_URL_PREFIX)?;
    let rest = rest.split(['#', '?']).next()?;
    let mut parts = rest.trim_end_matches('/').split('/');
    let (owner, repo, segment, number) =
        (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some()
        || segment != ISSUES_SEGMENT
        || !plain_name(owner)
        || !plain_name(repo)
    {
        return None;
    }
    Some(IssueRef {
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        number: number.parse().ok()?,
    })
}

/// The GitHub API.
#[derive(Debug, Clone)]
pub struct GitHub {
    client: Client,
    api_url: String,
    token: Option<String>,
}

impl GitHub {
    /// The API at `api_url` (`https://api.github.com`), with `token` for private repositories.
    pub fn new(api_url: &str, token: Option<String>) -> Result<Self, String> {
        install_crypto_provider();
        let client = Client::builder()
            .timeout(CALL_TIMEOUT)
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            client,
            api_url: api_url.trim_end_matches('/').to_owned(),
            token,
        })
    }

    /// The issue `issue`.
    ///
    /// # Errors
    ///
    /// Returns the reason when GitHub cannot be reached or does not give the issue.
    pub async fn issue(&self, issue: &IssueRef) -> Result<Issue, String> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}",
            self.api_url, issue.owner, issue.repo, issue.number
        );
        let mut request = self
            .client
            .get(&url)
            .header(USER_AGENT, AGENT)
            .header(ACCEPT, GITHUB_JSON);
        if let Some(token) = &self.token {
            request = request.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = request.send().await.map_err(|error| error.to_string())?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("GitHub answered {status} for {url}"));
        }
        response.json().await.map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_issue_url_names_owner_repo_and_number() {
        assert_eq!(
            parse_issue_url("https://github.com/PikkonMG/blighted-emu/issues/12#issuecomment-1"),
            Some(IssueRef {
                owner: "PikkonMG".to_owned(),
                repo: "blighted-emu".to_owned(),
                number: 12,
            })
        );
        for refused in [
            "https://github.com/a/b/pull/12",
            "https://github.com/a/b/issues/x",
            "https://github.com/../b/issues/1",
            "https://example.com/a/b/issues/1",
            "https://github.com/a/b/issues/1/extra",
        ] {
            assert_eq!(parse_issue_url(refused), None, "{refused}");
        }
    }

    #[tokio::test]
    async fn an_issue_is_read_from_the_api() {
        use axum::Router;
        use axum::routing::get;
        let app = Router::new().route(
            "/repos/a/b/issues/7",
            get(|| async {
                axum::Json(serde_json::json!({
                    "title": "Wolves drop nothing",
                    "body": null,
                    "html_url": "https://github.com/a/b/issues/7",
                    "state": "open",
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move { axum::serve(listener, app).await });
        let github = GitHub::new(&format!("http://{address}/"), None).expect("client");
        let issue = parse_issue_url("https://github.com/a/b/issues/7").expect("url");
        let read = github.issue(&issue).await.expect("issue");
        assert_eq!(read.title, "Wolves drop nothing");
        assert_eq!(read.body, None);
        let missing = parse_issue_url("https://github.com/a/b/issues/8").expect("url");
        assert!(github.issue(&missing).await.is_err());
    }
}
