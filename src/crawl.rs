//! Module for crawling and indexing This Week in Rust content.
//! Handles fetching articles, parsing content, and storing entries in the database.

use anyhow::{bail, Result};
use chrono::NaiveDate;
use headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption;
use headless_chrome::{Browser, LaunchOptionsBuilder, Tab};
use log;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::fs;
use std::time;
use url::Url;

// Input configuration
const GITHUB_OWNER: &str = "rust-lang";
const GITHUB_REPO: &str = "this-week-in-rust";
const GITHUB_BRANCH: &str = "master";

// Output paths configuration
const TWIR_OUT_PATH: &str = "content/twir";
const INDEX_OUT_PATH: &str = "content/index";
const RAW_OUT_PATH: &str = "content/raw";
const SCREENSHOT_OUT_PATH: &str = "content/screenshots";

// List of unsupported file extensions for crawling
const EXCLUDED_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "avif", "pdf"];

/// Represents a unique identifier for a TWiR entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryId {
    pub title: String,
    pub url: Url,
    pub category: String,
    pub date: NaiveDate,
}

impl std::fmt::Display for EntryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let encoded = urlencoding::encode(self.url.as_str());
        write!(f, "{}-{}", self.date, encoded)
    }
}

/// Represents a complete TWiR entry including its content
#[derive(Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: EntryId,
    /// Raw text of website after HTML tags got removed
    pub text: Option<String>,
}

/// Determines if a URL should be crawled based on its file extension
pub fn should_crawl(url: &Url) -> bool {
    EXCLUDED_EXTENSIONS
        .iter()
        .all(|ext| !url.path().ends_with(ext))
}

/// Main indexing function that processes all TWiR content
pub async fn index_all(pool: PgPool) -> Result<()> {
    create_output_directories()?;
    let browser = setup_browser()?;

    let entries = fetch_twir_entries().await?;
    process_entries(&pool, &browser, entries).await?;

    println!("Successfully downloaded all files from the specified path.");
    Ok(())
}

/// Creates necessary output directories
fn create_output_directories() -> Result<()> {
    fs::create_dir_all(TWIR_OUT_PATH)?;
    fs::create_dir_all(INDEX_OUT_PATH)?;
    fs::create_dir_all(RAW_OUT_PATH)?;
    fs::create_dir_all(SCREENSHOT_OUT_PATH)?;
    Ok(())
}

/// Sets up the headless Chrome browser
fn setup_browser() -> Result<Browser> {
    let opt = LaunchOptionsBuilder::default()
        .headless(true)
        .idle_browser_timeout(time::Duration::from_millis(60 * 60_000))
        .build()?;
    Ok(Browser::new(opt)?)
}

/// Fetches TWiR entries from GitHub
async fn fetch_twir_entries() -> Result<Vec<serde_json::Value>> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/contents/content?ref={}",
        GITHUB_OWNER, GITHUB_REPO, GITHUB_BRANCH
    );

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("User-Agent", "Rust GitHub API Client")
        .send()
        .await?;

    response.error_for_status_ref()?;
    Ok(response.json().await?)
}

/// Processes all entries, downloading content and storing in the database
async fn process_entries(
    pool: &PgPool,
    browser: &Browser,
    items: Vec<serde_json::Value>,
) -> Result<()> {
    for item in items {
        let file_name = item["name"].as_str().unwrap();
        let download_url = item["download_url"].as_str().unwrap();
        let download_file_path = format!("{}/{}", TWIR_OUT_PATH, file_name);

        if fs::metadata(&download_file_path).is_ok() {
            log::info!("Skipping: {}", file_name);
            continue;
        }

        process_single_entry(pool, browser, &download_url, &download_file_path).await?;
    }
    Ok(())
}

/// Processes a single TWiR entry
async fn process_single_entry(
    pool: &PgPool,
    browser: &Browser,
    download_url: &str,
    download_file_path: &str,
) -> Result<()> {
    let client = reqwest::Client::new();
    let content = client
        .get(download_url)
        .header("User-Agent", "Rust GitHub API Client")
        .send()
        .await?
        .text()
        .await?;

    fs::write(download_file_path, &content)?;
    log::trace!("Downloaded: {}", download_file_path);

    let entry_ids = parse_file(&content);

    for id in entry_ids {
        if !should_process_url(&id.url) {
            continue;
        }

        let entry_path = format!("{INDEX_OUT_PATH}/{}.json", id);
        if fs::metadata(&entry_path).is_ok() {
            log::info!("Entry exists; skipping: {}", id);
            continue;
        }

        if !should_crawl(&id.url) {
            log::info!("Skipping unsupported file extension: {}", id.url);
            continue;
        }

        match crawl(browser, &id).await {
            Ok(text) => {
                let entry = Entry { id, text };
                let json = serde_json::to_string_pretty(&entry)?;
                fs::write(entry_path, json)?;
                insert_entry(pool, &entry).await?;
            }
            Err(e) => {
                log::error!("Failed to download: {} | Error: {e}", id.url);
            }
        }
    }
    Ok(())
}

/// Determines if a URL should be processed based on various criteria
fn should_process_url(url: &Url) -> bool {
    let supported_protocols = ["http", "https"];
    if !supported_protocols
        .iter()
        .any(|protocol| url.scheme() == *protocol)
    {
        log::info!("Skipping unsupported protocol: {}", url);
        return false;
    }

    let ignored_urls = [
        "github.com",
        "reddit.com",
        "meetup.com",
        "twitter.com",
        "vimeo.com",
        "irc.mozilla.org",
    ];

    if ignored_urls.iter().any(|u| url.to_string().contains(u)) {
        log::info!("Skipping ignored URL: {}", url);
        return false;
    }

    // Skip specific URLs
    let exact_matches = ["http://rust-lang.org/"];
    if exact_matches.iter().any(|u| url.to_string() == *u) {
        log::info!("Skipping exact match URL: {}", url);
        return false;
    }

    true
}

/// Parses raw content into EntryId structs
fn parse_file(content: &str) -> Vec<EntryId> {
    let (meta, body) = content.split_once("\n\n").unwrap();
    let date = parse_date_from(meta).unwrap();
    let body = skip_intro(body);
    content_to_entry_ids(&body, date)
}

/// Parses the date from TWiR metadata
fn parse_date_from(meta: &str) -> Result<NaiveDate> {
    for line in meta.lines() {
        if line.starts_with("Date") {
            let date_str = line.split_once(":").unwrap().1.trim();
            let date_formats = ["%Y-%m-%d", "%Y-%m-%d %H:%M"];

            for format in date_formats.iter() {
                if let Ok(date) = NaiveDate::parse_from_str(date_str, format) {
                    return Ok(date);
                }
            }
            bail!("Found date but failed to parse it: {}", date_str);
        }
    }
    bail!("Did not find post date")
}

/// Skips the intro section of TWiR content
fn skip_intro(body: &str) -> String {
    let mut in_header = false;
    let mut header_line = 0;

    for (i, line) in body.lines().enumerate() {
        if line.trim_start().starts_with("#") {
            in_header = true;
            header_line = i;
            continue;
        }

        if in_header && line.trim_start().starts_with("-") {
            return body
                .lines()
                .skip(header_line)
                .collect::<Vec<&str>>()
                .join("\n");
        }
    }

    body.to_string()
}

/// Converts markdown content to EntryId structs
fn content_to_entry_ids(content: &str, date: NaiveDate) -> Vec<EntryId> {
    let mut entries = Vec::new();
    let mut current_category = String::new();
    let mut current_title = String::new();
    let mut current_url = String::new();
    let mut in_link = false;

    let parser = Parser::new_ext(content, Options::all());

    for event in parser {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                current_category = String::new();
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                in_link = true;
                current_url = dest_url.to_string();
            }
            Event::End(TagEnd::Link) => {
                in_link = false;
                if let Ok(url) = Url::parse(&current_url) {
                    entries.push(EntryId {
                        title: current_title.clone(),
                        url,
                        category: current_category.clone(),
                        date,
                    });
                }
                current_title.clear();
                current_url.clear();
            }
            Event::Text(text) => {
                if current_category.is_empty() {
                    current_category = text.to_string();
                } else if in_link {
                    if current_title.is_empty() {
                        current_title = text.to_string();
                    } else {
                        current_title.push_str(&text);
                    }
                }
            }
            Event::Code(text) => {
                if in_link {
                    current_title.push('`');
                    current_title.push_str(&text);
                    current_title.push('`');
                }
            }
            _ => {}
        }
    }

    entries
}

/// Removes cookie consent banners from a webpage
fn remove_cookie_banner(tab: &Tab) -> Result<()> {
    let selectors = [
        "#lightbox",
        "#cookie-banner",
        ".cookie-banner",
        "#cookieConsent",
        ".cookie-consent",
        "[class*='cookie-consent']",
        "[id*='cookie-consent']",
        "[class*='cookie-notice']",
        "[id*='cookie-notice']",
        "[class*='cookie-policy']",
        "[id*='cookie-policy']",
        "#onetrust-consent-sdk",
        ".CookieConsent",
        "#pz-gdpr",
        ".cookie-disclaimer",
        ".cookie-notice",
        ".cookie-policy",
        ".cookie-popup",
        ".cookie-accept",
        ".cookie-accepts",
        ".cookie-acceptance",
        ".cookie-acceptance-banner",
        ".cookie-acceptance-container",
        ".cookie-acceptance-overlay",
        ".cookie-acceptance-wrapper",
        ".cookie-accepter",
        ".cookie-acceptor",
        "#CybotCookiebotDialog",
        "#disclaimer",
        "#disclaimer-container",
        ".disclaimer",
        ".disclaimer-container",
        "[id*='sp_message_container']",
    ];

    let all_selectors = selectors.join(", ");
    let js = format!(
        r#"
        document.querySelectorAll('{}').forEach(el => el.remove());
        "#,
        all_selectors
    );

    tab.evaluate(&js, false)?;
    log::debug!("Attempted to remove cookie banner elements");
    Ok(())
}

/// Crawls a single webpage and extracts its content
async fn crawl(browser: &Browser, entry_id: &EntryId) -> Result<Option<String>> {
    log::info!("Crawling {}", entry_id.url);

    // Quick check for common error status codes
    let response = reqwest::get(entry_id.url.as_str()).await?;
    if response.status() == 403 || response.status() == 404 || response.status() == 410 {
        bail!("404: Not Found or moved {}", entry_id.url);
    }

    let tab = browser.new_tab()?;
    tab.set_default_timeout(time::Duration::from_secs(30));

    if let Err(e) = tab.navigate_to(entry_id.url.as_str()) {
        return Err(e);
    }

    if let Err(e) = tab.wait_until_navigated() {
        log::error!("Failed to wait for navigation: {}", entry_id.url);
        return Err(e);
    }

    remove_cookie_banner(&tab)?;

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

    // Create screenshot for debugging purposes
    let screenshot_path = format!("{SCREENSHOT_OUT_PATH}/{}.jpg", entry_id);
    log::info!("Creating screenshot {screenshot_path}");
    let screenshot =
        tab.capture_screenshot(CaptureScreenshotFormatOption::Jpeg, Some(75), None, true)?;
    fs::write(screenshot_path, &screenshot)?;
    log::info!("Done creating screenshot");

    tab.close(true)?;

    if text
        .as_ref()
        .map_or(false, |t| t.contains("Verifying you are human"))
    {
        bail!("Captcha detected: {}", entry_id.url);
    }

    Ok(text)
}

/// Inserts or updates an entry in the database
async fn insert_entry(pool: &PgPool, entry: &Entry) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO twir.entries (title, url, category, date, text)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (url) DO UPDATE
        SET title = $1, category = $3, date = $4, text = $5
        "#,
        entry.id.title,
        entry.id.url.as_str(),
        entry.id.category,
        entry.id.date,
        entry.text
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_skip_intro() {
        let input = indoc! {"
            # What's cooking on master?
        
            89 pull requests were merged this week. This is the most pull requests merged
            in a week, ever. 10 1.0 issues were closed this week, and 0 opened.
            
            ## Breaking Changes
             
            - Unique vector patterns (matching on a `~[]`) [has been removed from the
            language](https://github.com/mozilla/rust/pull/12244). One can still match
            against a slice.
        "};

        let expected = indoc! {"
            ## Breaking Changes
             
            - Unique vector patterns (matching on a `~[]`) [has been removed from the
            language](https://github.com/mozilla/rust/pull/12244). One can still match
            against a slice.
        "};

        assert_eq!(skip_intro(input), expected.trim_end());
    }

    #[test]
    fn test_skip_intro_header3() {
        let input = indoc! {"
            ## What's cooking on master?
        
            89 pull requests were merged this week. This is the most pull requests merged
            in a week, ever. 10 1.0 issues were closed this week, and 0 opened.
            
            ### Breaking Changes
             
            - Unique vector patterns (matching on a `~[]`) [has been removed from the
            language](https://github.com/mozilla/rust/pull/12244). One can still match
            against a slice.
        "};

        let expected = indoc! {"
            ### Breaking Changes
             
            - Unique vector patterns (matching on a `~[]`) [has been removed from the
            language](https://github.com/mozilla/rust/pull/12244). One can still match
            against a slice.
        "};

        assert_eq!(skip_intro(input), expected.trim_end());
    }

    #[test]
    fn test_skip_intro_not_found() {
        let input = indoc! {"
            ## What's cooking on master?
        
            89 pull requests were merged this week. This is the most pull requests merged
            in a week, ever. 10 1.0 issues were closed this week, and 0 opened.
        "};

        assert_eq!(skip_intro(input), input);
    }

    #[test]
    fn test_content_parse_category() {
        let content = indoc! {"
                ### Official
                
                * [This Development-cycle in Cargo: 1.81](https://blog.rust-lang.org/inside-rust/2024/08/15/this-development-cycle-in-cargo-1.81.html)
                * [Async Closures MVP: Call for Testing!](https://blog.rust-lang.org/inside-rust/2024/08/09/async-closures-call-for-testing.html)
        "};

        let date = NaiveDate::from_ymd_opt(2024, 8, 21).unwrap();
        let entries = content_to_entry_ids(content, date);

        assert_eq!(entries.len(), 2);
        let first = &entries[0];
        assert_eq!(first.category, "Official");
        assert_eq!(first.title, "This Development-cycle in Cargo: 1.81");
        assert_eq!(
            first.url,
            Url::parse("https://blog.rust-lang.org/inside-rust/2024/08/15/this-development-cycle-in-cargo-1.81.html").unwrap()
        );

        let second = &entries[1];
        assert_eq!(second.category, "Official");
        assert_eq!(second.title, "Async Closures MVP: Call for Testing!");
        assert_eq!(
            second.url,
            Url::parse("https://blog.rust-lang.org/inside-rust/2024/08/09/async-closures-call-for-testing.html").unwrap()
        );
    }

    #[test]
    fn test_code_in_content() {
        let content = indoc! {"
                ### Official
                
                * [This `code` in Cargo](https://example.com)
        "};

        let date = NaiveDate::from_ymd_opt(2024, 8, 21).unwrap();
        let entries = content_to_entry_ids(content, date);

        assert_eq!(entries.len(), 1);
        let first = &entries[0];
        assert_eq!(first.category, "Official");
        assert_eq!(first.title, "This `code` in Cargo");
        assert_eq!(first.url, Url::parse("https://example.com").unwrap());
    }

    #[test]
    fn test_parse_date() {
        let meta = indoc! {"
            Number: 561
            Date: 2024-08-21
        "};

        let date = parse_date_from(meta);
        assert!(date.is_ok());
        assert_eq!(date.unwrap(), NaiveDate::from_ymd_opt(2024, 8, 21).unwrap());
    }

    #[test]
    fn test_parse_date_time() {
        let meta = indoc! {"
            Number: 561
            Date: 2024-08-21 12:00
        "};

        let date = parse_date_from(meta);
        assert!(date.is_ok());
        assert_eq!(date.unwrap(), NaiveDate::from_ymd_opt(2024, 8, 21).unwrap());
    }
}
