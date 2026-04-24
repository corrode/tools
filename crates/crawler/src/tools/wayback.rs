//! Wayback Machine integration for fetching archived versions of dead pages.
//!
//! Uses the `web/0/` shorthand:
//! `https://web.archive.org/web/0/<url>`
//!
//! Wayback Machine interprets timestamp `0` as "give me the best (newest)
//! available snapshot", redirecting to the actual timestamped URL. This is
//! simpler and more reliable than the Availability JSON API, which is heavily
//! rate-limited and frequently returns empty results for pages that are clearly
//! archived.

/// Selectors for Wayback Machine-injected chrome that should be stripped before
/// extracting text content from an archived page.
pub static WAYBACK_SELECTORS: &[&str] = &[
    // Main toolbar
    "#wm-ipp-base",
    "#wm-ipp",
    // Donation/fundraising banner
    "#donato",
    "#wm-donate",
    // "About this capture" overlay
    "#wmtb",
    // Close/nav buttons injected into the archived page
    "#wm-tb-tg",
    // Any remaining archive.org injected wrappers
    "#playback",
    "#wm-share",
    "#wm-save",
];

/// Constructs a Wayback Machine URL for the best available snapshot of `url`.
///
/// Wayback interprets timestamp `0` as "newest available snapshot" and issues
/// a redirect to the actual timestamped URL. We return the constructed URL
/// directly — the browser will follow the redirect naturally.
///
/// Returns `None` only if the URL cannot be serialised into the Wayback path,
/// which should never happen in practice.
pub fn wayback_url_for(url: &url::Url) -> Option<url::Url> {
    let wayback = format!("https://web.archive.org/web/0/{}", url.as_str());
    match url::Url::parse(&wayback) {
        Ok(u) => {
            tracing::debug!("Wayback URL for {url}: {u}");
            Some(u)
        }
        Err(e) => {
            tracing::warn!("Failed to construct Wayback URL for {url}: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wayback_url_construction() {
        let url = url::Url::parse("http://edunham.net/2016/04/11/plushie_rustacean_pattern.html")
            .unwrap();
        let wayback = wayback_url_for(&url).unwrap();
        assert_eq!(
            wayback.as_str(),
            "https://web.archive.org/web/0/http://edunham.net/2016/04/11/plushie_rustacean_pattern.html"
        );
    }

    #[test]
    fn test_wayback_selectors_are_valid() {
        // Smoke-test: all selectors are non-empty strings
        for selector in WAYBACK_SELECTORS {
            assert!(!selector.is_empty());
        }
    }
}
