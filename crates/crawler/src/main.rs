#![deny(missing_docs)]
#![deny(rustdoc::missing_crate_level_docs)]

//! Module for crawling and indexing This Week in Rust content.
//! Handles fetching articles, parsing content, and storing entries in the database.

mod browser;
mod cookies;
mod parser;
mod sanitizer;
mod youtube;

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
#[command(about = "Crawls and indexes This Week in Rust content", long_about = None)]
struct Args {
    /// Save raw HTML to disk for future analysis
    #[arg(long, default_value_t = false)]
    save_raw_html: bool,
}

// Output paths configuration
const TWIR_OUT_PATH: &str = "./content/twir";
const INDEX_OUT_PATH: &str = "./content/index";
const RAW_OUT_PATH: &str = "./content/raw";
const SCREENSHOT_OUT_PATH: &str = "./content/screenshots";
const DB_PATH: &str = "content/db/twir.db";
const DB_DIR_PATH: &str = "./content/db";

/// Main indexing function that processes and stores TWiR content
#[tokio::main]
pub async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    create_output_directories()?;

    let browser = Browser::new(args.save_raw_html)?;
    let parser = TwirParser::new();
    let repo = Repository::new(DB_PATH).await?;

    let entries = parser.fetch_twir_entries().await?;

    for item in entries {
        let file_name = item["name"].as_str().unwrap();
        let download_url = item["download_url"].as_str().unwrap();
        let download_file_path = format!("{}/{}", TWIR_OUT_PATH, file_name);

        // Skip if we've already downloaded this file
        if fs::metadata(&download_file_path).is_ok() {
            info!("Skipping downloaded file: {}", file_name);
            continue;
        }

        // Download and save content
        let content = parser.download_content(download_url).await?;
        fs::write(&download_file_path, &content)?;

        // Parse and process entries
        for id in parser.parse_file(&content) {
            // Skip if URL shouldn't be processed
            if !should_process_url(&id.url) {
                continue;
            }

            // Crawl and store content
            if let Ok(text) = browser.crawl(&id).await {
                let entry = Entry { id, text };

                // Store in database
                if let Err(e) = repo.insert_entry(&entry).await {
                    info!("Failed to store entry {}: {}", entry.id.url, e);
                    continue;
                }

                let entry_path = format!("{INDEX_OUT_PATH}/{}.json", entry.id);
                if let Err(e) = fs::write(&entry_path, serde_json::to_string_pretty(&entry)?) {
                    info!("Failed to save JSON for {}: {}", entry.id.url, e);
                }
            }
        }
    }

    info!("Successfully indexed all TWiR content");
    Ok(())
}

/// Creates necessary output directories
fn create_output_directories() -> Result<()> {
    fs::create_dir_all(TWIR_OUT_PATH)?;
    fs::create_dir_all(INDEX_OUT_PATH)?;
    fs::create_dir_all(RAW_OUT_PATH)?;
    fs::create_dir_all(SCREENSHOT_OUT_PATH)?;
    fs::create_dir_all(DB_DIR_PATH)?;
    Ok(())
}

/// Determines if a URL should be processed based on various criteria
fn should_process_url(url: &url::Url) -> bool {
    let supported_protocols = ["http", "https"];
    if !supported_protocols
        .iter()
        .any(|protocol| url.scheme() == *protocol)
    {
        log::info!("Skipping unsupported protocol: {}", url);
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
        // Other
        "eventbrite.com",
        "smartrecruiters.com",
        "bamboohr.com",
        "google.com/calendar",
    ];

    if ignored_urls.iter().any(|u| url.to_string().contains(u)) {
        log::info!("Skipping ignored URL: {}", url);
        return false;
    }

    true
}
