// SPDX-License-Identifier: AGPL-3.0-or-later
//! Scrapes the metrics pages every `scrape_interval_ms` (admin tools spec section 5.2). Each
//! round also samples CPU and lets silent sims leave. A page that is down or answers no
//! Prometheus samples is "not up"; the other pages are still read.

use std::time::Duration;

use nw_common::prometheus::Scrape;
use nw_common::time::unix_ms;
use nw_login::install_crypto_provider;
use reqwest::Client;
use tokio::time::MissedTickBehavior;

use crate::live::cpu::CpuSampler;
use crate::live::{SharedState, lock};

const NO_SAMPLES: &str = "the page has no Prometheus samples";

/// The samples of a page; an error when it has none.
///
/// # Errors
///
/// When `text` holds no Prometheus sample line.
pub fn read_page(text: &str) -> Result<Scrape, String> {
    let scrape = Scrape::parse(text);
    if scrape.is_empty() {
        Err(NO_SAMPLES.to_owned())
    } else {
        Ok(scrape)
    }
}

async fn fetch(http: &Client, url: &str) -> Result<Scrape, String> {
    let response = http
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| error.to_string())?;
    let text = response.text().await.map_err(|error| error.to_string())?;
    read_page(&text)
}

/// Every `interval`: scrapes every page of `urls` (each within `interval`), samples CPU and lets
/// silent sims leave. Runs until the task is dropped. Installs the TLS crypto provider that the
/// HTTP client needs.
pub async fn run(urls: Vec<String>, interval: Duration, mut cpu: CpuSampler, state: SharedState) {
    install_crypto_provider();
    let http = match Client::builder().timeout(interval).build() {
        Ok(http) => http,
        Err(error) => {
            tracing::warn!(%error, "no HTTP client: the metrics pages are not read");
            return;
        }
    };
    let mut timer = tokio::time::interval(interval);
    timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        timer.tick().await;
        let results =
            futures_util::future::join_all(urls.iter().map(|url| fetch(&http, url))).await;
        let now_ms = unix_ms();
        let per_name = cpu.sample(now_ms);
        let mut live = lock(&state);
        for (url, result) in urls.iter().zip(results) {
            if let Err(error) = &result {
                tracing::debug!(%url, %error, "metrics page not read");
            }
            live.scrape(url, result, now_ms);
        }
        live.cpu(per_name);
        live.expire(now_ms);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::live::shared;

    const METRICS: &str = "rep_gateway_datagrams_in_total 5\n";
    const INTERVAL: Duration = Duration::from_millis(50);
    const WAIT: Duration = Duration::from_secs(5);

    #[test]
    fn a_page_without_samples_is_not_read() {
        assert!(read_page("<html>a login page</html>").is_err());
        assert!(read_page(METRICS).is_ok());
    }

    #[tokio::test]
    async fn one_down_page_does_not_stop_the_other_scrapes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let up = format!("http://{}/metrics", listener.local_addr().expect("address"));
        let app = axum::Router::new().route("/metrics", axum::routing::get(|| async { METRICS }));
        tokio::spawn(async move { axum::serve(listener, app).await });
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let down = format!("http://{}/metrics", closed.local_addr().expect("address"));
        drop(closed);
        let state = shared();
        let dir = tempfile::tempdir().expect("dir");
        let cpu = CpuSampler::new(dir.path().to_path_buf(), Vec::new());
        let task = tokio::spawn(run(
            vec![down.clone(), up.clone()],
            INTERVAL,
            cpu,
            state.clone(),
        ));
        let seen = tokio::time::timeout(WAIT, async {
            loop {
                let metrics = crate::live::lock(&state).view().metrics;
                if metrics.len() == 2 {
                    return metrics;
                }
                tokio::time::sleep(INTERVAL).await;
            }
        })
        .await
        .expect("both pages scraped");
        task.abort();
        let up_of = |url: &str| seen.iter().find(|page| page.url == url).map(|page| page.up);
        assert_eq!((up_of(&up), up_of(&down)), (Some(true), Some(false)));
    }
}
