//! Wayback Machine integration for fetching archived versions of dead pages.
//!
//! Builds URLs of the form:
//!
//! ```text
//! https://web.archive.org/web/0if_/<url>
//! ```
//!
//! Two URL modifiers are in play here:
//!
//! - **`0`** as the timestamp tells Wayback "give me the best (newest)
//!   available snapshot" — it serves a redirect to the actual timestamped
//!   URL, so the browser just follows it. Simpler and far more reliable
//!   than the Availability JSON API, which is heavily rate-limited and
//!   frequently returns empty results for pages that are clearly archived.
//!
//! - **`if_`** is the "iframe" render flag. It tells Wayback to omit its
//!   navigational toolbar / donation banner / "About this capture" overlay
//!   while still rewriting CSS and image references to the archived
//!   originals. Without it, we'd have to strip the injected chrome from
//!   the DOM ourselves; with it, the page renders cleanly out of the box.
//!
//! See <https://help.archive.org/help/using-the-wayback-machine/> and the
//! community documentation on render flags (`id_`, `if_`, `ij_`).

use anyhow::{Context, Result};

/// Constructs a Wayback Machine URL for the best available snapshot of `url`,
/// rendered without the Wayback toolbar (`if_` iframe flag).
///
/// Returns an error only if the URL cannot be serialised into the Wayback
/// path, which should never happen for any URL the crawler produces.
pub fn wayback_url_for(url: &url::Url) -> Result<url::Url> {
    let wayback = format!("https://web.archive.org/web/0if_/{url}");
    url::Url::parse(&wayback).with_context(|| format!("Failed to construct Wayback URL for {url}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_uses_iframe_flag_and_zero_timestamp() {
        let url = url::Url::parse("http://edunham.net/2016/04/11/plushie_rustacean_pattern.html")
            .unwrap();
        assert_eq!(
            wayback_url_for(&url).unwrap().as_str(),
            "https://web.archive.org/web/0if_/http://edunham.net/2016/04/11/plushie_rustacean_pattern.html"
        );
    }

    #[test]
    fn url_with_query_string_is_preserved() {
        let url = url::Url::parse("https://example.com/page?id=42&lang=en").unwrap();
        let wayback = wayback_url_for(&url).unwrap();
        assert!(wayback.as_str().ends_with("?id=42&lang=en"));
        assert!(wayback.as_str().contains("/web/0if_/"));
    }
}
