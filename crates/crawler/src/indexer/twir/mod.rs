use super::Indexer;
use crate::{browser::Browser, paths};
use anyhow::Result;
use async_trait::async_trait;
use log::info;
use std::fs;
use storage::Repository;
use types::Entry;

mod parser;
use parser::TwirParser;

/// Indexer for "This Week in Rust" newsletter
pub struct Twir {
    parser: TwirParser,
    browser: Browser,
    debug: bool,
    crawl_count: usize,
    dry_run: bool,
    start_date: Option<String>,
    overwrite: bool,
}

impl Twir {
    /// Creates a new Twir indexer
    pub fn new(browser: Browser) -> Self {
        Self {
            parser: TwirParser::new(),
            browser,
            debug: false,
            crawl_count: 0,
            dry_run: false,
            start_date: None,
            overwrite: false,
        }
    }

    /// Set debug mode
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Set dry run mode
    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    /// Set start date
    pub fn with_start_date(mut self, date: Option<String>) -> Self {
        self.start_date = date;
        self
    }

    /// Set overwrite mode
    pub fn with_overwrite(mut self, overwrite: bool) -> Self {
        self.overwrite = overwrite;
        self
    }

    /// Determines if a URL should be processed
    fn should_process_url(&self, url: &url::Url) -> bool {
        let supported_protocols = ["http", "https"];
        if !supported_protocols
            .iter()
            .any(|protocol| url.scheme() == *protocol)
        {
            log::info!("Skipping unsupported protocol: {url}");
            return false;
        }

        let ignored_urls = [
            // Social media and forums
            "github.com",
            "reddit.com",
            "meetup.com",
            "twitter.com",
            "https://t.me",
            "x.com",
            "vimeo.com",
            "bsky.app",
            "mastodon.social",
            "irc.mozilla.org",
            "mibbit.com",
            // TWiR infrastructure
            "this-week-in-rust.org",
            "this-week-in-rust.us11.list-manage.com",
            "users.rust-lang.org",
            // Rust project infrastructure
            "rust-lang.org",
            "forge.rust-lang.org",
            "foundation.rust-lang.org",
            // Event platforms
            "luma.com",
            "lu.ma",
            "eventbrite.com",
            "calagator.org",
            // Job/recruiting platforms
            "smartrecruiters.com",
            "bamboohr.com",
            // Other
            "google.com/calendar",
        ];

        if ignored_urls.iter().any(|u| url.to_string().contains(u)) {
            log::debug!("Skipping ignored URL: {url}");
            return false;
        }

        true
    }
}

#[async_trait]
impl Indexer for Twir {
    fn name(&self) -> &'static str {
        "twir"
    }

    fn set_debug(&mut self, value: bool) {
        self.debug = value;
    }

    fn set_dry_run(&mut self, value: bool) {
        self.dry_run = value;
    }

    fn set_overwrite(&mut self, value: bool) {
        self.overwrite = value;
    }

    fn set_start_date(&mut self, date: Option<String>) {
        self.start_date = date;
    }

    async fn index(&mut self, repo: &Repository) -> Result<()> {
        info!("Fetching TWiR entries...");
        let entries = self.parser.fetch_twir_entries().await?;

        // Determine start date logic
        let start_date_str = if self.overwrite {
            info!("Overwrite mode: starting from beginning");
            None
        } else if let Some(ref date) = self.start_date {
            info!("Using specified start date: {date}");
            Some(date.clone())
        } else {
            // Use latest date from database as default
            if let Some(latest_date) = repo.get_latest_entry_date().await? {
                let date_str = latest_date.format("%Y-%m-%d").to_string();
                info!("Resuming from latest database entry date: {date_str}");
                Some(date_str)
            } else {
                info!("No entries in database, starting from beginning");
                None
            }
        };

        let mut resume_crawling = start_date_str.is_none();

        for item in entries {
            let Some(file_name) = item["name"].as_str() else {
                continue;
            };
            let Some(download_url) = item["download_url"].as_str() else {
                continue;
            };

            // Check resume logic
            if !resume_crawling && let Some(ref start_date) = start_date_str {
                // Check if this file matches or is after the start date
                // File names are like "2013-06-29-this-week-in-rust.md"
                if file_name.len() >= 10 && &file_name[0..10] >= start_date.as_str() {
                    info!("Reached checkpoint file: {file_name}, resuming crawling");
                    resume_crawling = true;
                } else {
                    info!("Skipping file before checkpoint: {file_name}");
                    continue;
                }
            }

            info!("Processing file: {file_name}");
            let markdown_file_path = format!("{}/{file_name}", &*paths::MARKDOWN_PATH);

            // Download or read file
            let content = if fs::metadata(&markdown_file_path).is_ok() {
                info!("Reading existing file: {file_name}");
                fs::read_to_string(&markdown_file_path)?
            } else {
                info!("Downloading new file: {file_name}");
                let content = self.parser.download_content(download_url).await?;
                if self.debug {
                    fs::write(&markdown_file_path, &content)?;
                }
                content
            };

            // Parse file
            let Some(parse_result) = self.parser.parse_file(&content) else {
                log::warn!("No valid entries found in file: {file_name}");
                continue;
            };

            let issue_number = parse_result.issue_number;

            // Process Quotes
            for quote in parse_result.quotes {
                if self.dry_run {
                    info!(
                        "[DRY RUN] Would insert quote: \"{}\" by {}",
                        quote.text.lines().next().unwrap_or(""),
                        quote.author
                    );
                } else if let Err(e) = repo.insert_quote(&quote).await {
                    log::warn!("Failed to insert quote: {e}");
                }
            }

            // Process Links
            for id in parse_result.entries {
                if !self.should_process_url(&id.url) {
                    continue;
                }

                if repo.url_exists(&id.url).await? {
                    log::debug!("Skipping already crawled URL: {}", id.url);
                    continue;
                }

                if self.dry_run {
                    info!(
                        "[DRY RUN] Would crawl: {} | {} | {} | {}",
                        id.date, id.category, id.title, id.url
                    );
                    continue;
                }

                // Recreate browser every 50 crawls to prevent memory leaks
                self.crawl_count += 1;
                if self.crawl_count.is_multiple_of(50) {
                    info!("Recreating browser to prevent memory issues");
                    // We need a way to recreate the browser cleanly.
                    // Since browser creation can fail, we handle it here.
                    match Browser::new(self.debug) {
                        Ok(b) => self.browser = b,
                        Err(e) => log::error!("Failed to recreate browser: {e}"),
                    }
                }

                log::info!("Crawling URL: {}", id.url);

                // Crawl with retry logic for closed connections
                let crawl_result = self.browser.crawl(&id).await;
                let crawl_result = if let Err(ref e) = crawl_result {
                    let err_msg = e.to_string();
                    if err_msg.contains("connection is closed") || err_msg.contains("Connection") {
                        log::warn!("Browser connection closed, recreating browser and retrying...");
                        match Browser::new(self.debug) {
                            Ok(b) => {
                                self.browser = b;
                                self.browser.crawl(&id).await
                            }
                            Err(recreate_err) => {
                                log::error!(
                                    "Failed to recreate browser during retry: {recreate_err}"
                                );
                                Err(anyhow::anyhow!("Browser recreation failed"))
                            }
                        }
                    } else {
                        crawl_result
                    }
                } else {
                    crawl_result
                };

                match crawl_result {
                    Ok(Some(text)) => {
                        let reference = issue_number.map(|num| format!("TWiR #{}", num));

                        let entry = Entry {
                            id: id.clone(),
                            text: Some(text),
                            thumbnail_url: None,
                            reference,
                        };
                        if let Err(e) = repo.insert_entry(&entry).await {
                            log::error!("Failed to store entry {}: {e}", id.url);
                        } else {
                            info!("Successfully stored entry: {}", id.url);
                        }
                    }
                    Ok(None) => {
                        log::warn!("No text content extracted for {}", id.url);
                    }
                    Err(e) => {
                        log::warn!("Failed to crawl {}: {e}", id.url);
                    }
                }
            }
        }

        Ok(())
    }
}
