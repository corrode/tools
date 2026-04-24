//! Browser automation for crawling web pages

use crate::cookies::COOKIE_BANNER_SELECTORS;
use crate::paths;
use crate::sanitizer::Sanitizer;
use crate::tools::wayback::{self, WAYBACK_SELECTORS};
use types::Metadata;

use anyhow::{Result, bail};
use headless_chrome::{
    Browser as ChromeBrowser, LaunchOptionsBuilder, Tab,
    protocol::cdp::Page::CaptureScreenshotFormatOption,
};
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::{fs, time};

/// Map of domain rewrites (old_domain -> new_domain_prefix)
static URL_REWRITES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        // words.steveklabnik.com/foo -> steveklabnik.com/writing/foo
        ("words.steveklabnik.com", "https://steveklabnik.com/writing"),
    ])
});

/// Guard struct that ensures a tab is closed when dropped
struct TabGuard {
    tab: Arc<Tab>,
}

impl TabGuard {
    fn new(tab: Arc<Tab>) -> Self {
        Self { tab }
    }
}

impl Drop for TabGuard {
    fn drop(&mut self) {
        if let Err(e) = self.tab.close(true) {
            let err_str = e.to_string();
            // These are benign conditions during cleanup:
            // - "Not attached to an active page": tab was already closed/detached
            // - "underlying connection is closed": browser was shut down
            if !err_str.contains("Not attached to an active page")
                && !err_str.contains("underlying connection is closed")
            {
                tracing::warn!("Failed to close tab: {e}");
            }
        }
    }
}

impl std::ops::Deref for TabGuard {
    type Target = Tab;

    fn deref(&self) -> &Self::Target {
        &self.tab
    }
}

// YouTube URL patterns
static YOUTUBE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(https?://)?(www\.)?youtube(-nocookie)?\.com").expect("Invalid YouTube regex")
});

static YOUTUBE_SHORT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(https?://)?(www\.)?(youtu\.?be)").expect("Invalid YouTube short regex")
});

/// Phrases that strongly suggest a page is gone or otherwise unusable,
/// warranting a Wayback Machine fallback.
///
/// Matched case-insensitively against the extracted page text. The list is
/// intentionally conservative — false positives mean we waste a Wayback
/// lookup, but false negatives mean we silently index a useless 404 page.
static GONE_PHRASES: &[&str] = &[
    "404 not found",
    "page not found",
    "no longer available",
    "has been removed",
    "has been deleted",
    "this page does not exist",
    "410 gone",
];

/// Wrapper around headless Chrome for crawling web pages
pub struct Browser {
    inner: ChromeBrowser,
    debug: bool,
}

impl Browser {
    /// Creates a new browser instance
    ///
    /// # Arguments
    /// * `debug` - If true, saves raw HTML/Markdown and screenshots to disk for future analysis
    pub fn new(debug: bool) -> Result<Self> {
        let mut args = Vec::new();
        if std::env::var("CHROME_NO_SANDBOX").is_ok() {
            args.push(std::ffi::OsStr::new("--no-sandbox"));
        }

        let opt = LaunchOptionsBuilder::default()
            .headless(true)
            .window_size(Some((1920, 1080)))
            .idle_browser_timeout(time::Duration::from_millis(60 * 60_000))
            .args(args)
            .build()?;

        Ok(Self {
            inner: ChromeBrowser::new(opt)?,
            debug,
        })
    }

    /// Rewrites URLs that have moved to new domains.
    /// This is public so callers can rewrite URLs before passing to crawl.
    pub fn rewrite_url(url: &url::Url) -> url::Url {
        // Check domain rewrite map
        if let Some(host) = url.host_str()
            && let Some(new_prefix) = URL_REWRITES.get(host)
            && let Ok(new_url) = url::Url::parse(&format!("{}{}", new_prefix, url.path()))
        {
            tracing::debug!("Rewrote {} -> {}", url, new_url);
            return new_url;
        }

        // YouTube URLs -> thumbnail to avoid cookie banners
        if let Some(rewritten) = Self::rewrite_youtube_url(url) {
            return rewritten;
        }

        url.clone()
    }

    /// Rewrites YouTube URLs to thumbnail URLs to avoid cookie banners
    fn rewrite_youtube_url(url: &url::Url) -> Option<url::Url> {
        let url_str = url.as_str();

        // Check if it's a YouTube URL
        if !YOUTUBE_PATTERN.is_match(url_str) && !YOUTUBE_SHORT_PATTERN.is_match(url_str) {
            return None;
        }

        // Extract video ID based on URL format
        let video_id = if url.path() == "/watch" {
            // Standard format: youtube.com/watch?v=VIDEO_ID
            url.query_pairs()
                .find(|(key, _)| key == "v")
                .map(|(_, value)| value.to_string())
        } else if url.path().starts_with("/embed/") {
            // Embed format: youtube.com/embed/VIDEO_ID
            url.path().strip_prefix("/embed/").map(String::from)
        } else if YOUTUBE_SHORT_PATTERN.is_match(url_str) {
            // Short format: youtu.be/VIDEO_ID
            url.path().strip_prefix('/').map(String::from)
        } else {
            None
        };

        // Rewrite to thumbnail URL if we got a video ID
        video_id
            .and_then(|id| url::Url::parse(&format!("https://img.youtube.com/vi/{id}/0.jpg")).ok())
    }

    /// Returns true if the extracted page text strongly suggests the page is
    /// gone (404, removed, etc.) and should trigger a Wayback fallback.
    fn looks_gone(text: &str) -> bool {
        let lower = text.to_lowercase();
        GONE_PHRASES.iter().any(|phrase| lower.contains(phrase))
    }

    /// Crawls a webpage and returns its text content.
    ///
    /// If the live page fails to load or appears to be a 404 / removed
    /// page, automatically retries via the Wayback Machine before giving up.
    pub fn crawl(&self, metadata: &Metadata) -> anyhow::Result<Option<String>> {
        tracing::info!("Crawling {}", metadata.url);

        // Rewrite URLs (domain migrations, YouTube thumbnails, etc.)
        let target_url = Self::rewrite_url(&metadata.url);

        if target_url != *metadata.url {
            tracing::info!("Rewrote URL: {} -> {}", metadata.url, target_url);
        }

        // For YouTube thumbnails, we can skip browser rendering
        if target_url.host_str() == Some("img.youtube.com") {
            tracing::info!("YouTube thumbnail, skipping browser rendering");
            return Ok(Some(format!("YouTube video: {}", metadata.title)));
        }

        match self.crawl_url(&target_url, false, Some(metadata)) {
            Ok(text) if Self::looks_gone(&text) => {
                tracing::warn!(
                    "Live page for {} appears to be gone, trying Wayback Machine...",
                    metadata.url
                );
                self.crawl_wayback(&metadata.url)
            }
            Ok(text) => Ok(Some(text)),
            Err(live_err) => {
                tracing::warn!(
                    "Live page failed for {} ({live_err}), trying Wayback Machine...",
                    metadata.url
                );
                self.crawl_wayback(&metadata.url)
            }
        }
    }

    /// Performs a single browser crawl of `target_url`.
    ///
    /// `is_wayback` controls whether Wayback-specific chrome is stripped from
    /// the page in addition to cookie banners. `metadata` is used only for
    /// debug-mode artefact filenames; it's `None` for Wayback retries.
    fn crawl_url(
        &self,
        target_url: &url::Url,
        is_wayback: bool,
        metadata: Option<&Metadata>,
    ) -> Result<String> {
        // Use TabGuard to ensure tab is closed on all paths (success, error, or panic)
        let tab = TabGuard::new(self.inner.new_tab()?);
        tab.set_default_timeout(time::Duration::from_secs(30));

        tracing::debug!("Navigating to URL: {}", target_url);
        tab.navigate_to(target_url.as_str())?;

        tracing::debug!("Waiting for navigation to complete (30s timeout)...");
        if let Err(e) = tab.wait_until_navigated() {
            tracing::error!("Navigation timeout or error for {}: {e}", target_url);
            bail!("Navigation failed: {e}");
        }
        tracing::debug!("Navigation completed successfully");

        if is_wayback {
            self.remove_wayback_chrome(&tab)?;
        }
        self.remove_cookie_banner(&tab)?;

        // Wait for Cloudflare verification to complete (if present)
        // Poll up to 15 seconds for the verification page to disappear
        let text = self.wait_for_content(&tab, 15)?;
        tracing::debug!("Extracted text: {} chars", text.len());

        if self.debug
            && let Some(metadata) = metadata
        {
            let html = tab.get_content()?;
            self.save_raw_html(&html, metadata)?;
            self.take_screenshot(&tab, metadata)?;
        }

        // Tab is automatically closed when `tab` (TabGuard) is dropped

        Ok(text)
    }

    /// Looks up the Wayback Machine for an archived copy of `original_url`
    /// and crawls it. Returns `Ok(None)` if no usable snapshot was found.
    fn crawl_wayback(&self, original_url: &url::Url) -> Result<Option<String>> {
        let Some(wayback_url) = wayback::wayback_url_for(original_url) else {
            tracing::warn!("Could not construct Wayback URL for {original_url}");
            return Ok(None);
        };

        tracing::info!("Trying Wayback snapshot: {wayback_url}");

        match self.crawl_url(&wayback_url, true, None) {
            Ok(text) if Self::looks_gone(&text) => {
                tracing::warn!("Wayback snapshot for {original_url} also looks gone");
                Ok(None)
            }
            Ok(text) => {
                tracing::info!("Wayback fallback succeeded for {original_url}");
                Ok(Some(text))
            }
            Err(e) => {
                tracing::warn!("Wayback snapshot failed for {original_url}: {e}");
                Ok(None)
            }
        }
    }

    /// Takes a screenshot of the current page
    fn take_screenshot(&self, tab: &TabGuard, metadata: &Metadata) -> Result<()> {
        let screenshot_path = format!("{}/{metadata}.jpg", *paths::SCREENSHOT_PATH);
        tracing::info!("Creating screenshot {screenshot_path}");
        let screenshot =
            tab.capture_screenshot(CaptureScreenshotFormatOption::Jpeg, Some(75), None, true)?;
        fs::write(screenshot_path, &screenshot)?;
        tracing::info!("Done creating screenshot");
        Ok(())
    }

    /// Saves raw HTML to disk for future analysis
    fn save_raw_html(&self, html: &str, metadata: &Metadata) -> Result<()> {
        let html_path = format!("{}/{metadata}.html", *paths::HTML_PATH);
        tracing::info!("Saving raw HTML to {html_path}");
        fs::write(html_path, html)?;
        tracing::debug!("Raw HTML saved successfully");
        Ok(())
    }

    /// Waits for page content, handling Cloudflare verification pages
    fn wait_for_content(&self, tab: &TabGuard, max_wait_secs: u64) -> Result<String> {
        let start = time::Instant::now();
        let poll_interval = time::Duration::from_millis(500);
        let max_wait = time::Duration::from_secs(max_wait_secs);

        loop {
            let html = tab.get_content()?;
            tracing::trace!("HTML: {html}");

            let text = Sanitizer::sanitize(&html)?;

            // Check for Cloudflare verification page
            if text.contains("Verifying you are human")
                || text.contains("Just a moment")
                || text.contains("Checking your browser")
            {
                if start.elapsed() >= max_wait {
                    bail!("Timeout waiting for Cloudflare verification to complete");
                }
                tracing::debug!(
                    "Cloudflare verification in progress, waiting... ({:.1}s)",
                    start.elapsed().as_secs_f32()
                );
                std::thread::sleep(poll_interval);
                continue;
            }

            return Ok(text);
        }
    }

    /// Removes cookie consent banners from a webpage
    fn remove_cookie_banner(&self, tab: &TabGuard) -> Result<()> {
        let all_selectors = COOKIE_BANNER_SELECTORS.join(", ");
        let js =
            format!(r#"document.querySelectorAll('{all_selectors}').forEach(el => el.remove());"#);

        tab.evaluate(&js, false)?;
        tracing::debug!("Attempted to remove cookie banner elements");
        Ok(())
    }

    /// Removes Wayback Machine toolbar and other archive.org injected chrome
    /// from the page so they don't pollute the extracted text.
    fn remove_wayback_chrome(&self, tab: &TabGuard) -> Result<()> {
        let all_selectors = WAYBACK_SELECTORS.join(", ");
        let js =
            format!(r#"document.querySelectorAll('{all_selectors}').forEach(el => el.remove());"#);

        tab.evaluate(&js, false)?;
        tracing::debug!("Attempted to remove Wayback Machine chrome");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rewrite_youtube_watch_url() {
        let url = url::Url::parse("https://www.youtube.com/watch?v=dQw4w9WgXcQ").unwrap();
        let rewritten = Browser::rewrite_youtube_url(&url);

        assert!(rewritten.is_some());
        assert_eq!(
            rewritten.unwrap().as_str(),
            "https://img.youtube.com/vi/dQw4w9WgXcQ/0.jpg"
        );
    }

    #[test]
    fn test_rewrite_youtube_embed_url() {
        let url = url::Url::parse("https://www.youtube.com/embed/dQw4w9WgXcQ").unwrap();
        let rewritten = Browser::rewrite_youtube_url(&url);

        assert!(rewritten.is_some());
        assert_eq!(
            rewritten.unwrap().as_str(),
            "https://img.youtube.com/vi/dQw4w9WgXcQ/0.jpg"
        );
    }

    #[test]
    fn test_rewrite_youtube_short_url() {
        let url = url::Url::parse("https://youtu.be/dQw4w9WgXcQ").unwrap();
        let rewritten = Browser::rewrite_youtube_url(&url);

        assert!(rewritten.is_some());
        assert_eq!(
            rewritten.unwrap().as_str(),
            "https://img.youtube.com/vi/dQw4w9WgXcQ/0.jpg"
        );
    }

    #[test]
    fn test_rewrite_youtube_nocookie_url() {
        let url = url::Url::parse("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ").unwrap();
        let rewritten = Browser::rewrite_youtube_url(&url);

        assert!(rewritten.is_some());
        assert_eq!(
            rewritten.unwrap().as_str(),
            "https://img.youtube.com/vi/dQw4w9WgXcQ/0.jpg"
        );
    }

    #[test]
    fn test_non_youtube_url_not_rewritten() {
        let url = url::Url::parse("https://example.com/watch?v=test").unwrap();
        let rewritten = Browser::rewrite_youtube_url(&url);

        assert!(rewritten.is_none());
    }
}
