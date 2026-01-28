#![deny(missing_docs)]
#![deny(rustdoc::missing_crate_level_docs)]

//! Module for crawling and indexing content.
//! Handles fetching articles, parsing content, and storing entries in the database.

mod browser;
mod cookies;
mod parser;
mod paths;
mod sanitizer;
// mod youtube;

pub use browser::Browser;
pub use parser::TwirParser;
pub use storage::Repository;
pub use types::*;

use anyhow::Result;
use clap::Parser;
use log::info;
use std::fs;

/// Command line arguments for the crawler
#[derive(Parser, Debug)]
#[command(name = "twir-crawler")]
#[command(about = "Crawls and indexes Rust content", long_about = None)]
struct Args {
    /// Enable debug mode: save raw HTML, screenshots, JSON, and markdown to disk
    #[arg(long, default_value_t = false)]
    debug: bool,

    /// Start from the beginning, ignoring checkpoint
    #[arg(long, default_value_t = false)]
    overwrite: bool,

    /// Start from a specific date (format: YYYY-MM-DD), overrides checkpoint
    #[arg(long)]
    start_date: Option<String>,

    /// Dry run mode: parse TWiR files and show what would be crawled without actually crawling
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

/// Statistics for dry-run mode
#[derive(Default)]
struct DryRunStats {
    files_processed: usize,
    urls_found: usize,
    urls_skipped: usize,
    urls_already_crawled: usize,
    urls_would_crawl: usize,
}

/// Main indexing function that processes and stores TWiR content
#[tokio::main]
pub async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    create_output_directories()?;

    let mut browser = Browser::new(args.debug)?;
    let parser = TwirParser::new();
    let repo = Repository::new(types::get_search_index_path()).await?;
    let mut crawl_count = 0;

    // Fetch all entries first
    let entries = parser.fetch_twir_entries().await?;

    // Determine the start date
    let start_date_str = if args.overwrite {
        info!("Overwrite mode: starting from beginning");
        None
    } else if let Some(ref date) = args.start_date {
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

    // Find the checkpoint file based on start date
    let checkpoint = if let Some(ref start_date) = start_date_str {
        let matching_file = entries.iter().find(|item| {
            if let Some(file_name) = item["name"].as_str() {
                // File names are like "2013-06-29-this-week-in-rust.md"
                // Extract date prefix (first 10 chars: "YYYY-MM-DD")
                file_name.len() >= 10 && &file_name[0..10] >= start_date.as_str()
            } else {
                false
            }
        });

        if let Some(file) = matching_file {
            if let Some(file_name) = file["name"].as_str() {
                info!("Starting from file: {file_name}");
                Some(file_name.to_string())
            } else {
                info!("No file name found for start date, starting from beginning");
                None
            }
        } else {
            info!("No file found for start date, starting from beginning");
            None
        }
    } else {
        None
    };

    let mut resume_crawling = checkpoint.is_none(); // If no checkpoint, start immediately
    let mut dry_run_stats = DryRunStats::default();

    for item in entries {
        // Skip items that don't have the expected fields (e.g., directories)
        let Some(file_name) = item["name"].as_str() else {
            log::debug!("Skipping item without name field");
            continue;
        };
        let Some(download_url) = item["download_url"].as_str() else {
            log::debug!("Skipping item '{file_name}' without download_url field");
            continue;
        };
        let markdown_file_path = format!("{}/{file_name}", &*paths::MARKDOWN_PATH);

        // Check if we should start processing from this file
        if !resume_crawling {
            if Some(file_name.to_string()) == checkpoint {
                // We've reached the checkpoint file, start processing from here
                info!("Reached checkpoint file: {file_name}, resuming crawling");
                resume_crawling = true;
            } else {
                // Skip files before the checkpoint
                info!("Skipping file before checkpoint: {file_name}");
                continue;
            }
        }

        info!("Processing file: {file_name}");
        dry_run_stats.files_processed += 1;

        // Download file if we don't have it yet
        let content = if fs::metadata(&markdown_file_path).is_ok() {
            info!("Reading existing file: {file_name}");
            fs::read_to_string(&markdown_file_path)?
        } else {
            info!("Downloading new file: {file_name}");
            let content = parser.download_content(download_url).await?;
            if args.debug {
                fs::write(&markdown_file_path, &content)?;
            }
            content
        };

        // Parse and process entries
        let Some(ids) = parser.parse_file(&content) else {
            log::warn!("No valid entries found in file: {file_name}");
            continue;
        };
        for id in ids {
            dry_run_stats.urls_found += 1;

            // Skip if URL shouldn't be processed
            if !should_process_url(&id.url) {
                log::debug!("Skipping URL based on criteria: {}", id.url);
                dry_run_stats.urls_skipped += 1;
                continue;
            }

            // Skip if URL already exists in database
            if repo.url_exists(&id.url).await? {
                log::info!("Skipping already crawled URL: {}", id.url);
                dry_run_stats.urls_already_crawled += 1;
                continue;
            }

            if args.dry_run {
                // Dry run mode: just log what would be crawled
                info!(
                    "[DRY RUN] Would crawl: {} | {} | {} | {}",
                    id.date, id.category, id.title, id.url
                );
                dry_run_stats.urls_would_crawl += 1;
            } else {
                // Recreate browser every 50 crawls to prevent memory leaks
                crawl_count += 1;
                if crawl_count % 50 == 0 {
                    info!("Recreating browser after {crawl_count} crawls to prevent memory issues");
                    drop(browser);
                    browser = Browser::new(args.debug)?;
                }

                // Crawl and store content
                log::info!("Crawling URL: {}", id.url);
                let crawl_result = browser.crawl(&id).await;

                // If browser connection is closed, recreate it and retry once
                let crawl_result = if let Err(ref e) = crawl_result {
                    let err_msg = e.to_string();
                    if err_msg.contains("connection is closed") || err_msg.contains("Connection") {
                        log::warn!("Browser connection closed, recreating browser and retrying...");
                        drop(browser);
                        browser = Browser::new(args.debug)?;
                        browser.crawl(&id).await
                    } else {
                        crawl_result
                    }
                } else {
                    crawl_result
                };

                match crawl_result {
                    Ok(text) => {
                        let entry = Entry { id, text };

                        // Store in database
                        if let Err(e) = repo.insert_entry(&entry).await {
                            info!("Failed to store entry {}: {e}", entry.id.url);
                            continue;
                        }

                        info!("Successfully stored entry: {}", entry.id.url);

                        if args.debug {
                            // Write JSON file for troubleshooting
                            let json_path = format!("{}/{}.json", &*paths::JSON_PATH, entry.id);
                            if let Err(e) =
                                fs::write(&json_path, serde_json::to_string_pretty(&entry)?)
                            {
                                info!("Failed to save JSON for {}: {e}", entry.id.url);
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("Failed to crawl {}: {e}", id.url);
                    }
                }
            }
        }
    }

    if args.dry_run {
        info!("=== DRY RUN SUMMARY ===");
        info!("Files processed: {}", dry_run_stats.files_processed);
        info!("URLs found: {}", dry_run_stats.urls_found);
        info!("URLs skipped (filtered): {}", dry_run_stats.urls_skipped);
        info!(
            "URLs already crawled: {}",
            dry_run_stats.urls_already_crawled
        );
        info!(
            "URLs that would be crawled: {}",
            dry_run_stats.urls_would_crawl
        );
        info!("======================");
    } else {
        info!("Successfully indexed all TWiR content");
    }
    Ok(())
}

/// Creates necessary output directories
fn create_output_directories() -> Result<()> {
    fs::create_dir_all(&*paths::MARKDOWN_PATH)?;
    fs::create_dir_all(&*paths::JSON_PATH)?;
    fs::create_dir_all(&*paths::HTML_PATH)?;
    fs::create_dir_all(&*paths::SCREENSHOT_PATH)?;
    Ok(())
}

/// Determines if a URL should be processed based on various criteria
fn should_process_url(url: &url::Url) -> bool {
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
        "https://t.me", // Explicitly using full URL to avoid matching other domains
        "x.com",
        "vimeo.com",
        "bsky.app",
        "mastodon.social",
        "irc.mozilla.org",
        "mibbit.com",
        // TWiR infrastructure (appears in every issue template)
        "this-week-in-rust.org",
        "this-week-in-rust.us11.list-manage.com", // Newsletter signup
        "users.rust-lang.org",                    // Rust user forum
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
