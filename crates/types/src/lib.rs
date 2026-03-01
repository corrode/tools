#![deny(missing_docs)]
#![deny(rustdoc::missing_crate_level_docs)]

//! # Rust Search Types
//!
//! Shared types used across the search system, including crawler
//! payloads, repository models, and view-layer helpers.

pub mod params;
pub mod search_result;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::sqlite::SqliteRow;
use sqlx::{Database, Decode, Encode, FromRow, Row, Sqlite, Type};
use std::fmt;
use strum::Display;

/// Newtype wrapper around `url::Url` with `sqlx` support.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Url(url::Url);

impl Type<Sqlite> for Url {
    fn type_info() -> <Sqlite as Database>::TypeInfo {
        <String as Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &<Sqlite as Database>::TypeInfo) -> bool {
        <String as Type<Sqlite>>::compatible(ty)
    }
}

impl Encode<'_, Sqlite> for Url {
    fn encode_by_ref(
        &self,
        buf: &mut <Sqlite as Database>::ArgumentBuffer<'_>,
    ) -> Result<IsNull, BoxDynError> {
        <String as Encode<Sqlite>>::encode(self.0.to_string(), buf)
    }
}

impl Decode<'_, Sqlite> for Url {
    fn decode(value: <Sqlite as Database>::ValueRef<'_>) -> Result<Self, BoxDynError> {
        let s = <String as Decode<Sqlite>>::decode(value)?;
        url::Url::parse(&s)
            .map(Url)
            .map_err(|e| Box::new(e) as BoxDynError)
    }
}

impl Url {
    /// Parses the string into a `Url`.
    pub fn parse(input: &str) -> Result<Self, url::ParseError> {
        url::Url::parse(input).map(Self)
    }

    /// Returns the inner string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Returns the host if one exists.
    #[must_use]
    pub fn host_str(&self) -> Option<&str> {
        self.0.host_str()
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<url::Url> for Url {
    fn from(value: url::Url) -> Self {
        Self(value)
    }
}

impl std::ops::Deref for Url {
    type Target = url::Url;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A YouTube video ID (e.g., "dQw4w9WgXcQ").
///
/// This newtype ensures type safety and validates that the ID is not empty
/// or malformed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VideoId(String);

/// Error type for invalid video IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidVideoId(String);

impl fmt::Display for InvalidVideoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid video ID: {}", self.0)
    }
}

impl std::error::Error for InvalidVideoId {}

impl VideoId {
    /// Creates a new VideoId from a string.
    ///
    /// # Errors
    ///
    /// Returns an error if the ID is empty, contains only whitespace,
    /// or contains invalid characters (spaces, emojis, etc.).
    pub fn new(id: impl Into<String>) -> Result<Self, InvalidVideoId> {
        let id = id.into();
        let trimmed = id.trim();

        if trimmed.is_empty() {
            return Err(InvalidVideoId("video ID cannot be empty".to_string()));
        }

        // YouTube video IDs are alphanumeric with hyphens and underscores
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(InvalidVideoId(format!(
                "video ID contains invalid characters: {}",
                trimmed
            )));
        }

        // YouTube video IDs msut have a minimum length (11 characters).
        // We can be lenient here and just enforce a reasonable minimum to catch
        // obvious errors.
        if trimmed.len() < 10 {
            return Err(InvalidVideoId(format!(
                "video ID is too short ({} chars): {}",
                trimmed.len(),
                trimmed
            )));
        }

        Ok(Self(trimmed.to_string()))
    }

    /// Returns the video ID as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the YouTube watch URL for this video.
    #[must_use]
    pub fn watch_url(&self) -> Url {
        Url::parse(&format!("https://www.youtube.com/watch?v={}", self.0))
            .expect("valid YouTube URL")
    }
}

impl fmt::Display for VideoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A YouTube playlist ID.
///
/// This newtype ensures type safety and validates that the ID is not empty.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlaylistId(String);

/// Error type for invalid playlist IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidPlaylistId(String);

impl fmt::Display for InvalidPlaylistId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid playlist ID: {}", self.0)
    }
}

impl std::error::Error for InvalidPlaylistId {}

impl PlaylistId {
    /// Creates a new PlaylistId from a string.
    ///
    /// # Errors
    ///
    /// Returns an error if the ID is empty or contains only whitespace.
    pub fn new(id: impl Into<String>) -> Result<Self, InvalidPlaylistId> {
        let id = id.into();
        let trimmed = id.trim();

        if trimmed.is_empty() {
            return Err(InvalidPlaylistId("playlist ID cannot be empty".to_string()));
        }

        // YouTube playlist IDs start with PL, UU, etc. and are alphanumeric with hyphens/underscores
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(InvalidPlaylistId(format!(
                "playlist ID contains invalid characters: {}",
                trimmed
            )));
        }

        Ok(Self(trimmed.to_string()))
    }

    /// Returns the playlist ID as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the YouTube playlist URL.
    #[must_use]
    pub fn playlist_url(&self) -> Url {
        Url::parse(&format!("https://www.youtube.com/playlist?list={}", self.0))
            .expect("valid YouTube playlist URL")
    }
}

impl fmt::Display for PlaylistId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Top-level content filters used by the UI/API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ContentType {
    /// Articles and other written content.
    Articles,
    /// Videos and other multimedia.
    Video,
    /// Podcast episodes and other audio content.
    Podcast,
    /// Research papers and academic publications.
    Research,
    /// Conference talks and related presentations.
    Talks,
}

/// Duration abstraction used for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duration {
    /// Estimated reading time (minutes).
    ReadingTime(u32),
    /// Video duration (seconds).
    Video(u32),
}

impl Duration {
    /// Creates a reading-time duration from a word count (200 wpm heuristic).
    #[must_use]
    pub fn from_word_count(words: usize) -> Self {
        let minutes = (words / 200).max(1) as u32;
        Self::ReadingTime(minutes)
    }

    /// Creates a duration from seconds.
    #[must_use]
    pub fn from_seconds(seconds: u32) -> Self {
        Self::Video(seconds)
    }

    /// Parses ISO 8601 durations (e.g. `PT1H2M3S`).
    #[must_use]
    pub fn parse_iso8601(duration: &str) -> Option<Self> {
        let duration = duration.strip_prefix("PT")?;

        let mut seconds = 0u32;
        let mut buf = String::new();

        for ch in duration.chars() {
            match ch {
                '0'..='9' => buf.push(ch),
                'H' => {
                    seconds += buf.parse::<u32>().unwrap_or(0) * 3600;
                    buf.clear();
                }
                'M' => {
                    seconds += buf.parse::<u32>().unwrap_or(0) * 60;
                    buf.clear();
                }
                'S' => {
                    seconds += buf.parse::<u32>().unwrap_or(0);
                    buf.clear();
                }
                _ => {}
            }
        }

        Some(Self::Video(seconds))
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadingTime(minutes) => write!(f, "~{minutes} min read"),
            Self::Video(total_seconds) => {
                let hours = total_seconds / 3600;
                let minutes = (total_seconds % 3600) / 60;
                let seconds = total_seconds % 60;

                if hours > 0 {
                    write!(f, "{hours}:{minutes:02}:{seconds:02}")
                } else {
                    write!(f, "{minutes}:{seconds:02}")
                }
            }
        }
    }
}

/// Returns the base data directory.
pub fn get_data_dir() -> String {
    std::env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string())
}

/// Returns the path to the SQLite database file.
pub fn get_search_index_path() -> String {
    std::env::var("SEARCH_INDEX_PATH").unwrap_or_else(|_| format!("{}/index.db", get_data_dir()))
}

/// Common metadata shared by articles and videos.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Metadata {
    /// Title.
    pub title: String,
    /// URL.
    pub url: Url,
    /// Category label.
    pub category: String,
    /// Publication date.
    pub date: NaiveDate,
}

impl fmt::Display for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let encoded = urlencoding::encode(self.url.as_str());
        write!(f, "{}-{encoded}", self.date)
    }
}

/// Article-specific data fields (shared between Article and NewArticle).
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ArticleData {
    /// Common metadata fields.
    #[sqlx(flatten)]
    pub metadata: Metadata,
    /// Full text content for indexing.
    pub text: String,
    /// Optional reference (RFC number, TWiR issue, etc.).
    pub reference: Option<String>,
    /// Word count used for reading-time estimates.
    pub word_count: i64,
}

/// Article row stored in the `articles` table.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Article {
    /// Primary key.
    pub id: i64,
    /// Article data fields.
    #[sqlx(flatten)]
    pub data: ArticleData,
}

/// Payload used when inserting or updating an article.
pub type NewArticle = ArticleData;

impl Article {
    /// Returns the word count for the article.
    #[must_use]
    pub fn word_count(&self) -> usize {
        self.data.word_count.max(0) as usize
    }

    /// Returns the title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.data.metadata.title
    }

    /// Returns the URL.
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.data.metadata.url
    }

    /// Returns the category.
    #[must_use]
    pub fn category(&self) -> &str {
        &self.data.metadata.category
    }

    /// Returns the publication date.
    #[must_use]
    pub fn date(&self) -> NaiveDate {
        self.data.metadata.date
    }

    /// Returns the text content.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.data.text
    }

    /// Returns the reference.
    #[must_use]
    pub fn reference(&self) -> Option<&str> {
        self.data.reference.as_deref()
    }
}

/// Video-specific data fields (shared between Video and NewVideo).
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct VideoData {
    /// Common metadata fields.
    #[sqlx(flatten)]
    pub metadata: Metadata,
    /// Transcript/description for indexing.
    pub text: String,
    /// Optional thumbnail URL.
    pub thumbnail_url: Option<String>,
    /// Duration in seconds, if known.
    pub duration_seconds: Option<i64>,
}

/// Video row stored in the `videos` table.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Video {
    /// Primary key.
    pub id: i64,
    /// Video data fields.
    #[sqlx(flatten)]
    pub data: VideoData,
}

/// Payload used when inserting or updating a video.
pub type NewVideo = VideoData;

/// Podcast episode-specific data fields (shared between PodcastEpisode and NewPodcastEpisode).
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PodcastEpisodeData {
    /// Common metadata fields
    #[sqlx(flatten)]
    pub metadata: Metadata,
    /// Podcast/show name
    pub podcast_name: String,
    /// Episode name (distinct from podcast/show name)
    pub episode_name: String,
    /// Episode summary/description for indexing
    pub summary: String,
    /// Optional thumbnail URL
    pub thumbnail_url: Option<String>,
    /// Duration in seconds, if known
    pub duration_seconds: Option<i64>,
    /// Transcript text for indexing
    pub transcript: String,
}

/// Podcast Episode row stored in the `podcast_episodes` table.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PodcastEpisode {
    /// Primary key.
    pub id: i64,
    /// Podcast episode data fields.
    #[sqlx(flatten)]
    pub data: PodcastEpisodeData,
}

/// Payload used when inserting or updating a podcast episode.
pub type NewPodcastEpisode = PodcastEpisodeData;

impl PodcastEpisode {
    /// Returns the title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.data.metadata.title
    }

    /// Returns the podcast/show name.
    #[must_use]
    pub fn podcast_name(&self) -> &str {
        &self.data.podcast_name
    }

    /// Returns the episode name.
    #[must_use]
    pub fn episode_name(&self) -> &str {
        &self.data.episode_name
    }

    /// Returns the URL.
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.data.metadata.url
    }

    /// Returns the category.
    #[must_use]
    pub fn category(&self) -> &str {
        &self.data.metadata.category
    }

    /// Returns the publication date.
    #[must_use]
    pub fn date(&self) -> NaiveDate {
        self.data.metadata.date
    }

    /// Returns the summary.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.data.summary
    }

    /// Returns the transcript text.
    #[must_use]
    pub fn transcript(&self) -> &str {
        &self.data.transcript
    }

    /// Returns the text content, preferring transcript and falling back to summary.
    #[must_use]
    pub fn text(&self) -> &str {
        if self.data.transcript.is_empty() {
            &self.data.summary
        } else {
            &self.data.transcript
        }
    }

    /// Returns the thumbnail URL.
    #[must_use]
    pub fn thumbnail_url(&self) -> Option<&str> {
        self.data.thumbnail_url.as_deref()
    }

    /// Returns the duration in seconds.
    #[must_use]
    pub fn duration_seconds(&self) -> Option<i64> {
        self.data.duration_seconds
    }
}

impl Video {
    /// Returns the title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.data.metadata.title
    }

    /// Returns the URL.
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.data.metadata.url
    }

    /// Returns the category.
    #[must_use]
    pub fn category(&self) -> &str {
        &self.data.metadata.category
    }

    /// Returns the publication date.
    #[must_use]
    pub fn date(&self) -> NaiveDate {
        self.data.metadata.date
    }

    /// Returns the text content.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.data.text
    }

    /// Returns the thumbnail URL.
    #[must_use]
    pub fn thumbnail_url(&self) -> Option<&str> {
        self.data.thumbnail_url.as_deref()
    }

    /// Returns the duration in seconds.
    #[must_use]
    pub fn duration_seconds(&self) -> Option<i64> {
        self.data.duration_seconds
    }
}

/// Research paper-specific data fields (shared between ResearchPaper and NewResearchPaper).
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ResearchPaperData {
    /// Common metadata fields.
    #[sqlx(flatten)]
    pub metadata: Metadata,
    /// Authors of the research paper.
    pub authors: String,
    /// Abstract or summary of the research paper.
    pub abstract_text: String,
    /// Full text content for indexing (optional, may be empty if not available).
    pub text: String,
    /// DOI (Digital Object Identifier) or arXiv ID.
    pub paper_id: Option<String>,
    /// Publication venue (journal, conference, etc.).
    pub publication: Option<String>,
}

/// Research Paper row stored in the `research_papers` table.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ResearchPaper {
    /// Primary key.
    pub id: i64,
    /// Research paper data fields.
    #[sqlx(flatten)]
    pub data: ResearchPaperData,
}

/// Payload used when inserting or updating a research paper.
pub type NewResearchPaper = ResearchPaperData;

/// ArXiv category codes mapped to human-readable names.
///
/// These are the primary Computer Science subcategories from arXiv.
/// See <https://arxiv.org/category_taxonomy> for the full taxonomy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArxivCategory {
    /// cs.AI — Artificial Intelligence
    CsAI,
    /// cs.AR — Hardware Architecture
    CsAR,
    /// cs.CC — Computational Complexity
    CsCC,
    /// cs.CE — Computational Engineering, Finance, and Science
    CsCE,
    /// cs.CG — Computational Geometry
    CsCG,
    /// cs.CL — Computation and Language
    CsCL,
    /// cs.CR — Cryptography and Security
    CsCR,
    /// cs.CV — Computer Vision and Pattern Recognition
    CsCV,
    /// cs.CY — Computers and Society
    CsCY,
    /// cs.DB — Databases
    CsDB,
    /// cs.DC — Distributed, Parallel, and Cluster Computing
    CsDC,
    /// cs.DL — Digital Libraries
    CsDL,
    /// cs.DM — Discrete Mathematics
    CsDM,
    /// cs.DS — Data Structures and Algorithms
    CsDS,
    /// cs.ET — Emerging Technologies
    CsET,
    /// cs.FL — Formal Languages and Automata Theory
    CsFL,
    /// cs.GL — General Literature
    CsGL,
    /// cs.GR — Graphics
    CsGR,
    /// cs.GT — Computer Science and Game Theory
    CsGT,
    /// cs.HC — Human-Computer Interaction
    CsHC,
    /// cs.IR — Information Retrieval
    CsIR,
    /// cs.IT — Information Theory
    CsIT,
    /// cs.LG — Machine Learning
    CsLG,
    /// cs.LO — Logic in Computer Science
    CsLO,
    /// cs.MA — Multiagent Systems
    CsMA,
    /// cs.MM — Multimedia
    CsMM,
    /// cs.MS — Mathematical Software
    CsMS,
    /// cs.NA — Numerical Analysis
    CsNA,
    /// cs.NE — Neural and Evolutionary Computing
    CsNE,
    /// cs.NI — Networking and Internet Architecture
    CsNI,
    /// cs.OH — Other Computer Science
    CsOH,
    /// cs.OS — Operating Systems
    CsOS,
    /// cs.PF — Performance
    CsPF,
    /// cs.PL — Programming Languages
    CsPL,
    /// cs.RO — Robotics
    CsRO,
    /// cs.SC — Symbolic Computation
    CsSC,
    /// cs.SD — Sound
    CsSD,
    /// cs.SE — Software Engineering
    CsSE,
    /// cs.SI — Social and Information Networks
    CsSI,
    /// cs.SY — Systems and Control
    CsSY,
    /// An unrecognized category code.
    Other(String),
}

impl ArxivCategory {
    /// Parse an arXiv category code (e.g. `"cs.SE"`) into an [`ArxivCategory`].
    #[must_use]
    pub fn from_code(code: &str) -> Self {
        match code {
            "cs.AI" => Self::CsAI,
            "cs.AR" => Self::CsAR,
            "cs.CC" => Self::CsCC,
            "cs.CE" => Self::CsCE,
            "cs.CG" => Self::CsCG,
            "cs.CL" => Self::CsCL,
            "cs.CR" => Self::CsCR,
            "cs.CV" => Self::CsCV,
            "cs.CY" => Self::CsCY,
            "cs.DB" => Self::CsDB,
            "cs.DC" => Self::CsDC,
            "cs.DL" => Self::CsDL,
            "cs.DM" => Self::CsDM,
            "cs.DS" => Self::CsDS,
            "cs.ET" => Self::CsET,
            "cs.FL" => Self::CsFL,
            "cs.GL" => Self::CsGL,
            "cs.GR" => Self::CsGR,
            "cs.GT" => Self::CsGT,
            "cs.HC" => Self::CsHC,
            "cs.IR" => Self::CsIR,
            "cs.IT" => Self::CsIT,
            "cs.LG" => Self::CsLG,
            "cs.LO" => Self::CsLO,
            "cs.MA" => Self::CsMA,
            "cs.MM" => Self::CsMM,
            "cs.MS" => Self::CsMS,
            "cs.NA" => Self::CsNA,
            "cs.NE" => Self::CsNE,
            "cs.NI" => Self::CsNI,
            "cs.OH" => Self::CsOH,
            "cs.OS" => Self::CsOS,
            "cs.PF" => Self::CsPF,
            "cs.PL" => Self::CsPL,
            "cs.RO" => Self::CsRO,
            "cs.SC" => Self::CsSC,
            "cs.SD" => Self::CsSD,
            "cs.SE" => Self::CsSE,
            "cs.SI" => Self::CsSI,
            "cs.SY" => Self::CsSY,
            other => Self::Other(other.to_string()),
        }
    }
}

impl fmt::Display for ArxivCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::CsAI => "Artificial Intelligence",
            Self::CsAR => "Hardware Architecture",
            Self::CsCC => "Computational Complexity",
            Self::CsCE => "Computational Engineering",
            Self::CsCG => "Computational Geometry",
            Self::CsCL => "Computation and Language",
            Self::CsCR => "Cryptography and Security",
            Self::CsCV => "Computer Vision",
            Self::CsCY => "Computers and Society",
            Self::CsDB => "Databases",
            Self::CsDC => "Distributed Computing",
            Self::CsDL => "Digital Libraries",
            Self::CsDM => "Discrete Mathematics",
            Self::CsDS => "Data Structures and Algorithms",
            Self::CsET => "Emerging Technologies",
            Self::CsFL => "Formal Languages",
            Self::CsGL => "General Literature",
            Self::CsGR => "Graphics",
            Self::CsGT => "Game Theory",
            Self::CsHC => "Human-Computer Interaction",
            Self::CsIR => "Information Retrieval",
            Self::CsIT => "Information Theory",
            Self::CsLG => "Machine Learning",
            Self::CsLO => "Logic in Computer Science",
            Self::CsMA => "Multiagent Systems",
            Self::CsMM => "Multimedia",
            Self::CsMS => "Mathematical Software",
            Self::CsNA => "Numerical Analysis",
            Self::CsNE => "Neural and Evolutionary Computing",
            Self::CsNI => "Networking",
            Self::CsOH => "Other",
            Self::CsOS => "Operating Systems",
            Self::CsPF => "Performance",
            Self::CsPL => "Programming Languages",
            Self::CsRO => "Robotics",
            Self::CsSC => "Symbolic Computation",
            Self::CsSD => "Sound",
            Self::CsSE => "Software Engineering",
            Self::CsSI => "Social and Information Networks",
            Self::CsSY => "Systems and Control",
            Self::Other(code) => code.as_str(),
        };
        f.write_str(name)
    }
}

impl ResearchPaper {
    /// Returns the title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.data.metadata.title
    }

    /// Returns the URL.
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.data.metadata.url
    }

    /// Returns the category.
    #[must_use]
    pub fn category(&self) -> &str {
        &self.data.metadata.category
    }

    /// Returns the publication date.
    #[must_use]
    pub fn date(&self) -> NaiveDate {
        self.data.metadata.date
    }

    /// Returns the authors.
    #[must_use]
    pub fn authors(&self) -> &str {
        &self.data.authors
    }

    /// Returns the abstract.
    #[must_use]
    pub fn abstract_text(&self) -> &str {
        &self.data.abstract_text
    }

    /// Returns the full text content.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.data.text
    }

    /// Returns the paper ID (DOI or arXiv ID).
    #[must_use]
    pub fn paper_id(&self) -> Option<&str> {
        self.data.paper_id.as_deref()
    }

    /// Returns the publication venue.
    #[must_use]
    pub fn publication(&self) -> Option<&str> {
        self.data.publication.as_deref()
    }
}

/// Speaker data for conference talks.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Speaker {
    /// Primary key.
    pub id: i64,
    /// Speaker's name.
    pub name: String,
}

/// Data for creating a new speaker (without ID).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSpeaker {
    /// Speaker's name.
    pub name: String,
}

/// Talk-specific data fields.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TalkData {
    /// Talk title.
    pub title: String,
    /// Abstract or description of the talk.
    pub summary: String,
    /// Full transcript of the talk (from video captions or manual transcription).
    /// TODO: Integrate with yt-dlp for auto-generated captions in the future.
    pub transcript: Option<String>,
    /// The conference where this talk was presented (e.g., "RustConf", "EuroRust").
    pub conference: String,
    /// Date when the talk was presented.
    pub date: NaiveDate,
    /// Canonical URL for the talk on the conference website.
    pub website_url: Url,
    /// URL to video recording (YouTube, Vimeo, conference platform, etc.).
    pub video_url: Option<String>,
    /// URL to slide deck (SlideShare, Speaker Deck, Google Slides, PDF, etc.).
    pub slides_url: Option<String>,
    /// Optional thumbnail URL for the talk.
    pub thumbnail_url: Option<String>,
    /// Duration of the talk in seconds.
    pub duration_seconds: Option<i64>,
}

/// Talk row stored in the `talks` table.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Talk {
    /// Primary key.
    pub id: i64,
    /// Talk data fields.
    #[sqlx(flatten)]
    pub data: TalkData,
}

/// Payload used when inserting or updating a talk.
pub type NewTalk = TalkData;

impl Talk {
    /// Returns the title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.data.title
    }

    /// Returns the summary/abstract.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.data.summary
    }

    /// Returns the transcript if available.
    #[must_use]
    pub fn transcript(&self) -> Option<&str> {
        self.data.transcript.as_deref()
    }

    /// Returns the conference name.
    #[must_use]
    pub fn conference(&self) -> &str {
        &self.data.conference
    }

    /// Returns the presentation date.
    #[must_use]
    pub fn date(&self) -> NaiveDate {
        self.data.date
    }

    /// Returns the canonical website URL.
    #[must_use]
    pub fn website_url(&self) -> &Url {
        &self.data.website_url
    }

    /// Returns the video URL if available.
    #[must_use]
    pub fn video_url(&self) -> Option<&str> {
        self.data.video_url.as_deref()
    }

    /// Returns the slides URL if available.
    #[must_use]
    pub fn slides_url(&self) -> Option<&str> {
        self.data.slides_url.as_deref()
    }

    /// Returns the thumbnail URL if available.
    #[must_use]
    pub fn thumbnail_url(&self) -> Option<&str> {
        self.data.thumbnail_url.as_deref()
    }

    /// Returns the duration in seconds if available.
    #[must_use]
    pub fn duration_seconds(&self) -> Option<i64> {
        self.data.duration_seconds
    }

    /// Returns the text content for search, preferring transcript and falling back to summary.
    #[must_use]
    pub fn text(&self) -> &str {
        self.data
            .transcript
            .as_deref()
            .filter(|t| !t.is_empty())
            .unwrap_or(&self.data.summary)
    }
}

/// Quote of the Week.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    /// Quote text.
    pub text: String,
    /// Author attribution.
    pub author: String,
    /// Optional URL for attribution.
    pub url: Option<Url>,
    /// Date of the associated TWiR issue.
    pub date: NaiveDate,
}

/// Unified search entry produced by queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SearchEntry {
    /// Article result.
    Article(Article),
    /// Video result.
    Video(Video),
    /// Podcast episode result.
    Podcast(PodcastEpisode),
    /// Research paper result.
    Research(ResearchPaper),
    /// Conference talk result.
    Talk(Talk),
}

impl SearchEntry {
    /// Returns the title.
    #[must_use]
    pub fn title(&self) -> &str {
        match self {
            Self::Article(article) => article.title(),
            Self::Video(video) => video.title(),
            Self::Podcast(podcast) => podcast.title(),
            Self::Research(paper) => paper.title(),
            Self::Talk(talk) => talk.title(),
        }
    }

    /// Returns the URL.
    #[must_use]
    pub fn url(&self) -> &Url {
        match self {
            Self::Article(article) => article.url(),
            Self::Video(video) => video.url(),
            Self::Podcast(podcast) => podcast.url(),
            Self::Research(paper) => paper.url(),
            Self::Talk(talk) => talk.website_url(),
        }
    }

    /// Returns the category.
    #[must_use]
    pub fn category(&self) -> &str {
        match self {
            Self::Article(article) => article.category(),
            Self::Video(video) => video.category(),
            Self::Podcast(podcast) => podcast.category(),
            Self::Research(paper) => paper.category(),
            Self::Talk(talk) => talk.conference(),
        }
    }

    /// Returns the publication date.
    #[must_use]
    pub fn date(&self) -> NaiveDate {
        match self {
            Self::Article(article) => article.date(),
            Self::Video(video) => video.date(),
            Self::Podcast(podcast) => podcast.date(),
            Self::Research(paper) => paper.date(),
            Self::Talk(talk) => talk.date(),
        }
    }

    /// Returns the text content.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Article(article) => article.text(),
            Self::Video(video) => video.text(),
            Self::Podcast(podcast) => podcast.text(),
            Self::Research(paper) => paper.text(),
            Self::Talk(talk) => talk.text(),
        }
    }

    /// Returns the summary, when available.
    #[must_use]
    pub fn summary(&self) -> Option<&str> {
        match self {
            Self::Podcast(podcast) => Some(podcast.summary()),
            Self::Talk(talk) => Some(talk.summary()),
            _ => None,
        }
    }

    /// Returns the host string from the URL.
    #[must_use]
    pub fn host_str(&self) -> Option<&str> {
        self.url().host_str()
    }

    /// Returns the content type (Article or Video).
    #[must_use]
    pub fn content_type(&self) -> ContentType {
        match self {
            Self::Article(_) => ContentType::Articles,
            Self::Video(_) => ContentType::Video,
            Self::Podcast(_) => ContentType::Podcast,
            Self::Research(_) => ContentType::Research,
            Self::Talk(_) => ContentType::Talks,
        }
    }

    /// Returns the reference string (e.g., RFC number) if available.
    #[must_use]
    pub fn reference(&self) -> Option<&str> {
        match self {
            Self::Article(article) => article.reference(),
            Self::Video(_) => None,
            Self::Podcast(_) => None,
            Self::Research(_) => None,
            Self::Talk(_) => None,
        }
    }

    /// Returns the thumbnail URL if available.
    #[must_use]
    pub fn thumbnail_url(&self) -> Option<&str> {
        match self {
            Self::Article(_) => None,
            Self::Video(video) => video.thumbnail_url(),
            Self::Podcast(podcast) => podcast.thumbnail_url(),
            Self::Research(_) => None,
            Self::Talk(talk) => talk.thumbnail_url(),
        }
    }

    /// Returns the duration in seconds if available (for videos).
    #[must_use]
    pub fn duration_seconds(&self) -> Option<i64> {
        match self {
            Self::Article(_) => None,
            Self::Video(video) => video.duration_seconds(),
            Self::Podcast(podcast) => podcast.duration_seconds(),
            Self::Research(_) => None,
            Self::Talk(talk) => talk.duration_seconds(),
        }
    }

    /// Returns the word count (for articles).
    #[must_use]
    pub fn word_count(&self) -> usize {
        match self {
            Self::Article(article) => article.word_count(),
            Self::Video(_) => 0,
            Self::Podcast(_) => 0,
            Self::Research(_) => 0,
            Self::Talk(_) => 0,
        }
    }
}

/// Search result row returned by the repository.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Entry payload.
    pub entry: SearchEntry,
    /// FTS ranking score.
    pub rank: f64,
    /// Highlight snippet.
    pub snippet: Option<String>,
    /// Title with search terms wrapped in `<mark>` tags (if FTS matched the title).
    pub highlighted_title: Option<String>,
}

impl<'r> FromRow<'r, SqliteRow> for SearchResult {
    fn from_row(row: &'r SqliteRow) -> Result<Self, sqlx::Error> {
        let content_type: String = row.try_get("content_type")?;
        let rank = row.try_get("rank")?;
        let snippet = row.try_get("snippet")?;
        let highlighted_title: Option<String> = row.try_get("highlighted_title").unwrap_or(None);

        let entry = match content_type.as_str() {
            "article" => SearchEntry::Article(Article {
                id: row.try_get("id")?,
                data: ArticleData {
                    metadata: Metadata {
                        title: row.try_get("title")?,
                        url: row.try_get("url")?,
                        category: row.try_get("category")?,
                        date: row.try_get("date")?,
                    },
                    text: row
                        .try_get::<Option<String>, _>("text")?
                        .unwrap_or_default(),
                    reference: row.try_get("reference")?,
                    word_count: row.try_get::<Option<i64>, _>("word_count")?.unwrap_or(0),
                },
            }),
            "video" => SearchEntry::Video(Video {
                id: row.try_get("id")?,
                data: VideoData {
                    metadata: Metadata {
                        title: row.try_get("title")?,
                        url: row.try_get("url")?,
                        category: row.try_get("category")?,
                        date: row.try_get("date")?,
                    },
                    text: row
                        .try_get::<Option<String>, _>("text")?
                        .unwrap_or_default(),
                    thumbnail_url: row.try_get("thumbnail_url")?,
                    duration_seconds: row.try_get("duration_seconds")?,
                },
            }),
            "podcast" => SearchEntry::Podcast(PodcastEpisode {
                id: row.try_get("id")?,
                data: PodcastEpisodeData {
                    metadata: Metadata {
                        title: row.try_get("title")?,
                        url: row.try_get("url")?,
                        category: row.try_get("category")?,
                        date: row.try_get("date")?,
                    },
                    podcast_name: row.try_get("podcast_name")?,
                    episode_name: row.try_get("episode_name")?,
                    summary: row.try_get("summary")?,
                    thumbnail_url: row.try_get("thumbnail_url")?,
                    duration_seconds: row.try_get("duration_seconds")?,
                    transcript: row.try_get("transcript")?,
                },
            }),
            "research" => SearchEntry::Research(ResearchPaper {
                id: row.try_get("id")?,
                data: ResearchPaperData {
                    metadata: Metadata {
                        title: row.try_get("title")?,
                        url: row.try_get("url")?,
                        category: row.try_get("category")?,
                        date: row.try_get("date")?,
                    },
                    authors: row.try_get("authors")?,
                    abstract_text: row.try_get("abstract_text")?,
                    text: row
                        .try_get::<Option<String>, _>("text")?
                        .unwrap_or_default(),
                    paper_id: row.try_get("paper_id")?,
                    publication: row.try_get("publication")?,
                },
            }),
            "talk" => SearchEntry::Talk(Talk {
                id: row.try_get("id")?,
                data: TalkData {
                    title: row.try_get("title")?,
                    summary: row.try_get("summary")?,
                    transcript: row.try_get("transcript")?,
                    conference: row.try_get("conference")?,
                    date: row.try_get("date")?,
                    website_url: row.try_get("url")?,
                    video_url: row.try_get("video_url")?,
                    slides_url: row.try_get("slides_url")?,
                    thumbnail_url: row.try_get("thumbnail_url")?,
                    duration_seconds: row.try_get("duration_seconds")?,
                },
            }),
            other => {
                let err: BoxDynError = format!("unknown content_type: {other}").into();
                return Err(sqlx::Error::Decode(err));
            }
        };

        Ok(Self {
            entry,
            rank,
            snippet,
            highlighted_title,
        })
    }
}

impl SearchResult {
    /// Host helper.
    #[must_use]
    pub fn host_str(&self) -> Option<&str> {
        self.entry.host_str()
    }

    /// URL helper.
    #[must_use]
    pub fn url(&self) -> &Url {
        self.entry.url()
    }

    /// Title helper.
    #[must_use]
    pub fn title(&self) -> &str {
        self.entry.title()
    }

    /// Category helper.
    #[must_use]
    pub fn category(&self) -> &str {
        self.entry.category()
    }

    /// Date helper.
    #[must_use]
    pub fn date(&self) -> NaiveDate {
        self.entry.date()
    }

    /// Word-count helper.
    #[must_use]
    pub fn word_count(&self) -> usize {
        self.entry.word_count()
    }

    /// Duration helper.
    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        match &self.entry {
            SearchEntry::Video(video) => video
                .duration_seconds()
                .map(|seconds: i64| Duration::Video(seconds.max(0) as u32)),
            SearchEntry::Podcast(podcast) => podcast
                .duration_seconds()
                .map(|seconds: i64| Duration::Video(seconds.max(0) as u32)),
            SearchEntry::Talk(talk) => talk
                .duration_seconds()
                .map(|seconds: i64| Duration::Video(seconds.max(0) as u32)),
            SearchEntry::Article(article) => Some(Duration::from_word_count(article.word_count())),
            SearchEntry::Research(_) => None,
        }
    }

    /// Thumbnail helper.
    #[must_use]
    pub fn thumbnail_url(&self) -> Option<&str> {
        self.entry.thumbnail_url()
    }

    /// Reference helper.
    pub fn formatted_reference(&self) -> Option<&str> {
        self.entry.reference()
    }

    /// Content-type helper.
    #[must_use]
    pub fn content_type(&self) -> ContentType {
        self.entry.content_type()
    }

    /// Icon helper used by the UI.
    pub fn icon_svg(&self) -> &'static str {
        let host = self.host_str();

        match host {
            Some("news.ycombinator.com") => {
                r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18"/><path d="M12 6.5l-4 7.5h2v4h4v-4h2z"/></svg>"#
            }
            Some("reddit.com" | "www.reddit.com") => {
                r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><circle cx="9" cy="11" r="1"/><circle cx="15" cy="11" r="1"/><path d="M9 15c.5 1 1.5 2 3 2s2.5-1 3-2"/><path d="M7 11.5C7 10.7 6.5 10 6 10s-1 .7-1 1.5.5 1.5 1 1.5 1-.7 1-1.5z"/><path d="M19 11.5c0-.8-.5-1.5-1-1.5s-1 .7-1 1.5.5 1.5 1 1.5 1-.7 1-1.5z"/></svg>"#
            }
            Some("youtube.com" | "www.youtube.com" | "youtu.be") => {
                r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22.54 6.42a2.78 2.78 0 0 0-1.94-2C18.88 4 12 4 12 4s-6.88 0-8.6.46a2.78 2.78 0 0 0-1.94 2A29 29 0 0 0 1 11.75a29 29 0 0 0 .46 5.33A2.78 2.78 0 0 0 3.4 19c1.72.46 8.6.46 8.6.46s6.88 0 8.6-.46a2.78 2.78 0 0 0 1.94-2 29 29 0 0 0 .46-5.25 29 29 0 0 0-.46-5.33z"/><polygon points="9.75,15.02 15.5,11.75 9.75,8.48"/></svg>"#
            }
            Some("github.com") => {
                r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 19c-5 1.5-5-2.5-7-3m14 6v-3.87a3.37 3.37 0 0 0-.94-2.61c3.14-.35 6.44-1.54 6.44-7A5.44 5.44 0 0 0 20 4.77 5.07 5.07 0 0 0 19.91 1S18.73.65 16 2.48a13.38 13.38 0 0 0-7 0C6.27.65 5.09 1 5.09 1A5.07 5.07 0 0 0 5 4.77a5.44 5.44 0 0 0-1.5 3.78c0 5.42 3.3 6.61 6.44 7A3.37 3.37 0 0 0 9 18.13V22"/></svg>"#
            }
            _ => {
                // Fall back to category icon
                match self.entry.category() {
                    "News & Blog Posts" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 22h16a2 2 0 0 0 2-2V4a2 2 0 0 0-2-2H8a2 2 0 0 0-2 2v16a2 2 0 0 1-2 2Zm0 0a2 2 0 0 1-2-2v-9c0-1.1.9-2 2-2h2"/><path d="M18 14h-8"/><path d="M15 18h-5"/><path d="M10 6h8v4h-8z"/></svg>"#
                    }
                    "Observations/Thoughts" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>"#
                    }
                    "Rust Walkthroughs" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20"/><path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z"/></svg>"#
                    }
                    "Project/Tooling Updates" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>"#
                    }
                    "Miscellaneous" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>"#
                    }
                    "Rust Jobs" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="7" width="20" height="14" rx="2" ry="2"/><path d="M16 21V5a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v16"/></svg>"#
                    }
                    "Newsletters" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z"/><polyline points="22,6 12,13 2,6"/></svg>"#
                    }
                    "Quote of the Week" => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 21c3 0 7-1 7-8V5c0-1.25-.756-2.017-2-2H4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2 1 0 1 0 1 1v1c0 1-1 2-2 2s-1 .008-1 1.031V20c0 1 0 1 1 1z"/><path d="M15 21c3 0 7-1 7-8V5c0-1.25-.757-2.017-2-2h-4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2h.75c0 2.25.25 4-2.75 4v3c0 1 0 1 1 1z"/></svg>"#
                    }
                    _ => {
                        r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>"#
                    }
                }
            }
        }
    }
}

/// Statistics about the indexed content.
#[derive(Debug, Serialize, Deserialize)]
pub struct Stats {
    /// Total number of indexed entries (articles + videos).
    pub total_entries: i64,
    /// Earliest indexed entry date (across all content types).
    pub earliest_date: Option<NaiveDate>,
    /// Latest indexed entry date (across all content types).
    pub latest_date: Option<NaiveDate>,
    /// Total unique domains (across all content types).
    pub total_unique_domains: i64,
    /// Article-specific statistics.
    pub articles: ArticleStats,
    /// Video-specific statistics.
    pub videos: VideoStats,
}

/// Statistics specific to articles.
#[derive(Debug, Serialize, Deserialize)]
pub struct ArticleStats {
    /// Total number of articles.
    pub total: i64,
    /// Average article size in characters.
    pub avg_size_chars: i64,
    /// Total characters across all articles.
    pub total_characters: i64,
    /// Average word count per article.
    pub avg_word_count: i64,
    /// Total words across all articles.
    pub total_words: i64,
    /// Articles per year.
    pub per_year: Vec<YearStats>,
    /// Articles per month.
    pub per_month: Vec<MonthStats>,
    /// Categories and their counts.
    pub categories: Vec<CategoryStats>,
    /// Top domains by year.
    pub top_domains_by_year: Vec<YearlyDomainStats>,
    /// Most prolific domain overall.
    pub top_domain_overall: Option<DomainStats>,
}

/// Statistics specific to videos.
#[derive(Debug, Serialize, Deserialize)]
pub struct VideoStats {
    /// Total number of videos.
    pub total: i64,
    /// Total duration of all videos in seconds.
    pub total_duration_seconds: i64,
    /// Median video duration in seconds.
    pub median_duration_seconds: i64,
    /// Longest video.
    pub longest_video: Option<VideoDurationRecord>,
    /// Shortest video.
    pub shortest_video: Option<VideoDurationRecord>,
    /// Videos per year.
    pub per_year: Vec<YearStats>,
    /// Videos per month.
    pub per_month: Vec<MonthStats>,
    /// Categories and their counts.
    pub categories: Vec<CategoryStats>,
    /// Top channels (domains) for videos.
    pub top_channels: Vec<ChannelStats>,
}

/// A record of a video with its duration (for longest/shortest tracking).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoDurationRecord {
    /// Video title.
    pub title: String,
    /// Video URL.
    pub url: String,
    /// Duration in seconds.
    pub duration_seconds: i64,
}

/// Statistics for a video channel/source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelStats {
    /// Channel or domain name.
    pub channel: String,
    /// Number of videos from this channel.
    pub video_count: i64,
    /// Total duration of videos from this channel in seconds.
    pub total_duration_seconds: i64,
}

/// Category statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryStats {
    /// Category name.
    pub category: String,
    /// Number of entries in this category.
    pub count: i64,
    /// Percentage relative to max category (for progress bar).
    pub percentage: i64,
}

/// Year statistics.
#[derive(Debug, Serialize, Deserialize)]
pub struct YearStats {
    /// Year.
    pub year: i32,
    /// Number of entries in this year.
    pub count: i64,
    /// Percentage relative to max year (for progress bar).
    pub percentage: i64,
}

/// Month statistics (year-month breakdown).
#[derive(Debug, Serialize, Deserialize)]
pub struct MonthStats {
    /// Year-month label (e.g., "2024-01").
    pub year_month: String,
    /// Year.
    pub year: i32,
    /// Month.
    pub month: i32,
    /// Number of entries in this month.
    pub count: i64,
    /// Percentage relative to max month (for bar chart).
    pub percentage: i64,
}

/// Domain statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainStats {
    /// Domain name.
    pub domain: String,
    /// Number of entries from this domain.
    pub count: i64,
}

/// Top domains by year.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YearlyDomainStats {
    /// Year.
    pub year: i32,
    /// Top domains for this year.
    pub domains: Vec<DomainStats>,
}

/// Keyword statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordStats {
    /// Keyword.
    pub keyword: String,
    /// Frequency count.
    pub count: i64,
}

/// Top keywords by year.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YearlyKeywordStats {
    /// Year.
    pub year: i32,
    /// Top keywords for this year.
    pub keywords: Vec<KeywordStats>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_from_word_count() {
        assert_eq!(Duration::from_word_count(0).to_string(), "~1 min read");
        assert_eq!(Duration::from_word_count(100).to_string(), "~1 min read");
        assert_eq!(Duration::from_word_count(200).to_string(), "~1 min read");
        assert_eq!(Duration::from_word_count(400).to_string(), "~2 min read");
        assert_eq!(Duration::from_word_count(1000).to_string(), "~5 min read");
    }

    #[test]
    fn test_duration_video_display() {
        assert_eq!(Duration::Video(0).to_string(), "0:00");
        assert_eq!(Duration::Video(5).to_string(), "0:05");
        assert_eq!(Duration::Video(65).to_string(), "1:05");
        assert_eq!(Duration::Video(3600).to_string(), "1:00:00");
        assert_eq!(Duration::Video(3665).to_string(), "1:01:05");
        assert_eq!(Duration::Video(7325).to_string(), "2:02:05");
    }

    #[test]
    fn test_parse_iso8601() {
        assert_eq!(
            Duration::parse_iso8601("PT1M30S"),
            Some(Duration::Video(90))
        );
        assert_eq!(Duration::parse_iso8601("PT5M"), Some(Duration::Video(300)));
        assert_eq!(Duration::parse_iso8601("PT30S"), Some(Duration::Video(30)));
        assert_eq!(Duration::parse_iso8601("PT1H"), Some(Duration::Video(3600)));
        assert_eq!(
            Duration::parse_iso8601("PT1H2M3S"),
            Some(Duration::Video(3723))
        );
        assert_eq!(
            Duration::parse_iso8601("PT2H30M45S"),
            Some(Duration::Video(9045))
        );
        assert_eq!(Duration::parse_iso8601("invalid"), None);
        assert_eq!(Duration::parse_iso8601("P1D"), None);
    }

    #[test]
    fn test_video_id_valid() {
        let id = VideoId::new("dQw4w9WgXcQ").unwrap();
        assert_eq!(id.as_str(), "dQw4w9WgXcQ");
        assert_eq!(
            id.watch_url().as_str(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
    }

    #[test]
    fn test_video_id_with_hyphen_underscore() {
        let id = VideoId::new("abc-123_XYZ").unwrap();
        assert_eq!(id.as_str(), "abc-123_XYZ");
    }

    #[test]
    fn test_video_id_empty() {
        assert!(VideoId::new("").is_err());
        assert!(VideoId::new("   ").is_err());
    }

    #[test]
    fn test_video_id_invalid_chars() {
        assert!(VideoId::new("abc def").is_err()); // space
        assert!(VideoId::new("abc🎵def").is_err()); // emoji
    }

    #[test]
    fn test_video_id_too_short() {
        assert!(VideoId::new("abc").is_err());
    }

    #[test]
    fn test_playlist_id_valid() {
        let id = PlaylistId::new("PL2b0df3jKKiTWZeF7cip6ZUsaVXxWioRi").unwrap();
        assert_eq!(id.as_str(), "PL2b0df3jKKiTWZeF7cip6ZUsaVXxWioRi");
        assert!(id.playlist_url().as_str().contains("list="));
    }

    #[test]
    fn test_playlist_id_empty() {
        assert!(PlaylistId::new("").is_err());
        assert!(PlaylistId::new("   ").is_err());
    }

    #[test]
    fn test_playlist_id_invalid_chars() {
        assert!(PlaylistId::new("PL abc").is_err()); // space
    }
}
