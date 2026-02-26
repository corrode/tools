//! Conference talk indexer
//!
//! This module provides indexing for Rust conference talks from various conferences.
//! Each conference edition has its own parser implementation behind the `ScheduleParser` trait.

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use reqwest::header;
use std::{collections::HashMap, env};
use storage::Repository;
use tracing::info;
use types::{NewSpeaker, NewTalk, Url};
use urlnorm::UrlNormalizer;

use super::Indexer;
use crate::tools::slides::{SlidesConfig, find_slides};
use crate::tools::youtube::{
    ParsedPlaylistItem, YoutubeApi, fetch_transcript, playlist_id_from_url,
    video_id_from_watch_url, video_watch_url,
};

pub mod eurorust;
pub mod rustconf;
pub mod rustweek;
mod title_matcher;

use title_matcher::{TitleMatcher, TitleMatcherConfig};

/// A parsed talk with its speakers
#[derive(Debug, Clone)]
pub struct ParsedTalk {
    /// The talk data ready for insertion
    pub talk: NewTalk,
    /// The speakers for this talk
    pub speakers: Vec<NewSpeaker>,
}

/// Metadata describing a conference edition.
#[derive(Debug, Clone)]
pub struct ConferenceMetadata {
    /// Unique identifier for this parser (e.g., "rustconf-2024")
    pub id: &'static str,
    /// Conference name (e.g., "RustConf")
    pub conference: &'static str,
    /// Year or edition identifier (e.g., "2024")
    pub year: &'static str,
    /// Base URL for this edition
    pub url: Url,
    /// Optional YouTube playlist URL for this edition (used to enrich talks)
    pub youtube_playlist_url: Option<Url>,
}

fn static_url(url: &str) -> Url {
    Url::parse(url).expect("valid URL")
}

/// Returns a normalized base URL for safe `Url::join` usage.
/// Uses `urlnorm` to normalize the host and removes empty path segments.
pub fn base_url(url: &Url) -> Result<Url> {
    let host = UrlNormalizer::default()
        .normalize_host(url)
        .or_else(|| url.host_str())
        .context("URL has host")?;

    let mut normalized = url::Url::parse(&format!("{}://{}", url.scheme(), host))
        .context("Failed to build base URL")?;

    if let Some(port) = url.port() {
        normalized
            .set_port(Some(port))
            .map_err(|()| anyhow!("Invalid port"))?;
    }

    if let Some(segments) = url.path_segments() {
        let path = segments
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>()
            .join("/");
        if !path.is_empty() {
            normalized.set_path(&format!("{}/", path));
        } else {
            normalized.set_path("/");
        }
    }

    Ok(normalized.into())
}

/// Trait for parsing conference schedules
///
/// Each conference edition should implement this trait.
/// This allows for messy, edition-specific parsing logic while
/// keeping a clean interface.
#[async_trait]
pub trait ScheduleParser: Send + Sync {
    /// Returns the metadata for this conference edition.
    fn metadata(&self) -> ConferenceMetadata;

    /// Parse the schedule and return all talks
    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>>;
}

/// Registry of all available schedule parsers
pub fn get_all_parsers() -> Vec<Box<dyn ScheduleParser>> {
    vec![
        // RustConf editions
        Box::new(rustconf::RustConf2024),
        Box::new(rustconf::RustConf2023),
        Box::new(rustconf::RustConf2022),
        Box::new(rustconf::RustConf2021),
        Box::new(rustconf::RustConf2020),
        Box::new(rustconf::RustConf2019),
        Box::new(rustconf::RustConf2018),
        Box::new(rustconf::RustConf2017),
        Box::new(rustconf::RustConf2016),
        // EuroRust editions
        Box::new(eurorust::EuroRust2025),
        Box::new(eurorust::EuroRust2024),
        Box::new(eurorust::EuroRust2023),
        Box::new(eurorust::EuroRust2022),
        // RustWeek / RustNL editions
        Box::new(rustweek::RustWeek2025),
        Box::new(rustweek::RustNL2024),
        Box::new(rustweek::RustNL2023),
    ]
}

/// Result of matching a talk to a YouTube playlist video.
struct YoutubeMatch<'a> {
    video_id: String,
    playlist_item: Option<&'a ParsedPlaylistItem>,
}

/// Parsed playlist index for quick lookups.
#[derive(Debug)]
struct YoutubePlaylistIndex {
    conference: String,
    year: String,
    by_video_id: HashMap<String, ParsedPlaylistItem>,
    items: Vec<ParsedPlaylistItem>,
    matcher: TitleMatcher,
}

impl YoutubePlaylistIndex {
    fn new(items: Vec<ParsedPlaylistItem>, metadata: &ConferenceMetadata) -> Self {
        let mut by_video_id = HashMap::new();
        let conference = metadata.conference.to_lowercase();
        let year = metadata.year.to_lowercase();

        for item in &items {
            by_video_id.insert(item.video_id.clone(), item.clone());
        }

        let matcher = TitleMatcher::new(TitleMatcherConfig {
            conference: conference.clone(),
            year: year.clone(),
            threshold: 0.70,
        });

        Self {
            conference,
            year,
            by_video_id,
            items,
            matcher,
        }
    }

    fn find_by_video_id(&self, video_id: &str) -> Option<&ParsedPlaylistItem> {
        self.by_video_id.get(video_id)
    }

    fn find_by_title(&self, title: &str, speakers: &[String]) -> Option<&ParsedPlaylistItem> {
        self.matcher
            .find_match(title, speakers, &self.items)
            .map(|result| result.item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn item(video_id: &str, title: &str) -> ParsedPlaylistItem {
        ParsedPlaylistItem {
            video_id: video_id.to_string(),
            title: title.to_string(),
            description: String::new(),
            published_at: "2024-09-11T00:00:00Z".to_string(),
            thumbnail_url: None,
        }
    }

    #[rstest]
    #[case("https://example.com", "https://example.com/")]
    #[case("https://example.com/", "https://example.com/")]
    #[case("https://www.example.com/", "https://example.com/")]
    #[case("https://example.com/foo/", "https://example.com/foo/")]
    #[case("https://example.com//foo//", "https://example.com/foo/")]
    fn base_url_normalizes(#[case] input: &str, #[case] expected: &str) {
        let url = static_url(input);
        let base = base_url(&url).expect("valid base URL");
        assert_eq!(base.as_str(), expected);
    }

    #[test]
    fn rustconf_2024_title_variants_match_playlist() {
        let metadata = ConferenceMetadata {
            id: "rustconf-2024",
            conference: "RustConf",
            year: "2024",
            url: static_url("https://2024.rustconf.com"),
            youtube_playlist_url: None,
        };

        let playlist_titles = vec![
            r#"Aeva Black: "Making Open Source Secure by Design" | KEYNOTE | RustConf 2024"#,
            r#"Nick Cameron: "Eternal Sunshine of the Rustfmt'ed Mind" | RustConf 2024"#,
            r#"Jack Wrenn: "Safety Goggles for Alchemists" | RustConf 2024"#,
            r#"Rohit Dandamundi: "Widening the Ferris Net" | RustConf 2024"#,
            r#"Isabel Atkinson: “Rustify Your API: A Journey from Specification to Implementation” | RustConf 2024"#,
            r#"Sparrow Li: "The Current State and Future of Rust Compiler Performance" | RustConf 2024"#,
            r#"Nathan Stocks: "Shooting Stars! Livecode a Game in Less Than 30 Mins" | RustConf 2024"#,
            r#"Pedro Rittner & Sean Lawlor: "Actors and Factories in Rust" | RustConf 2024"#,
            r#"David Koloski: "The (Many) Mistakes I Made in rkyv" | RustConf 2024"#,
            r#"Kyler Chin: "How We Built a Rust-y Real-Time Public Transport Map" | RustConf 2024"#,
            r#"Adam Chalmers: "Making a Programming Language for 3D Design" | RustConf 2024"#,
            r#"Martin Pool: "Finding Bugs with cargo-mutants" | RustConf 2024"#,
            r#"Jack Huey & James Munns: "An Outsider’s Guide to the Rust Project” | KEYNOTE | RustConf 2024"#,
            r#"Miguel Ojeda (Rust for Linux): KEYNOTE | RustConf 2024"#,
            r#"Jonathan Pallant: "Six Clock Cycle per Pixel - Graphics on the Neotron Pico" | RustConf 2024"#,
            r#"Joannah Nanjekye: "Rust Interop: Memory Safety Across Foreign Function Boundaries" | RustConf 2024"#,
            r#"Jacob Pratt: "Compiler-Driven Development: Making Rust Work for You" | RustConf 2024"#,
            r#"Angus Morrison: "How Rust is Powering Next-Generation Space Mission Simulators" | RustConf 2024"#,
            r#"Michael Gattozzi: "What Happens When You Run Cargo Build?" | RustConf 2024"#,
            r#"Pallavi Thukral: "Rust in Motion: Building Reliable and Performant Robotics Systems" | RustConf 2024"#,
            r#"Marc-André Giroux: "Low-Overhead Observability in High-RPS Servers" | RustConf 2024"#,
            r#"Chris Biscardi: "Web Sites, Web Apps, and Web Assembly" | RustConf 2024"#,
            r#"Joshua Liebow-Feeser: "Safety in an Unsafe World" | RustConf 2024"#,
            r#"Nicholas Matsakis (Co-Lead, Rust Design Team): "Rust Roadmap 2.0” | KEYNOTE | RustConf 2024"#,
            r#"Frédéric Ameye: "Rust in Legacy Regulated Industries" | Rust Global @ RustConf 2024"#,
            r#"Quanyi Ma: "Embracing Monorepo and LLM Evolution" | Rust Global @ RustConf 2024"#,
            r#"Martin Geisler: "Rust Training at Scale" | Rust Global @ RustConf 2024"#,
            r#"Ed Jones: “Fearless Refactoring & the Art of Argument-Free Rust” | Rust Global @ RustConf 2024"#,
            r#"Walter Pearce: “Dude, Where's My C?" | Rust Global @ RustConf 2024"#,
        ];

        let items = playlist_titles
            .iter()
            .enumerate()
            .map(|(idx, title)| item(&format!("video-{idx}"), title))
            .collect::<Vec<_>>();

        let index = YoutubePlaylistIndex::new(items, &metadata);

        let pairs = vec![
            (
                "Making Open Source Secure by Design",
                r#"Aeva Black: "Making Open Source Secure by Design" | KEYNOTE | RustConf 2024"#,
            ),
            (
                "Eternal Sunshine of the Rustfmt'ed Mind",
                r#"Nick Cameron: "Eternal Sunshine of the Rustfmt'ed Mind" | RustConf 2024"#,
            ),
            (
                "Safety Goggles for Alchemists",
                r#"Jack Wrenn: "Safety Goggles for Alchemists" | RustConf 2024"#,
            ),
            (
                "Widening the Ferris Net",
                r#"Rohit Dandamundi: "Widening the Ferris Net" | RustConf 2024"#,
            ),
            (
                "Rustify Your API: A Journey from Specification to Implementation",
                r#"Isabel Atkinson: “Rustify Your API: A Journey from Specification to Implementation” | RustConf 2024"#,
            ),
            (
                "The Current State and Future of Rust Compiler Performance",
                r#"Sparrow Li: "The Current State and Future of Rust Compiler Performance" | RustConf 2024"#,
            ),
            (
                "Shooting Stars! Livecode a Game in Under 30 Minutes",
                r#"Nathan Stocks: "Shooting Stars! Livecode a Game in Less Than 30 Mins" | RustConf 2024"#,
            ),
            (
                "Actors and Factories in Rust: Distributed Processing Overload Protection",
                r#"Pedro Rittner & Sean Lawlor: "Actors and Factories in Rust" | RustConf 2024"#,
            ),
            (
                "The (Many) Mistakes I Made in rkyv",
                r#"David Koloski: "The (Many) Mistakes I Made in rkyv" | RustConf 2024"#,
            ),
            (
                "How We Built a Rust-y Real-Time Public Transport Map",
                r#"Kyler Chin: "How We Built a Rust-y Real-Time Public Transport Map" | RustConf 2024"#,
            ),
            (
                "Making a Programming Language for 3D Design",
                r#"Adam Chalmers: "Making a Programming Language for 3D Design" | RustConf 2024"#,
            ),
            (
                "Finding Bugs with cargo-mutants",
                r#"Martin Pool: "Finding Bugs with cargo-mutants" | RustConf 2024"#,
            ),
            (
                "An Outsider's Guide to the Rust Project",
                r#"Jack Huey & James Munns: "An Outsider’s Guide to the Rust Project” | KEYNOTE | RustConf 2024"#,
            ),
            (
                "Rust for Linux",
                r#"Miguel Ojeda (Rust for Linux): KEYNOTE | RustConf 2024"#,
            ),
            (
                "Six Clock Cycle per Pixel - Graphics on the Neotron Pico",
                r#"Jonathan Pallant: "Six Clock Cycle per Pixel - Graphics on the Neotron Pico" | RustConf 2024"#,
            ),
            (
                "Rust/C++ Interop: Memory Safety Across Foreign Function Boundaries",
                r#"Joannah Nanjekye: "Rust Interop: Memory Safety Across Foreign Function Boundaries" | RustConf 2024"#,
            ),
            (
                "Compiler-Driven Development: Making Rust Work for You",
                r#"Jacob Pratt: "Compiler-Driven Development: Making Rust Work for You" | RustConf 2024"#,
            ),
            (
                "Rust in Space! How Rust is Powering Next-Generation Space Mission Simulators",
                r#"Angus Morrison: "How Rust is Powering Next-Generation Space Mission Simulators" | RustConf 2024"#,
            ),
            (
                "What Happens When You Run Cargo Build?",
                r#"Michael Gattozzi: "What Happens When You Run Cargo Build?" | RustConf 2024"#,
            ),
            (
                "Rust in Motion: Building Reliable and Performant Robotics Systems",
                r#"Pallavi Thukral: "Rust in Motion: Building Reliable and Performant Robotics Systems" | RustConf 2024"#,
            ),
            (
                "Low-Overhead Observability in High-RPS Servers with the Tracing Crate",
                r#"Marc-André Giroux: "Low-Overhead Observability in High-RPS Servers" | RustConf 2024"#,
            ),
            (
                "Web Sites, Web Apps, and Web Assembly",
                r#"Chris Biscardi: "Web Sites, Web Apps, and Web Assembly" | RustConf 2024"#,
            ),
            (
                "Safety in an Unsafe World",
                r#"Joshua Liebow-Feeser: "Safety in an Unsafe World" | RustConf 2024"#,
            ),
            (
                "Project Goals: Rust Roadmap 2.0",
                r#"Nicholas Matsakis (Co-Lead, Rust Design Team): "Rust Roadmap 2.0” | KEYNOTE | RustConf 2024"#,
            ),
            (
                "Rust Global Rust in Legacy Regulated Industries? The Example of a Carmaker.",
                r#"Frédéric Ameye: "Rust in Legacy Regulated Industries" | Rust Global @ RustConf 2024"#,
            ),
            (
                "Rust Global Reimagining Rust-Powered Git - Embracing Monorepo and LLM Evolution",
                r#"Quanyi Ma: "Embracing Monorepo and LLM Evolution" | Rust Global @ RustConf 2024"#,
            ),
            (
                "Rust Global Rust Training at Scale",
                r#"Martin Geisler: "Rust Training at Scale" | Rust Global @ RustConf 2024"#,
            ),
            (
                "Rust Global Fearless Refactoring and the Art of Argument-Free Rust",
                r#"Ed Jones: “Fearless Refactoring & the Art of Argument-Free Rust” | Rust Global @ RustConf 2024"#,
            ),
            (
                "Rust Global Dude, Where's My C?",
                r#"Walter Pearce: “Dude, Where's My C?" | Rust Global @ RustConf 2024"#,
            ),
        ];

        for (db_title, expected_yt_title) in pairs {
            let matched = index
                .find_by_title(db_title, &[])
                .unwrap_or_else(|| panic!("Expected match for '{db_title}'"));
            assert_eq!(matched.title, expected_yt_title);
        }
    }
}

/// Main indexer for conference talks
pub struct ConferenceIndexer {
    client: reqwest::Client,
    youtube_api: YoutubeApi,
    debug: bool,
    dry_run: bool,
    overwrite: bool,
}

impl ConferenceIndexer {
    /// Creates a new conference indexer.
    ///
    /// # Errors
    ///
    /// Returns an error if the `YOUTUBE_API_KEY` environment variable is not set.
    pub fn new() -> Result<Self> {
        let api_key = env::var("YOUTUBE_API_KEY")
            .context("YOUTUBE_API_KEY environment variable is required")?;

        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static("corrode/search crawler"),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build reqwest client");

        let youtube_api = YoutubeApi::new(api_key);

        Ok(Self {
            client,
            youtube_api,
            debug: false,
            dry_run: false,
            overwrite: false,
        })
    }

    /// Get parsers
    fn get_parsers(&self) -> Vec<Box<dyn ScheduleParser>> {
        get_all_parsers()
    }

    /// Build a playlist index if metadata has a playlist URL.
    async fn load_playlist_index(
        &self,
        metadata: &ConferenceMetadata,
    ) -> Result<Option<YoutubePlaylistIndex>> {
        let Some(playlist_url) = metadata.youtube_playlist_url.as_ref() else {
            return Ok(None);
        };

        let Some(playlist_id) = playlist_id_from_url(playlist_url) else {
            tracing::warn!(
                "[{}] Could not extract playlist ID from URL: {}",
                metadata.id,
                playlist_url
            );
            return Ok(None);
        };

        tracing::info!(
            "[{}] Fetching YouTube playlist: {}",
            metadata.id,
            playlist_id
        );

        match self.youtube_api.fetch_full_playlist(&playlist_id).await {
            Ok(items) => {
                tracing::info!("[{}] Loaded {} playlist videos", metadata.id, items.len());
                Ok(Some(YoutubePlaylistIndex::new(items, metadata)))
            }
            Err(e) => {
                tracing::error!("[{}] Failed to fetch playlist via API: {}", metadata.id, e);
                Err(e)
            }
        }
    }

    /// Match a talk to a YouTube playlist video by video ID or title similarity.
    ///
    /// Updates `talk.video_url` as a side-effect when matched by title and the
    /// talk had no video URL.
    fn resolve_youtube_match<'a>(
        talk: &mut NewTalk,
        speakers: &[NewSpeaker],
        playlist: &'a YoutubePlaylistIndex,
    ) -> Option<YoutubeMatch<'a>> {
        // Extract video ID from the talk's existing video URL (if any)
        let mut video_id = talk
            .video_url
            .as_ref()
            .and_then(|url| Url::parse(url).ok())
            .and_then(|url| {
                video_id_from_watch_url(&url).or_else(|| {
                    let is_short = url
                        .host_str()
                        .map(|host| host.eq_ignore_ascii_case("youtu.be"))
                        .unwrap_or(false);
                    if !is_short {
                        return None;
                    }
                    let id = url.path().trim_start_matches('/');
                    if id.is_empty() {
                        None
                    } else {
                        Some(id.to_string())
                    }
                })
            });

        // Try matching by video ID first, then fall back to title similarity
        let mut playlist_item = video_id
            .as_deref()
            .and_then(|id| playlist.find_by_video_id(id));

        if playlist_item.is_none() {
            let speaker_names = speakers.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
            if let Some(item) = playlist.find_by_title(&talk.title, &speaker_names) {
                playlist_item = Some(item);
                if video_id.is_none() {
                    talk.video_url = Some(video_watch_url(&item.video_id));
                    video_id = Some(item.video_id.clone());
                }
            }
        }

        let video_id = video_id?;

        // Canonicalize youtu.be/ short URLs
        let needs_canonical = talk
            .video_url
            .as_deref()
            .map(|url| url.contains("youtu.be/"))
            .unwrap_or(true);
        if needs_canonical {
            talk.video_url = Some(video_watch_url(&video_id));
        }

        // Re-lookup in case the initial match was by title but we now have the ID
        let playlist_item = playlist_item.or_else(|| playlist.find_by_video_id(&video_id));

        Some(YoutubeMatch {
            video_id,
            playlist_item,
        })
    }

    /// Search for a slide deck URL using the YouTube description or talk summary
    /// as a hint.
    async fn find_slides_for_talk(
        talk: &NewTalk,
        playlist_item: Option<&ParsedPlaylistItem>,
        conference: &str,
        year: &str,
    ) -> Result<Option<String>> {
        let description = playlist_item
            .map(|item| item.description.as_str())
            .unwrap_or("");

        let hint = if description.trim().is_empty() {
            if talk.summary.starts_with("Talk by ") {
                ""
            } else {
                talk.summary.as_str()
            }
        } else {
            description
        };

        let candidate = find_slides(
            hint,
            &talk.title,
            conference,
            year,
            &SlidesConfig::default(),
        )
        .await?;

        Ok(candidate.map(|c| {
            tracing::debug!(
                "Slides found for '{}' from {:?}: {}",
                talk.title,
                c.source,
                c.url
            );
            c.url
        }))
    }

    /// Enrich a talk with YouTube metadata (video URL, transcript, duration, summary).
    async fn enrich_talk_with_youtube(
        &self,
        talk: &mut NewTalk,
        speakers: &[NewSpeaker],
        playlist: &YoutubePlaylistIndex,
    ) -> Result<()> {
        if talk
            .video_url
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(false)
        {
            talk.video_url = None;
        }
        if talk
            .slides_url
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(false)
        {
            talk.slides_url = None;
        }
        if talk
            .transcript
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(false)
        {
            talk.transcript = None;
        }

        let Some(youtube) = Self::resolve_youtube_match(talk, speakers, playlist) else {
            tracing::warn!(
                "No playlist match for talk '{}' ({} {})",
                talk.title,
                playlist.conference,
                playlist.year
            );
            if talk.slides_url.is_none() {
                talk.slides_url =
                    Self::find_slides_for_talk(talk, None, &playlist.conference, &playlist.year)
                        .await?;
            }
            return Ok(());
        };

        let YoutubeMatch {
            video_id,
            playlist_item,
        } = &youtube;

        // Thumbnail: prefer playlist metadata, fall back to YouTube default
        if talk.thumbnail_url.is_none() {
            if let Some(item) = playlist_item
                && let Some(url) = item
                    .thumbnail_url
                    .as_deref()
                    .filter(|u| !u.trim().is_empty())
            {
                talk.thumbnail_url = Some(url.to_string());
            }
            if talk.thumbnail_url.is_none() {
                talk.thumbnail_url =
                    Some(format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", video_id));
            }
        }

        if self.debug
            && let Some(item) = playlist_item
            && item.description.trim().is_empty()
        {
            info!(
                "YouTube description is empty for video {} (talk: {})",
                video_id, talk.title
            );
        }

        // Summary: use YouTube description when the parsed summary is missing or generic
        if (talk.summary.trim().is_empty() || talk.summary.starts_with("Talk by "))
            && let Some(item) = playlist_item
            && !item.description.trim().is_empty()
        {
            talk.summary = item.description.clone();
        }

        // Transcript
        if talk.transcript.as_deref().unwrap_or_default().is_empty() {
            tracing::trace!("Fetching transcript for video {}", video_id);
            if let Ok(transcript) = fetch_transcript(video_id).await {
                tracing::debug!(
                    "Transcript fetched for {} ({} chars)",
                    video_id,
                    transcript.len()
                );
                talk.transcript = Some(transcript);
            }
        }

        // Slides
        if talk.slides_url.is_none() {
            talk.slides_url = Self::find_slides_for_talk(
                talk,
                *playlist_item,
                &playlist.conference,
                &playlist.year,
            )
            .await?;
        }

        // Duration
        if talk.duration_seconds.is_none() {
            talk.duration_seconds = self.youtube_api.fetch_video_duration(video_id).await;
        }

        Ok(())
    }

    /// Process a single parsed talk
    async fn process_talk(&self, repo: &Repository, parsed: ParsedTalk) -> Result<()> {
        // Check if talk already exists
        if !self.overwrite && repo.talk_exists(&parsed.talk.website_url).await? {
            tracing::debug!("Talk already exists, skipping: {}", parsed.talk.title);
            return Ok(());
        }

        if self.dry_run {
            info!(
                "[DRY RUN] Would insert talk: {} ({} speakers)",
                parsed.talk.title,
                parsed.speakers.len()
            );
            return Ok(());
        }

        // Insert the talk
        let talk_id = repo.insert_talk(&parsed.talk).await?;

        // Insert speakers and link them to the talk
        for speaker in parsed.speakers {
            let speaker_id = repo.upsert_speaker(&speaker).await?;
            repo.link_speaker_to_talk(talk_id, speaker_id).await?;
        }

        Ok(())
    }
}

/// Stats collected during indexing
#[derive(Debug, Default)]
struct ConferenceStats {
    processed: usize,
    skipped_existing: usize,
    failed: usize,
}

#[async_trait]
impl Indexer for ConferenceIndexer {
    fn name(&self) -> &'static str {
        "conference"
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

    async fn index(&mut self, repo: &Repository) -> Result<()> {
        info!("Indexing conference talks...");

        let parsers = self.get_parsers();
        if parsers.is_empty() {
            info!("No conference parsers available");
            return Ok(());
        }

        let mut total_stats = ConferenceStats::default();

        for parser in parsers {
            let metadata = parser.metadata();
            info!(
                "[{}] Processing {} {}",
                metadata.id, metadata.conference, metadata.year
            );

            let playlist_index = self.load_playlist_index(&metadata).await?;

            // Parse the schedule
            match parser.parse(&self.client).await {
                Ok(talks) => {
                    info!("[{}] Found {} talks", metadata.id, talks.len());

                    for mut parsed in talks {
                        let talk_title = parsed.talk.title.clone();

                        if !self.overwrite {
                            match repo.talk_exists(&parsed.talk.website_url).await {
                                Ok(true) => {
                                    tracing::debug!(
                                        "[{}] Talk already exists: {}",
                                        metadata.id,
                                        talk_title
                                    );
                                    total_stats.skipped_existing += 1;
                                    continue;
                                }
                                Ok(false) => {}
                                Err(e) => {
                                    tracing::warn!(
                                        "[{}] Failed to check talk existence: {}",
                                        metadata.id,
                                        e
                                    );
                                    if self.debug {
                                        return Err(e);
                                    }
                                    total_stats.failed += 1;
                                    continue;
                                }
                            }
                        }

                        if let Some(index) = playlist_index.as_ref()
                            && let Err(e) = self
                                .enrich_talk_with_youtube(&mut parsed.talk, &parsed.speakers, index)
                                .await
                        {
                            tracing::warn!(
                                "[{}] Failed to enrich talk '{}': {}",
                                metadata.id,
                                talk_title,
                                e
                            );
                            if self.debug {
                                return Err(e);
                            }
                        }

                        match self.process_talk(repo, parsed).await {
                            Ok(()) => {
                                info!("[{}] Indexed talk: {}", metadata.id, talk_title);
                                total_stats.processed += 1;
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "[{}] Failed to index talk '{}': {}",
                                    metadata.id,
                                    talk_title,
                                    e
                                );
                                if self.debug {
                                    return Err(e);
                                }
                                total_stats.failed += 1;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("[{}] Failed to parse schedule: {}", metadata.id, e);
                    if self.debug {
                        return Err(e);
                    }
                }
            }
        }

        info!(
            "Conference indexing complete: {} processed, {} skipped, {} failed",
            total_stats.processed, total_stats.skipped_existing, total_stats.failed
        );

        Ok(())
    }
}

#[cfg(test)]
mod playlist_matching_tests {
    use super::*;
    use rstest::rstest;

    fn item(video_id: &str, title: &str) -> ParsedPlaylistItem {
        ParsedPlaylistItem {
            video_id: video_id.to_string(),
            title: title.to_string(),
            description: String::new(),
            published_at: String::new(),
            thumbnail_url: None,
        }
    }

    /// Test data structure for a conference year
    struct ConferenceTestData {
        year: &'static str,
        /// YouTube playlist video titles (as they appear on YouTube)
        youtube_titles: Vec<&'static str>,
        /// Pairs of (db_title, expected_youtube_title)
        /// If expected_youtube_title is None, we just check that *some* match is found
        pairs: Vec<(&'static str, Option<&'static str>)>,
    }

    // ==================== RustConf 2024 ====================
    fn rustconf_2024_data() -> ConferenceTestData {
        ConferenceTestData {
            year: "2024",
            youtube_titles: vec![
                r#"Aeva Black: "Making Open Source Secure by Design" | KEYNOTE | RustConf 2024"#,
                r#"Nick Cameron: "Eternal Sunshine of the Rustfmt'ed Mind" | RustConf 2024"#,
                r#"Jack Wrenn: "Safety Goggles for Alchemists" | RustConf 2024"#,
                r#"Rohit Dandamundi: "Widening the Ferris Net" | RustConf 2024"#,
                r#"Isabel Atkinson: "Rustify Your API: A Journey from Specification to Implementation" | RustConf 2024"#,
                r#"Sparrow Li: "The Current State and Future of Rust Compiler Performance" | RustConf 2024"#,
                r#"Nathan Stocks: "Shooting Stars! Livecode a Game in Less Than 30 Mins" | RustConf 2024"#,
                r#"Pedro Rittner & Sean Lawlor: "Actors and Factories in Rust" | RustConf 2024"#,
                r#"David Koloski: "The (Many) Mistakes I Made in rkyv" | RustConf 2024"#,
                r#"Kyler Chin: "How We Built a Rust-y Real-Time Public Transport Map" | RustConf 2024"#,
                r#"Adam Chalmers: "Making a Programming Language for 3D Design" | RustConf 2024"#,
                r#"Martin Pool: "Finding Bugs with cargo-mutants" | RustConf 2024"#,
                r#"Jack Huey & James Munns: "An Outsider's Guide to the Rust Project" | KEYNOTE | RustConf 2024"#,
                r#"Miguel Ojeda (Rust for Linux): KEYNOTE | RustConf 2024"#,
                r#"Jonathan Pallant: "Six Clock Cycle per Pixel - Graphics on the Neotron Pico" | RustConf 2024"#,
                r#"Joannah Nanjekye: "Rust Interop: Memory Safety Across Foreign Function Boundaries" | RustConf 2024"#,
                r#"Jacob Pratt: "Compiler-Driven Development: Making Rust Work for You" | RustConf 2024"#,
                r#"Angus Morrison: "How Rust is Powering Next-Generation Space Mission Simulators" | RustConf 2024"#,
                r#"Michael Gattozzi: "What Happens When You Run Cargo Build?" | RustConf 2024"#,
                r#"Pallavi Thukral: "Rust in Motion: Building Reliable and Performant Robotics Systems" | RustConf 2024"#,
                r#"Marc-André Giroux: "Low-Overhead Observability in High-RPS Servers" | RustConf 2024"#,
                r#"Chris Biscardi: "Web Sites, Web Apps, and Web Assembly" | RustConf 2024"#,
                r#"Joshua Liebow-Feeser: "Safety in an Unsafe World" | RustConf 2024"#,
                r#"Nicholas Matsakis (Co-Lead, Rust Design Team): "Rust Roadmap 2.0" | KEYNOTE | RustConf 2024"#,
                r#"Frédéric Ameye: "Rust in Legacy Regulated Industries" | Rust Global @ RustConf 2024"#,
                r#"Quanyi Ma: "Embracing Monorepo and LLM Evolution" | Rust Global @ RustConf 2024"#,
                r#"Martin Geisler: "Rust Training at Scale" | Rust Global @ RustConf 2024"#,
                r#"Ed Jones: "Fearless Refactoring & the Art of Argument-Free Rust" | Rust Global @ RustConf 2024"#,
                r#"Walter Pearce: "Dude, Where's My C?" | Rust Global @ RustConf 2024"#,
            ],
            pairs: vec![
                (
                    "Making Open Source Secure by Design",
                    Some(
                        r#"Aeva Black: "Making Open Source Secure by Design" | KEYNOTE | RustConf 2024"#,
                    ),
                ),
                (
                    "Eternal Sunshine of the Rustfmt'ed Mind",
                    Some(
                        r#"Nick Cameron: "Eternal Sunshine of the Rustfmt'ed Mind" | RustConf 2024"#,
                    ),
                ),
                (
                    "Safety Goggles for Alchemists",
                    Some(r#"Jack Wrenn: "Safety Goggles for Alchemists" | RustConf 2024"#),
                ),
                (
                    "Widening the Ferris Net",
                    Some(r#"Rohit Dandamundi: "Widening the Ferris Net" | RustConf 2024"#),
                ),
                (
                    "Rustify Your API: A Journey from Specification to Implementation",
                    Some(
                        r#"Isabel Atkinson: "Rustify Your API: A Journey from Specification to Implementation" | RustConf 2024"#,
                    ),
                ),
                (
                    "The Current State and Future of Rust Compiler Performance",
                    Some(
                        r#"Sparrow Li: "The Current State and Future of Rust Compiler Performance" | RustConf 2024"#,
                    ),
                ),
                (
                    "Shooting Stars! Livecode a Game in Under 30 Minutes",
                    Some(
                        r#"Nathan Stocks: "Shooting Stars! Livecode a Game in Less Than 30 Mins" | RustConf 2024"#,
                    ),
                ),
                (
                    "Actors and Factories in Rust: Distributed Processing Overload Protection",
                    Some(
                        r#"Pedro Rittner & Sean Lawlor: "Actors and Factories in Rust" | RustConf 2024"#,
                    ),
                ),
                (
                    "The (Many) Mistakes I Made in rkyv",
                    Some(r#"David Koloski: "The (Many) Mistakes I Made in rkyv" | RustConf 2024"#),
                ),
                (
                    "How We Built a Rust-y Real-Time Public Transport Map",
                    Some(
                        r#"Kyler Chin: "How We Built a Rust-y Real-Time Public Transport Map" | RustConf 2024"#,
                    ),
                ),
                (
                    "Making a Programming Language for 3D Design",
                    Some(
                        r#"Adam Chalmers: "Making a Programming Language for 3D Design" | RustConf 2024"#,
                    ),
                ),
                (
                    "Finding Bugs with cargo-mutants",
                    Some(r#"Martin Pool: "Finding Bugs with cargo-mutants" | RustConf 2024"#),
                ),
                (
                    "An Outsider's Guide to the Rust Project",
                    Some(
                        r#"Jack Huey & James Munns: "An Outsider's Guide to the Rust Project" | KEYNOTE | RustConf 2024"#,
                    ),
                ),
                (
                    "Rust for Linux",
                    Some(r#"Miguel Ojeda (Rust for Linux): KEYNOTE | RustConf 2024"#),
                ),
                (
                    "Six Clock Cycle per Pixel - Graphics on the Neotron Pico",
                    Some(
                        r#"Jonathan Pallant: "Six Clock Cycle per Pixel - Graphics on the Neotron Pico" | RustConf 2024"#,
                    ),
                ),
                (
                    "Rust/C++ Interop: Memory Safety Across Foreign Function Boundaries",
                    Some(
                        r#"Joannah Nanjekye: "Rust Interop: Memory Safety Across Foreign Function Boundaries" | RustConf 2024"#,
                    ),
                ),
                (
                    "Compiler-Driven Development: Making Rust Work for You",
                    Some(
                        r#"Jacob Pratt: "Compiler-Driven Development: Making Rust Work for You" | RustConf 2024"#,
                    ),
                ),
                (
                    "Rust in Space! How Rust is Powering Next-Generation Space Mission Simulators",
                    Some(
                        r#"Angus Morrison: "How Rust is Powering Next-Generation Space Mission Simulators" | RustConf 2024"#,
                    ),
                ),
                (
                    "What Happens When You Run Cargo Build?",
                    Some(
                        r#"Michael Gattozzi: "What Happens When You Run Cargo Build?" | RustConf 2024"#,
                    ),
                ),
                (
                    "Rust in Motion: Building Reliable and Performant Robotics Systems",
                    Some(
                        r#"Pallavi Thukral: "Rust in Motion: Building Reliable and Performant Robotics Systems" | RustConf 2024"#,
                    ),
                ),
                (
                    "Low-Overhead Observability in High-RPS Servers with the Tracing Crate",
                    Some(
                        r#"Marc-André Giroux: "Low-Overhead Observability in High-RPS Servers" | RustConf 2024"#,
                    ),
                ),
                (
                    "Web Sites, Web Apps, and Web Assembly",
                    Some(
                        r#"Chris Biscardi: "Web Sites, Web Apps, and Web Assembly" | RustConf 2024"#,
                    ),
                ),
                (
                    "Safety in an Unsafe World",
                    Some(r#"Joshua Liebow-Feeser: "Safety in an Unsafe World" | RustConf 2024"#),
                ),
                (
                    "Project Goals: Rust Roadmap 2.0",
                    Some(
                        r#"Nicholas Matsakis (Co-Lead, Rust Design Team): "Rust Roadmap 2.0" | KEYNOTE | RustConf 2024"#,
                    ),
                ),
                (
                    "Rust Global Rust in Legacy Regulated Industries? The Example of a Carmaker.",
                    Some(
                        r#"Frédéric Ameye: "Rust in Legacy Regulated Industries" | Rust Global @ RustConf 2024"#,
                    ),
                ),
                (
                    "Rust Global Reimagining Rust-Powered Git - Embracing Monorepo and LLM Evolution",
                    Some(
                        r#"Quanyi Ma: "Embracing Monorepo and LLM Evolution" | Rust Global @ RustConf 2024"#,
                    ),
                ),
                (
                    "Rust Global Rust Training at Scale",
                    Some(
                        r#"Martin Geisler: "Rust Training at Scale" | Rust Global @ RustConf 2024"#,
                    ),
                ),
                (
                    "Rust Global Fearless Refactoring and the Art of Argument-Free Rust",
                    Some(
                        r#"Ed Jones: "Fearless Refactoring & the Art of Argument-Free Rust" | Rust Global @ RustConf 2024"#,
                    ),
                ),
                (
                    "Rust Global Dude, Where's My C?",
                    Some(r#"Walter Pearce: "Dude, Where's My C?" | Rust Global @ RustConf 2024"#),
                ),
            ],
        }
    }

    // ==================== RustConf 2023 ====================
    fn rustconf_2023_data() -> ConferenceTestData {
        ConferenceTestData {
            year: "2023",
            youtube_titles: vec![
                // Actual titles from RustConf 2023 YouTube RSS feed
                r#"RustConf 2023 - Extending Rust's Effect System"#,
                r#"RustConf 2023 - Implementing a Blazingly Fast Quantum State Simulator in Rust"#,
                r#"RustConf 2023 - Fine! I'll just make my own stable ABI!"#,
                r#"RustConf 2023 - Profiling async applications in Rust"#,
                r#"RustConf 2023 - Beyond Ctrl-C the dark corners of Unix signal handling"#,
                r#"RustConf 2023 - A Rust-based garbage collector for Python"#,
                r#"RustConf 2023 - GUI Accessibility Across Platforms and Programming Languages Using Rust"#,
                r#"RustConf 2023 - Rust Foundation: Demystified"#,
                r#"RustConf 2023 - Async building blocks: A streaming Data Drama in Three Acts"#,
                r#"RustConf 2023 - Rewrite it in Objective-C?"#,
                r#"RustConf 2023 - Integrating Rust and Go: Lessons from Github Code Search"#,
                r#"RustConf 2023 - Anything you can do, I can do worse with macro_rules!"#,
                r#"RustConf 2023 - How Powerful is Const"#,
                r#"RustConf 2023 - Infrastructure for Rust"#,
                r#"RustConf 2023 - Using Rust and Battlesnake to never stop learning"#,
                r#"RustConf 2023 - The standard library is special. Let's change that."#,
                r#"RustConf 2023 - Rustacean Community Interfaces"#,
                r#"RustConf 2023 - Rust in the Skies over Antarctica"#,
                r#"RustConf 2023 - The Art and Science of Teaching Rust"#,
                r#"RustConf 2023 - Rust in the Wild: A Factory Control System From Scratch"#,
                r#"RustConf 2023 - Rusty Genomics - Rust in the Biosciences"#,
                r#"RustConf 2023 - Too many cooks or not enough kitchens?"#,
                // Note: Closing Keynote, Fearless Concurrency (AM), Ultimate Rust Crash Course (PM)
                // do not appear to have video recordings in the main playlist
            ],
            pairs: vec![
                ("Extending Rust's Effect System", None),
                (
                    "Beyond Ctrl-C: The Dark Corners of Unix Signal Handling",
                    None,
                ),
                (
                    "Making GUIs Accessible Across Platforms and Languages with Rust",
                    None,
                ),
                ("Profiling Async Applications in Rust", None),
                ("Rust Foundation: Demystified", None),
                (
                    "Infrastructure for Rust: Supporting a Growing Language and Community",
                    None,
                ),
                ("Rustacean Community Interfaces", None),
                ("The standard library is special. Let's change that.", None),
                (
                    "Fine, I'll Just Make My Own Stable ABI, with Compact sum-types and Stable `rustc`!",
                    None,
                ),
                (
                    "Implementing a Blazingly Fast Quantum State Simulator in Rust",
                    None,
                ),
                ("Rust in the Skies over Antarctica", None),
                ("A Rust-based Garbage Collector for Python", None),
                ("How Powerful is Const?", None),
                ("Integrating Rust and Go for GitHub code search", None),
                (
                    "Rewrite it in Objective-C? The dark and dangerous secrets of Rust on macOS",
                    None,
                ),
                ("The Art and Science of Teaching Rust", None),
                (
                    "Async Building Blocks: A Streaming Data Drama in Three Acts",
                    None,
                ),
                (
                    "Anything you can do, I can do worse with `macro_rules`!",
                    None,
                ),
                (
                    "Rust in the Wild: A Factory Control System From Scratch",
                    None,
                ),
                ("Rusty Genomics - Rust in the Biosciences", None),
                ("Using Rust and Battlesnake to never stop learning", None),
                // Subtitle matching: DB has formal "Main Title: subtitle" but YouTube only has subtitle
                (
                    "Organizational Boundary Problems: too many cooks or not enough kitchens?",
                    None,
                ),
            ],
        }
    }

    // ==================== RustConf 2022 ====================
    fn rustconf_2022_data() -> ConferenceTestData {
        ConferenceTestData {
            year: "2022",
            youtube_titles: vec![
                r#"RustConf 2022 - Opening Keynote"#,
                r#"RustConf 2022 - All aboard the Rust (electric freight) train! by James Munns"#,
                r#"RustConf 2022 - How we ship Rust in OpenSUSE by Federico Mena-Quintero"#,
                r#"RustConf 2022 - The Sheer Terror of PAM by Liv"#,
                r#"RustConf 2022 - Bootstrapping: The once and future compiler by John Googin"#,
                r#"RustConf 2022 - Weird Expressions and Where to Find Them by Kevin Conner"#,
                r#"RustConf 2022 - Async Rust: Past, Present, and Future by Nick Cameron"#,
                r#"RustConf 2022 - Writing a GraphQL compiler in Rust, a case study by Gerald Monaco"#,
                r#"RustConf 2022 - Your Open Source Repo Needs A Project Manager by Tobias Bieniek"#,
                r#"RustConf 2022 - What If We Pretended Unsafe Code Was Nice, And Then It Was? by Jacob Lifshay"#,
            ],
            pairs: vec![
                ("Opening Keynote", None),
                ("All aboard the Rust (electric freight) train!", None),
                ("How we ship Rust in OpenSUSE", None),
                ("The Sheer Terror of PAM", None),
                ("Bootstrapping: The once and future compiler", None),
                ("Weird Expressions and Where to Find Them", None),
                ("Async Rust: Past, Present, and Future", None),
                ("Writing a GraphQL compiler in Rust, a case study", None),
                ("Your Open Source Repo Needs A Project Manager", None),
                (
                    "What If We Pretended Unsafe Code Was Nice, And Then It Was?",
                    None,
                ),
            ],
        }
    }

    // ==================== RustConf 2021 ====================
    fn rustconf_2021_data() -> ConferenceTestData {
        ConferenceTestData {
            year: "2021",
            youtube_titles: vec![
                r#"RustConf 2021 - Project Update: Lang Team by Niko Matsakis"#,
                r#"RustConf 2021 - Project Update: Libs Team by Mara Bos"#,
                r#"RustConf 2021 - Move Constructors: Is it Possible? by Miguel Young de la Sota"#,
                r#"RustConf 2021 - Compile-Time Social Coordination by Zac Burns"#,
                r#"RustConf 2021 - Fuzz Driven Development by Arshia Mufti"#,
                r#"RustConf 2021 - Hacking `rustc`: Contributing to the Compiler by Esteban Kuber"#,
                r#"RustConf 2021 - How I Used Rust to Become Extremely Offline by Lina Cambridge"#,
                r#"RustConf 2021 - Identifying Pokémon Cards by Michael Gattozzi"#,
                r#"RustConf 2021 - Supercharging Your Code With Five Little-Known Attributes"#,
                r#"RustConf 2021 - The Importance of Not Over-Optimizing in Rust by Joshua Mir"#,
                r#"RustConf 2021 - This Week in Rust: 400 Issues and Counting! by Nell Shamrell-Harrington"#,
                r#"RustConf 2021 - Whoops! I Rewrote It in Rust"#,
                r#"RustConf 2021 - Writing the Fastest GBDT Library in Rust"#,
            ],
            pairs: vec![
                ("Project Update: Lang Team", None),
                ("Project Update: Libs Team", None),
                ("Move Constructors: Is it Possible?", None),
                ("Compile-Time Social Coordination", None),
                ("Fuzz Driven Development", None),
                ("Hacking `rustc`: Contributing to the Compiler", None),
                ("How I Used Rust to Become Extremely Offline", None),
                ("Identifying Pokémon Cards", None),
                (
                    "Supercharging Your Code With Five Little-Known Attributes",
                    None,
                ),
                ("The Importance of Not Over-Optimizing in Rust", None),
                ("This Week in Rust: 400 Issues and Counting!", None),
                ("Whoops! I Rewrote It in Rust", None),
                ("Writing the Fastest GBDT Library in Rust", None),
            ],
        }
    }

    // ==================== RustConf 2020 ====================
    fn rustconf_2020_data() -> ConferenceTestData {
        ConferenceTestData {
            year: "2020",
            youtube_titles: vec![
                r#"RustConf 2020 - Opening Keynote"#,
                r#"RustConf 2020 - Error Handling Isn't All About Errors by Jane Lusby"#,
                r#"RustConf 2020 - How to Start a Solo Project that You'll Stick With by Harry Bachrach"#,
                r#"RustConf 2020 - Under a Microscope: Exploring Fast and Safe Rust for Biology"#,
                r#"RustConf 2020 - Bending the Curve: A Personal Tutor at Your Fingertips"#,
                r#"RustConf 2020 - My First Rust Project: Creating a Roguelike with Amethyst"#,
                r#"RustConf 2020 - Macros for a More Productive Rust by jam1garner"#,
                r#"RustConf 2020 - Rust for Non-Systems Programmers by Rebecca Turner"#,
                r#"RustConf 2020 - Controlling Telescope Hardware with Rust"#,
                r#"RustConf 2020 - Closing Keynote by Siân Griffin"#,
            ],
            pairs: vec![
                ("Error Handling Isn't All About Errors", None),
                ("How to Start a Solo Project that You'll Stick With", None),
                (
                    "Under a Microscope: Exploring Fast and Safe Rust for Biology",
                    None,
                ),
                (
                    "Bending the Curve: A Personal Tutor at Your Fingertips",
                    None,
                ),
                (
                    "My First Rust Project: Creating a Roguelike with Amethyst",
                    None,
                ),
                ("Macros for a More Productive Rust", None),
                ("Rust for Non-Systems Programmers", None),
                ("Controlling Telescope Hardware with Rust", None),
                ("Closing Keynote", None),
            ],
        }
    }

    // ==================== RustConf 2019 ====================
    fn rustconf_2019_data() -> ConferenceTestData {
        ConferenceTestData {
            year: "2019",
            youtube_titles: vec![
                r#"RustConf 2019 - Opening Keynote"#,
                r#"RustConf 2019 - The Rust 2018 Module System by Ryan Levick"#,
                r#"RustConf 2019 - From Electron, to WASM, to Rust (aaand back to Electron)"#,
                r#"RustConf 2019 - Rust for Weld, a High Performance Parallel JIT Compiler"#,
                r#"RustConf 2019 - Messing around with fn main() and getting away with it"#,
                r#"RustConf 2019 - Is This Magic!? Ferris Explores Rustc! by David Barsky"#,
                r#"RustConf 2019 - Monotron - Building a retro computer in Embedded Rust"#,
                r#"RustConf 2019 - The Symbiotic Relationship of C++ and Rust by Isabella Muerte"#,
                r#"RustConf 2019 - Taking constant evaluation to the limit by Oliver Scherer"#,
                r#"RustConf 2019 - Towards an Open Ecosystem of Empowered UI Development"#,
                r#"RustConf 2019 - tokio-trace: scoped, structured, async-aware diagnostics"#,
                r#"RustConf 2019 - Syscalls for Rustaceans by Gargi Sharma"#,
                r#"RustConf 2019 - Flatulence, Crystals, and Happy Little Accidents"#,
                r#"RustConf 2019 - Class fixes; or, you become the Rust compiler"#,
                r#"RustConf 2019 - Bringing Rust Home to Meet the Parents"#,
                r#"RustConf 2019 - Closing Keynote"#,
            ],
            pairs: vec![
                ("The Rust 2018 Module System", None),
                (
                    "From Electron, to WASM, to Rust (aaand back to Electron)",
                    None,
                ),
                (
                    "Rust for Weld, a High Performance Parallel JIT Compiler",
                    None,
                ),
                (
                    "Messing around with fn main() and getting away with it",
                    None,
                ),
                ("Is This Magic!? Ferris Explores Rustc!", None),
                (
                    "Monotron - Building a retro computer in Embedded Rust",
                    None,
                ),
                ("The Symbiotic Relationship of C++ and Rust", None),
                ("Taking constant evaluation to the limit", None),
                (
                    "Towards an Open Ecosystem of Empowered UI Development",
                    None,
                ),
                (
                    "tokio-trace: scoped, structured, async-aware diagnostics",
                    None,
                ),
                ("Syscalls for Rustaceans", None),
                ("Flatulence, Crystals, and Happy Little Accidents", None),
                ("Class fixes; or, you become the Rust compiler", None),
                ("Bringing Rust Home to Meet the Parents", None),
            ],
        }
    }

    // ==================== RustConf 2018 ====================
    fn rustconf_2018_data() -> ConferenceTestData {
        ConferenceTestData {
            year: "2018",
            youtube_titles: vec![
                r#"RustConf 2018 - Opening Keynote"#,
                r#"RustConf 2018 - Space, The Rusty Frontier by Ryan Plauche"#,
                r#"RustConf 2018 - Benchmarking and Optimization of Rust Libraries by Paul Mason"#,
                r#"RustConf 2018 - Getting Something for Nothing"#,
                r#"RustConf 2018 - Writing Crates for Complete Beginners - A Tour of Turtle"#,
                r#"RustConf 2018 - Rust and the Web Platform: A Rookie's Guide"#,
                r#"RustConf 2018 - The Dark Secrets Lurking Inside cargo doc"#,
                r#"RustConf 2018 - C2Rust: Migrating Legacy Code to Rust"#,
                r#"RustConf 2018 - My Little Procedural Macro"#,
                r#"RustConf 2018 - Project Mentat: a store for evolving data in Rust"#,
                r#"RustConf 2018 - Using Raft in Rust"#,
                r#"RustConf 2018 - Integrating Rust into Tor: Successes and Challenges"#,
                r#"RustConf 2018 - Embedding Rust in C/C++"#,
                r#"RustConf 2018 - How to (not) introduce Rust at your workplace - a tale"#,
                r#"RustConf 2018 - The Opposite of Spaghetti Code"#,
                r#"RustConf 2018 - Closing Keynote"#,
            ],
            pairs: vec![
                ("Space, The Rusty Frontier", None),
                ("Benchmarking and Optimization of Rust Libraries", None),
                ("Getting Something for Nothing", None),
                (
                    "Writing Crates for Complete Beginners - A Tour of Turtle",
                    None,
                ),
                ("Rust and the Web Platform: A Rookie's Guide", None),
                ("The Dark Secrets Lurking Inside cargo doc", None),
                ("C2Rust: Migrating Legacy Code to Rust", None),
                ("My Little Procedural Macro", None),
                ("Project Mentat: a store for evolving data in Rust", None),
                ("Using Raft in Rust", None),
                ("Integrating Rust into Tor: Successes and Challenges", None),
                ("Embedding Rust in C/C++", None),
                (
                    "How to (not) introduce Rust at your workplace - a tale",
                    None,
                ),
                (
                    "The Opposite of Spaghetti Code: Building for Understanding",
                    None,
                ),
            ],
        }
    }

    // ==================== RustConf 2017 ====================
    fn rustconf_2017_data() -> ConferenceTestData {
        ConferenceTestData {
            year: "2017",
            youtube_titles: vec![
                r#"RustConf 2017 - Opening Keynote by Aaron Turon & Niko Matsakis"#,
                r#"RustConf 2017 - A Tale of Teaching Rust"#,
                r#"RustConf 2017 - Building Rocket"#,
                r#"RustConf 2017 - Shipping a Solid Rust Crate"#,
                r#"RustConf 2017 - Menhir and Friends: the State of the Art of Parsing in Rust"#,
                r#"RustConf 2017 - Type System Tips for the Real World"#,
                r#"RustConf 2017 - Improving Rust Performance Through Profiling and Benchmarking"#,
                r#"RustConf 2017 - Fast, Safe, Pure-Rust Elliptic Curve Cryptography"#,
                r#"RustConf 2017 - Closing Keynote: Safe Systems Software and the Future of Computing"#,
            ],
            pairs: vec![
                ("Opening Keynote", None),
                ("A Tale of Teaching Rust", None),
                ("Building Rocket", None),
                ("Shipping a Solid Rust Crate", None),
                (
                    "Menhir and Friends: the State of the Art of Parsing in Rust",
                    None,
                ),
                ("Type System Tips for the Real World", None),
                (
                    "Improving Rust Performance Through Profiling and Benchmarking",
                    None,
                ),
                ("Fast, Safe, Pure-Rust Elliptic Curve Cryptography", None),
                (
                    "Closing Keynote: Safe Systems Software and the Future of Computing",
                    None,
                ),
            ],
        }
    }

    // ==================== RustConf 2016 ====================
    fn rustconf_2016_data() -> ConferenceTestData {
        ConferenceTestData {
            year: "2016",
            youtube_titles: vec![
                r#"RustConf 2016 - Opening Keynote by Aaron Turon & Niko Matsakis"#,
                r#"RustConf 2016 - Closing Keynote by Julia Evans"#,
                r#"RustConf 2016 - A Modern Editor in Rust"#,
                r#"RustConf 2016 - Using Generics Effectively"#,
                r#"RustConf 2016 - RFC: In Order to Form a More Perfect union"#,
                r#"RustConf 2016 - The RustPlay Classifier"#,
                r#"RustConf 2016 - The Illustrated Adventure Survival Guide"#,
                r#"RustConf 2016 - Back to the Futures"#,
                r#"RustConf 2016 - Integrating Rust and VLC"#,
            ],
            pairs: vec![
                ("Opening Keynote", None),
                ("Closing Keynote", None),
                ("A Modern Editor in Rust", None),
                ("Using Generics Effectively", None),
                ("RFC: In Order to Form a More Perfect union", None),
                ("The RustPlay Classifier", None),
                ("The Illustrated Adventure Survival Guide", None),
                ("Back to the Futures", None),
                ("Integrating Rust and VLC", None),
            ],
        }
    }

    /// Run a single matching test for a conference year
    fn run_matching_test(data: ConferenceTestData) -> (usize, usize, Vec<String>) {
        // Use static IDs to satisfy lifetime requirements
        let (id, url): (&'static str, &'static str) = match data.year {
            "2024" => ("rustconf-2024", "https://2024.rustconf.com"),
            "2023" => ("rustconf-2023", "https://2023.rustconf.com"),
            "2022" => ("rustconf-2022", "https://2022.rustconf.com"),
            "2021" => ("rustconf-2021", "https://2021.rustconf.com"),
            "2020" => ("rustconf-2020", "https://2020.rustconf.com"),
            "2019" => ("rustconf-2019", "https://2019.rustconf.com"),
            "2018" => ("rustconf-2018", "https://2018.rustconf.com"),
            "2017" => ("rustconf-2017", "https://2017.rustconf.com"),
            "2016" => ("rustconf-2016", "https://2016.rustconf.com"),
            _ => panic!("Unknown year: {}", data.year),
        };

        let metadata = ConferenceMetadata {
            id,
            conference: "RustConf",
            year: data.year,
            url: static_url(url),
            youtube_playlist_url: None,
        };

        let items: Vec<ParsedPlaylistItem> = data
            .youtube_titles
            .iter()
            .enumerate()
            .map(|(idx, title)| item(&format!("video-{idx}"), title))
            .collect();

        let index = YoutubePlaylistIndex::new(items, &metadata);

        let mut matched = 0;
        let mut total = 0;
        let mut failures = Vec::new();

        for (db_title, expected_yt_title) in data.pairs {
            total += 1;
            let result = index.find_by_title(db_title, &[]);

            match (result, expected_yt_title) {
                (Some(found), Some(expected)) => {
                    if found.title == expected {
                        matched += 1;
                    } else {
                        failures.push(format!(
                            "[{}] '{}' matched '{}' but expected '{}'",
                            data.year, db_title, found.title, expected
                        ));
                    }
                }
                (Some(_), None) => {
                    // Any match is acceptable
                    matched += 1;
                }
                (None, _) => {
                    failures.push(format!("[{}] No match found for '{}'", data.year, db_title));
                }
            }
        }

        (matched, total, failures)
    }

    #[rstest]
    #[case::rustconf_2024(rustconf_2024_data())]
    #[case::rustconf_2023(rustconf_2023_data())]
    #[case::rustconf_2022(rustconf_2022_data())]
    #[case::rustconf_2021(rustconf_2021_data())]
    #[case::rustconf_2020(rustconf_2020_data())]
    #[case::rustconf_2019(rustconf_2019_data())]
    #[case::rustconf_2018(rustconf_2018_data())]
    #[case::rustconf_2017(rustconf_2017_data())]
    #[case::rustconf_2016(rustconf_2016_data())]
    fn test_rustconf_playlist_matching(#[case] data: ConferenceTestData) {
        let year = data.year;
        let (matched, total, failures) = run_matching_test(data);

        if !failures.is_empty() {
            println!("\n=== Matching failures for RustConf {} ===", year);
            for failure in &failures {
                println!("  {}", failure);
            }
        }

        let match_rate = if total > 0 {
            (matched as f64 / total as f64) * 100.0
        } else {
            100.0
        };

        println!(
            "RustConf {}: {}/{} matched ({:.1}%)",
            year, matched, total, match_rate
        );

        // Assert 100% match rate
        assert_eq!(
            matched, total,
            "RustConf {}: Expected 100% match rate, got {}/{} ({:.1}%)",
            year, matched, total, match_rate
        );
    }

    /// Test that documents the exact YouTube title format patterns we need to match.
    ///
    /// YouTube titles follow the pattern:
    ///   `Speaker Name: "Talk Title" | KEYNOTE | RustConf 2024`
    /// or:
    ///   `Speaker Name: "Talk Title" | RustConf 2024`
    ///
    /// The DB/conference page titles are just:
    ///   `Talk Title`
    ///
    /// This test ensures our fuzzy matching handles these transformations correctly.
    #[test]
    fn test_youtube_title_format_examples() {
        let metadata = ConferenceMetadata {
            id: "rustconf-2024",
            conference: "RustConf",
            year: "2024",
            url: static_url("https://2024.rustconf.com"),
            youtube_playlist_url: None,
        };

        // These are ACTUAL titles from YouTube (with speaker name, quotes, and suffix)
        let youtube_titles = [
            r#"Aeva Black: "Making Open Source Secure by Design" | KEYNOTE | RustConf 2024"#,
            r#"Nick Cameron: "Eternal Sunshine of the Rustfmt'ed Mind" | RustConf 2024"#,
            r#"Jack Wrenn: "Safety Goggles for Alchemists" | RustConf 2024"#,
            r#"Miguel Ojeda (Rust for Linux): KEYNOTE | RustConf 2024"#,
        ];

        let items: Vec<ParsedPlaylistItem> = youtube_titles
            .iter()
            .enumerate()
            .map(|(idx, title)| item(&format!("video-{idx}"), title))
            .collect();

        let index = YoutubePlaylistIndex::new(items, &metadata);

        // These are the ACTUAL titles from the conference homepage/schedule
        // The matching algorithm must find the corresponding YouTube video
        let test_cases = vec![
            // Homepage title -> Expected YouTube title
            (
                "Making Open Source Secure by Design",
                r#"Aeva Black: "Making Open Source Secure by Design" | KEYNOTE | RustConf 2024"#,
            ),
            (
                "Eternal Sunshine of the Rustfmt'ed Mind",
                r#"Nick Cameron: "Eternal Sunshine of the Rustfmt'ed Mind" | RustConf 2024"#,
            ),
            (
                "Safety Goggles for Alchemists",
                r#"Jack Wrenn: "Safety Goggles for Alchemists" | RustConf 2024"#,
            ),
            // Special case: YouTube title has speaker name in parens, no quoted title
            (
                "Rust for Linux",
                r#"Miguel Ojeda (Rust for Linux): KEYNOTE | RustConf 2024"#,
            ),
        ];

        // Test subtitle matching: formal "Main Title: subtitle" -> YouTube only has subtitle
        let subtitle_youtube_titles = [r#"RustConf 2023 - Too many cooks or not enough kitchens?"#];

        let subtitle_items: Vec<ParsedPlaylistItem> = subtitle_youtube_titles
            .iter()
            .enumerate()
            .map(|(idx, title)| item(&format!("subtitle-video-{idx}"), title))
            .collect();

        let subtitle_metadata = ConferenceMetadata {
            id: "rustconf-2023",
            conference: "RustConf",
            year: "2023",
            url: static_url("https://2023.rustconf.com"),
            youtube_playlist_url: None,
        };

        let subtitle_index = YoutubePlaylistIndex::new(subtitle_items, &subtitle_metadata);

        // DB has formal title with colon, YouTube only has the subtitle part
        let subtitle_matched = subtitle_index.find_by_title(
            "Organizational Boundary Problems: too many cooks or not enough kitchens?",
            &[],
        );
        assert!(
            subtitle_matched.is_some(),
            "Formal title with subtitle should match YouTube video that only has the subtitle"
        );
        assert_eq!(
            subtitle_matched.unwrap().title,
            r#"RustConf 2023 - Too many cooks or not enough kitchens?"#,
            "Subtitle matching should find the correct video"
        );

        for (homepage_title, expected_youtube_title) in test_cases {
            let matched = index.find_by_title(homepage_title, &[]);
            assert!(
                matched.is_some(),
                "Homepage title '{}' should match a YouTube video",
                homepage_title
            );
            assert_eq!(
                matched.unwrap().title,
                expected_youtube_title,
                "Homepage title '{}' should match YouTube title '{}'",
                homepage_title,
                expected_youtube_title
            );
        }
    }

    /// Comprehensive test that runs all years and reports aggregate stats
    #[test]
    fn test_all_rustconf_years_summary() {
        let all_data = vec![
            rustconf_2024_data(),
            rustconf_2023_data(),
            rustconf_2022_data(),
            rustconf_2021_data(),
            rustconf_2020_data(),
            rustconf_2019_data(),
            rustconf_2018_data(),
            rustconf_2017_data(),
            rustconf_2016_data(),
        ];

        let mut total_matched = 0;
        let mut total_tests = 0;
        let mut all_failures = Vec::new();

        println!("\n=== RustConf Playlist Matching Summary ===\n");

        for data in all_data {
            let year = data.year;
            let (matched, total, failures) = run_matching_test(data);
            total_matched += matched;
            total_tests += total;

            let match_rate = if total > 0 {
                (matched as f64 / total as f64) * 100.0
            } else {
                100.0
            };

            println!(
                "  RustConf {}: {}/{} ({:.1}%)",
                year, matched, total, match_rate
            );

            all_failures.extend(failures);
        }

        let overall_rate = if total_tests > 0 {
            (total_matched as f64 / total_tests as f64) * 100.0
        } else {
            100.0
        };

        println!(
            "\n  TOTAL: {}/{} ({:.1}%)\n",
            total_matched, total_tests, overall_rate
        );

        if !all_failures.is_empty() {
            println!("=== All Failures ===");
            for failure in &all_failures {
                println!("  {}", failure);
            }
            println!();
        }

        // We want at least 95% match rate overall
        assert!(
            overall_rate >= 95.0,
            "Overall match rate {:.1}% is below 95% threshold. {} failures out of {} tests.",
            overall_rate,
            all_failures.len(),
            total_tests
        );
    }
}
