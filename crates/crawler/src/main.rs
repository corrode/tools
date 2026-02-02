#![deny(missing_docs)]
#![deny(rustdoc::missing_crate_level_docs)]

//! Module for crawling and indexing content.
//! Handles fetching articles, parsing content, and storing entries in the database.

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use crawler::browser::Browser;
use crawler::indexer::{self, Indexer};
use crawler::paths;
use log::info;
use std::env;
use std::fs;
use storage::Repository;

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

/// Creates necessary output directories
fn create_output_directories() -> Result<()> {
    for path in [
        &*paths::MARKDOWN_PATH,
        &*paths::JSON_PATH,
        &*paths::HTML_PATH,
        &*paths::SCREENSHOT_PATH,
    ] {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

/// Main indexing function that processes and stores content
#[tokio::main]
pub async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    env_logger::init();
    let args = Args::parse();

    create_output_directories()?;

    let mut indexers: Vec<Box<dyn Indexer>> = Vec::new();

    // let browser =
    //     Browser::new(args.debug).context("Failed to initialize Browser for TWiR indexer")?;
    // let twir = indexer::twir::Twir::new(browser)
    //     .with_debug(args.debug)
    //     .with_dry_run(args.dry_run)
    //     .with_overwrite(args.overwrite)
    //     .with_start_date(args.start_date.clone());
    // indexers.push(Box::new(twir));

    // Set up YouTube Indexer
    let api_key =
        env::var("YOUTUBE_API_KEY").context("YOUTUBE_API_KEY environment variable not set")?;
    let youtube = indexer::youtube::Youtube::new(api_key).with_overwrite(args.overwrite);
    indexers.push(Box::new(youtube));

    let repo = Repository::new(types::get_search_index_path()).await?;

    for mut indexer in indexers {
        let name = indexer.name();
        info!("Starting indexer: {name}");
        if let Err(e) = indexer.index(&repo).await {
            log::error!("Indexer {name} failed: {e}");
        } else {
            info!("Indexer {name} completed successfully.");
        }
    }

    Ok(())
}
