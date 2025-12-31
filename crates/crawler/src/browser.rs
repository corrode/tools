//! Browser automation for crawling web pages

use super::cookies::COOKIE_BANNER_SELECTORS;
use super::SCREENSHOT_OUT_PATH;
use types::EntryId;

use anyhow::{bail, Result};
use headless_chrome::{
    protocol::cdp::Page::CaptureScreenshotFormatOption, Browser as ChromeBrowser,
    LaunchOptionsBuilder, Tab,
};
use std::{fs, time};

/// Wrapper around headless Chrome for crawling web pages
pub struct Browser {
    inner: ChromeBrowser,
}

impl Browser {
    /// Creates a new browser instance
    pub fn new() -> Result<Self> {
        let opt = LaunchOptionsBuilder::default()
            .headless(true)
            .idle_browser_timeout(time::Duration::from_millis(60 * 60_000))
            .build()?;

        Ok(Self {
            inner: ChromeBrowser::new(opt)?,
        })
    }

    /// Crawls a webpage and returns its text content
    pub async fn crawl(&self, entry_id: &EntryId) -> Result<Option<String>> {
        log::info!("Crawling {}", entry_id.url);

        // Quick check for common error status codes
        let response = reqwest::get(entry_id.url.as_str()).await?;
        if response.status() == 403 || response.status() == 404 || response.status() == 410 {
            bail!("404: Not Found or moved {}", entry_id.url);
        }

        let tab = self.inner.new_tab()?;
        tab.set_default_timeout(time::Duration::from_secs(30));

        tab.navigate_to(entry_id.url.as_str())?;

        if let Err(e) = tab.wait_until_navigated() {
            log::error!("Failed to wait for navigation: {}", entry_id.url);
            return Err(e);
        }

        self.remove_cookie_banner(&tab)?;

        let text = match tab.get_content() {
            Ok(html) => {
                log::trace!("HTML: {html}");
                let cleaned = html2text::from_read(html.as_bytes(), 500);
                log::debug!("Cleaned: {cleaned}");
                Some(cleaned)
            }
            Err(e) => {
                return Err(e);
            }
        };

        self.take_screenshot(&tab, entry_id)?;
        tab.close(true)?;

        if text
            .as_ref()
            .map_or(false, |t| t.contains("Verifying you are human"))
        {
            bail!("Captcha detected: {}", entry_id.url);
        }

        Ok(text)
    }

    /// Takes a screenshot of the current page
    fn take_screenshot(&self, tab: &Tab, entry_id: &EntryId) -> Result<()> {
        let screenshot_path = format!("{SCREENSHOT_OUT_PATH}/{}.jpg", entry_id);
        log::info!("Creating screenshot {screenshot_path}");
        let screenshot =
            tab.capture_screenshot(CaptureScreenshotFormatOption::Jpeg, Some(75), None, true)?;
        fs::write(screenshot_path, &screenshot)?;
        log::info!("Done creating screenshot");
        Ok(())
    }

    /// Removes cookie consent banners from a webpage
    fn remove_cookie_banner(&self, tab: &Tab) -> Result<()> {
        let all_selectors = COOKIE_BANNER_SELECTORS.join(", ");
        let js = format!(
            r#"document.querySelectorAll('{}').forEach(el => el.remove());"#,
            all_selectors
        );

        tab.evaluate(&js, false)?;
        log::debug!("Attempted to remove cookie banner elements");
        Ok(())
    }
}
