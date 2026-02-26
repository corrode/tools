#![deny(missing_docs)]
#![deny(rustdoc::missing_crate_level_docs)]

//! Module for crawling and indexing content.
//! Handles fetching articles, parsing content, and storing entries in the database.

use anyhow::Context;
use anyhow::Result;
use clap::{Parser, ValueEnum};
use crawler::indexer::{self, Indexer};
use crawler::paths;
use crawler::tools::browser::Browser;
use std::env;
use std::fs;
use storage::Repository;
use tracing::info;
use tracing_subscriber::EnvFilter;

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

    /// Specific indexer to run
    #[arg(long, value_enum)]
    indexer: CrawlerName,
}

#[derive(Debug, Clone, ValueEnum, PartialEq, Eq)]
enum CrawlerName {
    Conference,
    Podcast,
    Rfc,
    Research,
    Twir,
    Youtube,
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

fn create_twir_indexer(debug: bool) -> Result<Box<dyn Indexer>> {
    let browser = Browser::new(debug).context("Failed to initialize Browser for TWiR indexer")?;
    let twir = indexer::twir::Twir::new(browser);
    Ok(Box::new(twir))
}

fn create_rfc_indexer() -> Box<dyn Indexer> {
    Box::new(indexer::rfc::Rfc::new())
}

fn create_podcast_indexer() -> Box<dyn Indexer> {
    Box::new(indexer::podcast::PodcastIndexer::new())
}

fn create_research_indexer() -> Box<dyn Indexer> {
    Box::new(indexer::research::ResearchIndexer::new())
}

fn create_youtube_indexer() -> Result<Box<dyn Indexer>> {
    let api_key =
        env::var("YOUTUBE_API_KEY").context("YOUTUBE_API_KEY environment variable not set")?;
    let youtube = indexer::video::Youtube::new(api_key);
    Ok(Box::new(youtube))
}

fn create_conference_indexer() -> Result<Box<dyn Indexer>> {
    let indexer = indexer::conference::ConferenceIndexer::new()?;
    Ok(Box::new(indexer))
}

/// Main indexing function that processes and stores content
#[tokio::main]
pub async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let args = Args::parse();

    create_output_directories()?;

    let mut indexer: Box<dyn Indexer> = match args.indexer {
        CrawlerName::Conference => create_conference_indexer()?,
        CrawlerName::Podcast => create_podcast_indexer(),
        CrawlerName::Rfc => create_rfc_indexer(),
        CrawlerName::Research => create_research_indexer(),
        CrawlerName::Twir => create_twir_indexer(args.debug)?,
        CrawlerName::Youtube => create_youtube_indexer()?,
    };

    indexer.set_debug(args.debug);
    indexer.set_dry_run(args.dry_run);
    indexer.set_overwrite(args.overwrite);
    indexer.set_start_date(args.start_date);

    let repo = Repository::new(types::get_search_index_path()).await?;

    let name = indexer.name();
    info!("Starting indexer: {name}");
    if let Err(e) = indexer.index(&repo).await {
        tracing::error!("Indexer {name} failed: {e}");
    } else {
        info!("Indexer {name} completed successfully.");
    }

    Ok(())
}
