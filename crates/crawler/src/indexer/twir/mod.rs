use super::Indexer;
use crate::{
    paths,
    tools::{
        browser::Browser,
        youtube::{ThumbnailConfig, YoutubeApi, fetch_transcript, video_id_from_watch_url},
    },
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::NaiveDate;
use std::{env, fs};
use storage::Repository;
use tracing::info;
use types::{Metadata, NewArticle, Url, VideoData};

mod parser;
use parser::TwirParser;

/// Stats collected during indexing
#[derive(Debug, Default)]
struct TwirStats {
    articles_processed: usize,
    articles_skipped: usize,
    articles_failed: usize,
    videos_processed: usize,
    videos_skipped: usize,
    quotes_processed: usize,
    files_processed: usize,
}

/// Indexer for "This Week in Rust" newsletter
pub struct Twir {
    parser: TwirParser,
    browser: Browser,
    youtube_api: Option<YoutubeApi>,
    thumbnail_config: ThumbnailConfig,
    debug: bool,
    crawl_count: usize,
    dry_run: bool,
    start_date: Option<String>,
    overwrite: bool,
}

impl Twir {
    /// Creates a new Twir indexer
    pub fn new(browser: Browser) -> Self {
        let youtube_api = env::var("YOUTUBE_API_KEY").ok().map(YoutubeApi::new);

        Self {
            parser: TwirParser::new(),
            browser,
            youtube_api,
            thumbnail_config: ThumbnailConfig::new(false),
            debug: false,
            crawl_count: 0,
            dry_run: false,
            start_date: None,
            overwrite: false,
        }
    }

    /// Extracts date from filename if it matches YYYY-MM-DD pattern
    fn extract_date_from_filename(filename: &str) -> Option<NaiveDate> {
        if filename.len() >= 10 {
            NaiveDate::parse_from_str(&filename[0..10], "%Y-%m-%d").ok()
        } else {
            None
        }
    }

    /// Returns true if the URL points to a YouTube video (watch URL or youtu.be short link).
    fn is_youtube_video(url: &Url) -> bool {
        let Some(host) = url.host_str() else {
            return false;
        };
        matches!(host, "www.youtube.com" | "youtube.com" | "youtu.be") && url.path() == "/watch"
    }

    /// Determines if a URL should be processed
    fn should_process_url(url: &Url) -> bool {
        let supported_protocols = ["http", "https"];
        if !supported_protocols
            .iter()
            .any(|protocol| url.scheme() == *protocol)
        {
            tracing::info!("Skipping unsupported protocol: {url}");
            return false;
        }

        let ignored_urls = [
            // Video platforms (YouTube videos are handled separately; other YouTube URLs are skipped)
            "youtube.com",
            "youtu.be",
            "vimeo.com",
            // Social media and forums
            "github.com",
            "reddit.com",
            "meetup.com",
            "twitter.com",
            "https://t.me",
            "x.com",
            "bsky.app",
            "mastodon.social",
            "mibbit.com",
            // TWiR infrastructure
            "this-week-in-rust.org",
            "this-week-in-rust.us11.list-manage.com",
            "users.rust-lang.org",
            // Rust project infrastructure
            "rust-lang.org",
            "forge.rust-lang.org",
            "foundation.rust-lang.org",
            // Dead Mozilla infrastructure
            "mail.mozilla.org",
            "irc.mozilla.org",
            "etherpad.mozilla.org",
            "air.mozilla.org",
            "badges.mozilla.org",
            // Event platforms
            "luma.com",
            "lu.ma",
            "eventbrite.com",
            "eventbrite.fr",
            "calagator.org",
            // Job/recruiting platforms
            "smartrecruiters.com",
            "bamboohr.com",
            "careers.mozilla.org",
            // Dead/defunct domains
            "thread.gmane.org",
            "blog.gmane.org",
            "rustlog.octayn.net",
            "opensourcebridge.org",
            "rust-ci.org",
            "hiho.io",
            "llvm.lyngvig.org",
            "hydrocodedesign.com",
            "metajack.im",
            "cosmic.mearie.org",
            "cmr.github.io",
            "pcwalton.github.io",
            "adridu59.github.io",
            "adrientetar.legtux.org",
            "tombebbington.github.io",
            "michaelwoerister.github.io",
            "alan-andrade.github.io",
            "rustbyexample.github.io",
            "joshldavis.com",
            "spin.atomicobject.com",
            "catamorphism.org",
            "foocafe.org",
            // Other
            "google.com/calendar",
            "docs.google.com",
            "wikipedia.org",
            "en.wikipedia.org",
            "crates.io",
        ];

        if ignored_urls.iter().any(|u| url.to_string().contains(u)) {
            tracing::debug!("Skipping ignored URL: {url}");
            return false;
        }

        true
    }

    /// Stores a YouTube video link found in TWiR as a video entry.
    ///
    /// Extracts the video ID, fetches the transcript (if available), and
    /// downloads the thumbnail using the YouTube API if a key is configured.
    /// Falls back to inserting with just the title and date from the TWiR
    /// link when no API key is present.
    async fn index_youtube_video(
        &mut self,
        id: &Metadata,
        repo: &Repository,
        stats: &mut TwirStats,
    ) {
        if repo.url_exists(&id.url).await.unwrap_or(false) {
            tracing::debug!("Skipping existing video: {}", id.url);
            stats.videos_skipped += 1;
            return;
        }

        if self.dry_run {
            info!("[DRY RUN] Would store video: {} | {}", id.title, id.url);
            return;
        }

        let video_id = match video_id_from_watch_url(&id.url) {
            Some(vid) => vid,
            None => {
                tracing::warn!("Could not extract video ID from URL: {}", id.url);
                return;
            }
        };

        // Try to download thumbnail via API; skip gracefully if no key
        let thumbnail_url = if let Some(api) = &self.youtube_api {
            match api
                .download_thumbnail_for_video_id(
                    &video_id,
                    &self.thumbnail_config.static_dir,
                    self.thumbnail_config.overwrite,
                )
                .await
            {
                Ok(path) => path,
                Err(e) => {
                    tracing::warn!("Failed to download thumbnail for {video_id}: {e}");
                    None
                }
            }
        } else {
            None
        };

        // Fetch duration via API if available
        let duration_seconds = if let Some(api) = &self.youtube_api {
            api.fetch_video_duration(&video_id).await
        } else {
            None
        };

        // Build text content: start with the title, append transcript if available
        let mut text = id.title.clone();
        if let Ok(transcript) = fetch_transcript(&video_id).await {
            info!("Fetched transcript for TWiR video: {}", id.title);
            text.push_str("\n\n");
            text.push_str(&transcript);
        }

        let video = VideoData {
            metadata: id.clone(),
            text,
            thumbnail_url,
            duration_seconds,
        };

        match repo.insert_video(&video).await {
            Ok(_) => {
                info!("Stored TWiR video: {}", id.title);
                stats.videos_processed += 1;
            }
            Err(e) => {
                tracing::warn!("Failed to insert TWiR video {}: {e}", id.url);
            }
        }
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
        let mut stats = TwirStats::default();

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

        // Parse start date for proper date comparison
        let start_date = start_date_str
            .as_ref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

        for item in entries {
            let Some(file_name) = item["name"].as_str() else {
                continue;
            };
            let Some(download_url) = item["download_url"].as_str() else {
                continue;
            };

            // Extract date from filename - skip files without valid date patterns
            let Some(file_date) = Self::extract_date_from_filename(file_name) else {
                tracing::debug!("Skipping file without date pattern: {file_name}");
                continue;
            };

            // Check resume logic - skip files before the checkpoint date
            if let Some(ref checkpoint) = start_date
                && file_date < *checkpoint
            {
                tracing::debug!("Skipping file before checkpoint: {file_name}");
                continue;
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
                tracing::warn!("No valid entries found in file: {file_name}");
                continue;
            };

            let issue_number = parse_result.issue_number;

            stats.files_processed += 1;

            // Process Quotes
            for quote in parse_result.quotes {
                if self.dry_run {
                    info!(
                        "[DRY RUN] Would insert quote: \"{}\" by {}",
                        quote.text.lines().next().unwrap_or(""),
                        quote.author
                    );
                } else if let Err(e) = repo.insert_quote(&quote).await {
                    tracing::warn!("Failed to insert quote: {e}");
                } else {
                    stats.quotes_processed += 1;
                }
            }

            // Process Links
            for mut id in parse_result.entries {
                // Rewrite URLs that have moved to new domains
                let rewritten_url = Browser::rewrite_url(&id.url);
                if rewritten_url != *id.url {
                    tracing::info!("Rewrote URL: {} -> {}", id.url, rewritten_url);
                    id.url = rewritten_url.into();
                }

                // YouTube video links are stored as videos, not crawled as articles
                if Self::is_youtube_video(&id.url) {
                    self.index_youtube_video(&id, repo, &mut stats).await;
                    continue;
                }

                if !Self::should_process_url(&id.url) {
                    continue;
                }

                if repo.url_exists(&id.url).await? {
                    tracing::debug!("Skipping already crawled URL: {}", id.url);
                    stats.articles_skipped += 1;
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
                        Err(e) => tracing::error!("Failed to recreate browser: {e}"),
                    }
                }

                tracing::info!("Crawling URL: {}", id.url);

                // Crawl with retry logic for closed connections
                let crawl_result = self.browser.crawl(&id);
                let crawl_result = if let Err(ref e) = crawl_result {
                    let err_msg = e.to_string();
                    if err_msg.contains("connection is closed") || err_msg.contains("Connection") {
                        tracing::warn!(
                            "Browser connection closed, recreating browser and retrying..."
                        );
                        match Browser::new(self.debug) {
                            Ok(b) => {
                                self.browser = b;
                                self.browser.crawl(&id)
                            }
                            Err(recreate_err) => {
                                tracing::error!(
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
                        let word_count = text.split_whitespace().count() as i64;

                        let article = NewArticle {
                            metadata: id.clone(),
                            text,
                            reference,
                            word_count,
                        };
                        if let Err(e) = repo.insert_article(&article).await {
                            tracing::error!("Failed to store entry {}: {e}", id.url);
                            stats.articles_failed += 1;
                        } else {
                            info!("Successfully stored entry: {}", id.url);
                            stats.articles_processed += 1;
                        }
                    }
                    Ok(None) => {
                        tracing::warn!("No text content extracted for {}", id.url);
                        stats.articles_failed += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to crawl {}: {e}", id.url);
                        stats.articles_failed += 1;
                    }
                }
            }
        }

        info!("TWiR indexing complete:");
        info!("  Files processed: {}", stats.files_processed);
        info!("  Articles processed: {}", stats.articles_processed);
        info!("  Articles skipped (existing): {}", stats.articles_skipped);
        info!("  Articles failed: {}", stats.articles_failed);
        info!("  Videos processed: {}", stats.videos_processed);
        info!("  Videos skipped (existing): {}", stats.videos_skipped);
        info!("  Quotes processed: {}", stats.quotes_processed);

        Ok(())
    }
}
