//! Module for crawling and indexing This Week in Rust content.
//! Handles fetching articles, parsing content, and storing entries in the database.

mod browser;
mod parser;
mod repository;
mod types;

pub use browser::Browser;
pub use parser::TwirParser;
pub use repository::Repository;
pub use types::*;

use anyhow::Result;
use std::fs;

// Output paths configuration
const TWIR_OUT_PATH: &str = "./content/twir";
const INDEX_OUT_PATH: &str = "./content/index";
const RAW_OUT_PATH: &str = "./content/raw";
const SCREENSHOT_OUT_PATH: &str = "./content/screenshots";
const SQLITE_DATABASE_URL: &str = "twir.db";

/// Main entry point for indexing all TWiR content
pub async fn index_all() -> Result<()> {
    create_output_directories()?;

    let browser = Browser::new()?;
    let parser = TwirParser::new();
    let repo = Repository::new("twir.db").await?;

    let entries = parser.fetch_twir_entries().await?;

    for item in entries {
        let file_name = item["name"].as_str().unwrap();
        let download_url = item["download_url"].as_str().unwrap();
        let download_file_path = format!("{}/{}", TWIR_OUT_PATH, file_name);

        if fs::metadata(&download_file_path).is_ok() {
            log::info!("Skipping: {}", file_name);
            continue;
        }

        let content = parser.download_content(&download_url).await?;
        fs::write(&download_file_path, &content)?;

        let entry_ids = parser.parse_file(&content);

        for id in entry_ids {
            if !should_process_url(&id.url) {
                continue;
            }

            let entry_path = format!("{INDEX_OUT_PATH}/{}.json", id);
            if fs::metadata(&entry_path).is_ok() {
                log::info!("Entry exists; skipping: {}", id);
                continue;
            }

            if let Ok(text) = browser.crawl(&id).await {
                let entry = Entry { id, text };
                repo.save_entry(&entry_path, &entry)?;
                repo.insert_entry(&entry).await?;
            }
        }
    }

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
