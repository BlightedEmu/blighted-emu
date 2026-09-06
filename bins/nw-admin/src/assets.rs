// SPDX-License-Identifier: AGPL-3.0-or-later
//! The web files, built into the program (no build step, no package).

/// Content type of HTML pages.
pub const HTML: &str = "text/html; charset=utf-8";
/// Content type of style sheets.
pub const CSS: &str = "text/css; charset=utf-8";

/// One web file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset {
    /// `Content-Type` value.
    pub content_type: &'static str,
    /// The file.
    pub body: &'static str,
}

/// The web file of `name`; `None` for any other name.
pub fn asset(name: &str) -> Option<Asset> {
    let (content_type, body) = match name {
        "index.html" => (HTML, include_str!("../web/index.html")),
        "style.css" => (CSS, include_str!("../web/style.css")),
        _ => return None,
    };
    Some(Asset { content_type, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_files_have_their_type_and_others_none() {
        assert_eq!(
            asset("index.html").map(|file| file.content_type),
            Some(HTML)
        );
        assert_eq!(asset("style.css").map(|file| file.content_type), Some(CSS));
        assert!(asset("../Cargo.toml").is_none());
        assert!(asset("missing.js").is_none());
    }
}
