use anyhow::bail;
use anyhow::Result;
use chrono::NaiveDate;
use headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption;
use headless_chrome::Browser;
use headless_chrome::LaunchOptionsBuilder;
use pulldown_cmark::{Event, Parser, Tag};
use pulldown_cmark::{Options, TagEnd};
use serde::Deserialize;
use serde::Serialize;
use std::fs;
use std::time;
use url::Url;

// List of unsupported file extensions for crawling
const EXCLUDED_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "avif", "pdf"];

// Input
const GITHUB_OWNER: &str = "rust-lang";
const GITHUB_REPO: &str = "this-week-in-rust";
const GITHUB_BRANCH: &str = "master";

// Output
const TWIR_OUT_PATH: &str = "content/twir";
const INDEX_OUT_PATH: &str = "content/index";
const RAW_OUT_PATH: &str = "content/raw";
const SCREENSHOT_OUT_PATH: &str = "content/screenshots";

/// Entry from TWiR markdown file
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EntryId {
    title: String,
    url: Url,
    category: String,
    date: NaiveDate,
}

impl std::fmt::Display for EntryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let encoded = urlencoding::encode(self.url.as_str());
        write!(f, "{}-{}", self.date, encoded)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    id: EntryId,
    /// Raw text of website after HTML tags got removed
    text: Option<String>,
}

fn content_to_entry_ids(content: &str, date: NaiveDate) -> Vec<EntryId> {
    let mut entries = Vec::new();

    let mut current_category = String::new();
    let mut current_title = String::new();
    let mut current_url = String::new();
    let mut in_link = false;

    let parser = Parser::new_ext(content, Options::all());

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level: _level, .. }) => {
                current_category = String::new();
            }
            Event::End(TagEnd::Heading(_)) => {}

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
                } else {
                    println!("Failed to parse URL: {}", current_url);
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
                    // push with backticks
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

fn parse_file(content: &str) -> Vec<EntryId> {
    let (meta, body) = content.split_once("\n\n").unwrap();

    let date = parse_date_from(meta).unwrap();
    let body = skip_intro(body);
    content_to_entry_ids(&body, date)
}

/// Parses the date from this format:
/// Number: 561
/// Date: 2024-08-21
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

/// Skips the intro of the TWiR markdown file
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

    // not found
    body.to_string()
}

// Crawl a page if it doesn't end an extension on the list of unsupported extensions
fn should_crawl(url: &Url) -> bool {
    EXCLUDED_EXTENSIONS
        .iter()
        .all(|ext| !url.path().ends_with(ext))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();

    fs::create_dir_all(TWIR_OUT_PATH)?;
    fs::create_dir_all(INDEX_OUT_PATH)?;
    fs::create_dir_all(RAW_OUT_PATH)?;
    fs::create_dir_all(SCREENSHOT_OUT_PATH)?;

    let url = format!(
        "https://api.github.com/repos/{}/{}/contents/content?ref={}",
        GITHUB_OWNER, GITHUB_REPO, GITHUB_BRANCH
    );

    let client = reqwest::Client::new();
    let twir_api_response = client
        .get(&url)
        .header("User-Agent", "Rust GitHub API Client")
        .send()
        .await?;

    // Check that the response is successful
    twir_api_response.error_for_status_ref()?;

    let items = twir_api_response.json::<Vec<serde_json::Value>>().await?;

    let opt = LaunchOptionsBuilder::default()
        .headless(true)
        .idle_browser_timeout(time::Duration::from_millis(60 * 60_000)) // Set to a very long time to avoid timeouts in general
        .build()?;
    let browser = Browser::new(opt)?;

    for item in items {
        let file_name = item["name"].as_str().unwrap();
        let download_url = item["download_url"].as_str().unwrap();
        let download_file_path = format!("{}/{}", TWIR_OUT_PATH, file_name);

        if fs::metadata(&download_file_path).is_ok() {
            log::info!("Skipping: {}", file_name);
            continue;
        }

        let content = client
            .get(download_url)
            .header("User-Agent", "Rust GitHub API Client")
            .send()
            .await?
            .text()
            .await?;

        log::trace!("Downloaded: {}", file_name);
        fs::write(download_file_path, &content)?;

        let entry_ids = parse_file(&content);

        // Store the parsed entries in a database
        for id in entry_ids {
            let supported_protocols = ["http", "https"];

            if !supported_protocols
                .iter()
                .any(|protocol| id.url.scheme() == *protocol)
            {
                log::info!("Skipping unsupported protocol: {}", id.url);
                continue;
            }

            let ignored_urls = ["github.com", "reddit.com", "meetup.com", "twitter.com"];

            if ignored_urls
                .iter()
                .any(|url| id.url.to_string().contains(url))
            {
                log::info!("Skipping ignored URL: {}", id.url);
                continue;
            }

            let entry_path = format!("{INDEX_OUT_PATH}/{}.json", id);

            if fs::metadata(&entry_path).is_ok() {
                log::info!("Entry exists; skipping: {}", id);
                continue;
            }

            // Only crawl certain file extensions like `.html` or no extensions at all like `/` at the end
            if !should_crawl(&id.url) {
                log::info!("Skipping unsupported file extension: {}", id.url);
                continue;
            }

            let text = match crawl(&browser, &id).await {
                Ok(text) => text,
                Err(e) => {
                    log::error!("Failed to download: {}; Error: {e}", id.url);
                    continue;
                }
            };

            let entry = Entry { id, text };
            let json = serde_json::to_string_pretty(&entry)?;
            log::debug!("Writing to: {entry_path}");
            fs::write(entry_path, json)?;
        }
    }

    println!("Successfully downloaded all files from the specified path.");
    Ok(())
}

async fn crawl(browser: &Browser, entry_id: &EntryId) -> Result<Option<String>> {
    log::info!("Crawling {}", entry_id.url); // Changed from {url} to {id.url}

    // Quick check: if reqwest returns a known error code, return an error
    let response = reqwest::get(entry_id.url.as_str()).await?;
    if response.status() == 404 || response.status() == 410 {
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

    // "Debugging": create screenshot and store
    let screenshot_path = format!("{SCREENSHOT_OUT_PATH}/{}.jpg", entry_id);
    log::info!("Creating screenshot {screenshot_path}");
    let screenshot =
        tab.capture_screenshot(CaptureScreenshotFormatOption::Jpeg, Some(75), None, true)?;
    fs::write(screenshot_path, &screenshot)?;
    log::info!("Done creating screenshot");

    // Close tab
    tab.close(true)?;

    Ok(text)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use std::fs;

    use super::*;

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
        println!("{entries:#?}");
        assert_eq!(entries.len(), 2);

        let first = &entries[0];
        assert_eq!(first.category, "Official");
        assert_eq!(first.title, "This Development-cycle in Cargo: 1.81");
        assert_eq!(first.url, Url::parse("https://blog.rust-lang.org/inside-rust/2024/08/15/this-development-cycle-in-cargo-1.81.html").unwrap());

        let second = &entries[1];
        assert_eq!(second.category, "Official");
        assert_eq!(second.title, "Async Closures MVP: Call for Testing!");
        assert_eq!(second.url, Url::parse("https://blog.rust-lang.org/inside-rust/2024/08/09/async-closures-call-for-testing.html").unwrap());
    }

    #[test]
    fn test_code_in_content() {
        let content = indoc! {"
                ### Official
                
                * [This `code` in Cargo](https://example.com)
        "};

        let date = NaiveDate::from_ymd_opt(2024, 8, 21).unwrap();
        let entries = content_to_entry_ids(content, date);

        println!("{entries:#?}");
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

    // Old TWiR format
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

    #[test]
    fn test_content_to_entry_ids() {
        let content = fs::read_to_string("fixtures/test.md");
        assert!(content.is_ok());

        let entries = parse_file(content.unwrap().as_str());
        println!("{entries:#?}");
        // assert_eq!(entries.len(), 2);
    }
}
