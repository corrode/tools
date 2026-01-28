//! Browser automation for crawling web pages

use super::cookies::COOKIE_BANNER_SELECTORS;
use super::sanitizer::Sanitizer;
use types::{EntryId, get_html_path, get_screenshot_path};

use anyhow::{Result, bail};
use headless_chrome::{
    Browser as ChromeBrowser, LaunchOptionsBuilder, Tab,
    protocol::cdp::Page::CaptureScreenshotFormatOption,
};
use regex::Regex;
use std::sync::LazyLock;
use std::{fs, time};

// YouTube URL patterns
static YOUTUBE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(https?://)?(www\.)?youtube(-nocookie)?\.com").expect("Invalid YouTube regex")
});

static YOUTUBE_SHORT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(https?://)?(www\.)?(youtu\.?be)").expect("Invalid YouTube short regex")
});

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

    /// Crawls a webpage and returns its text content
    pub async fn crawl(&self, entry_id: &EntryId) -> Result<Option<String>> {
        log::info!("Crawling {}", entry_id.url);

        // Rewrite YouTube URLs to thumbnail URLs to avoid cookie banners
        let target_url =
            Self::rewrite_youtube_url(&entry_id.url).unwrap_or_else(|| entry_id.url.clone());

        if target_url != entry_id.url {
            log::info!("Rewritten YouTube URL to thumbnail: {target_url}");
        }

        // Quick check for common error status codes with timeout
        let client = reqwest::Client::builder()
            .connect_timeout(time::Duration::from_secs(10))
            .timeout(time::Duration::from_secs(30))
            .build()?;

        let response = client.get(target_url.as_str()).send().await?;
        if response.status() == 403 || response.status() == 404 || response.status() == 410 {
            bail!("404: Not Found or moved {}", entry_id.url);
        }

        // Exclude server errors (5xx)
        if response.status().is_server_error() {
            bail!("5xx: Server error {}", entry_id.url);
        }

        // For YouTube thumbnails, we can skip browser rendering
        if target_url != entry_id.url {
            log::info!("YouTube thumbnail verified, skipping browser rendering");
            return Ok(Some(format!("YouTube video: {}", entry_id.title)));
        }

        let tab = self.inner.new_tab()?;
        tab.set_default_timeout(time::Duration::from_secs(30));

        log::debug!("Navigating to URL: {}", entry_id.url);
        tab.navigate_to(entry_id.url.as_str())?;

        log::debug!("Waiting for navigation to complete (30s timeout)...");
        if let Err(e) = tab.wait_until_navigated() {
            log::error!("Navigation timeout or error for {}: {e}", entry_id.url);
            bail!("Navigation failed: {e}");
        }
        log::debug!("Navigation completed successfully");

        self.remove_cookie_banner(&tab)?;

        let text = match tab.get_content() {
            Ok(html) => {
                log::trace!("HTML: {html}");

                // Save raw HTML if flag is enabled
                if self.debug {
                    self.save_raw_html(&html, entry_id)?;
                }

                // Sanitize HTML and extract plain text content
                // dom_smoothie handles both cleaning and text extraction
                let text = Sanitizer::sanitize(&html)?;
                log::debug!("Extracted text: {} chars", text.len());
                Some(text)
            }
            Err(e) => {
                return Err(e);
            }
        };

        if self.debug {
            self.take_screenshot(&tab, entry_id)?;
        }
        tab.close(true)?;

        if text
            .as_ref()
            .is_some_and(|t| t.contains("Verifying you are human"))
        {
            bail!("Captcha detected: {}", entry_id.url);
        }

        Ok(text)
    }

    /// Takes a screenshot of the current page
    fn take_screenshot(&self, tab: &Tab, entry_id: &EntryId) -> Result<()> {
        let screenshot_path = format!("{}/{entry_id}.jpg", get_screenshot_path());
        log::info!("Creating screenshot {screenshot_path}");
        let screenshot =
            tab.capture_screenshot(CaptureScreenshotFormatOption::Jpeg, Some(75), None, true)?;
        fs::write(screenshot_path, &screenshot)?;
        log::info!("Done creating screenshot");
        Ok(())
    }

    /// Saves raw HTML to disk for future analysis
    fn save_raw_html(&self, html: &str, entry_id: &EntryId) -> Result<()> {
        let html_path = format!("{}/{entry_id}.html", get_html_path());
        log::info!("Saving raw HTML to {html_path}");
        fs::write(html_path, html)?;
        log::debug!("Raw HTML saved successfully");
        Ok(())
    }

    /// Removes cookie consent banners from a webpage
    fn remove_cookie_banner(&self, tab: &Tab) -> Result<()> {
        let all_selectors = COOKIE_BANNER_SELECTORS.join(", ");
        let js =
            format!(r#"document.querySelectorAll('{all_selectors}').forEach(el => el.remove());"#);

        tab.evaluate(&js, false)?;
        log::debug!("Attempted to remove cookie banner elements");
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
