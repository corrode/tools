#![deny(missing_docs)]
#![deny(rustdoc::missing_crate_level_docs)]

//! # Rust Search Storage
//!
//! This crate provides a repository for storing and retrieving search entries.
//! It uses SQLite as the underlying storage engine with FTS5 for full-text search.
//!
//! ## FTS Architecture
//!
//! The schema uses **separate FTS tables** for each content type:
//! - `articles_fts` - Full-text index for articles
//! - `videos_fts` - Full-text index for videos
//!
//! This design avoids rowid collisions and simplifies joins:
//! ```sql
//! -- Article search
//! SELECT * FROM articles_fts
//! JOIN articles a ON articles_fts.rowid = a.id
//! WHERE articles_fts MATCH ?
//!
//! -- Video search  
//! SELECT * FROM videos_fts
//! JOIN videos v ON videos_fts.rowid = v.id
//! WHERE videos_fts MATCH ?
//! ```
//!
//! ## BM25 Ranking
//!
//! FTS5 provides a hidden `rank` column using the BM25 algorithm. Lower scores
//! indicate better matches. Since both tables use the same FTS5 engine, BM25
//! scores are generally comparable. One caveat: BM25 uses Inverse Document
//! Frequency (IDF), so a rare word in one table may score differently than
//! in another.
//!
//! ## Top-N Optimization (Critical for Performance!)
//!
//! FTS5 has a Top-N optimization: when you `ORDER BY rank LIMIT N`, it doesn't
//! score every match—it uses the index to find the N best and stops early.
//!
//! A naive `UNION ALL` defeats this optimization:
//! ```sql
//! -- SLOW: Forces SQLite to score ALL matches, then sort everything
//! SELECT * FROM (
//!     SELECT ... FROM articles_fts WHERE MATCH ?
//!     UNION ALL
//!     SELECT ... FROM videos_fts WHERE MATCH ?
//! )
//! ORDER BY rank LIMIT 20
//! ```
//!
//! The fix is to "push down" the LIMIT into each subquery:
//! ```sql
//! -- FAST: Each subquery uses Top-N optimization, then we merge 40 rows
//! SELECT * FROM (
//!     SELECT ... FROM articles_fts WHERE MATCH ? ORDER BY rank LIMIT 20
//!     UNION ALL
//!     SELECT ... FROM videos_fts WHERE MATCH ? ORDER BY rank LIMIT 20
//! )
//! ORDER BY rank LIMIT 20
//! ```
//!
//! This way each FTS query uses Top-N (fast), and we only sort 40 rows total
//! instead of potentially thousands.
//!
//! ## Result Weighting (Future)
//!
//! To bias results toward certain content types, multiply the rank:
//! ```sql
//! SELECT ..., rank * 0.8 as rank FROM videos_fts ...  -- Boost videos
//! ```
//! Lower scores = better matches, so multiplying by < 1.0 boosts results.

use anyhow::Context;
use anyhow::Result;
use chrono::NaiveDate;
use sqlx::{
    FromRow, Pool, QueryBuilder, Row, Sqlite,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::path::Path;
use std::str::FromStr;
use types::NewPodcastEpisode;
use types::NewSpeaker;
use types::NewTalk;
use types::PodcastEpisode;
use types::Speaker;
use types::Talk;
use types::Url;
use types::VideoData;
use types::params::{Params, SortOrder};
use types::{
    Article, ArticleStats, CategoryStats, ChannelStats, ContentType, DocumentKind, NewArticle,
    ResearchPaper, SearchResult, Stats, Video, VideoDurationRecord, VideoStats, YearStats,
};

/// Returns the path to the compiled spellfix1 shared library.
///
/// Reads `SPELLFIX_PATH` from the environment, falling back to `ext/spellfix`
/// (without extension — SQLite appends `.so` or `.dylib` automatically).
fn spellfix_extension_path() -> String {
    std::env::var("SPELLFIX_PATH").unwrap_or_else(|_| "ext/spellfix".to_string())
}

/// Manages storage and retrieval of search entries
#[derive(Clone)]
pub struct Repository {
    pool: Pool<Sqlite>,
}

/// Configuration for search queries, bundling all search parameters
#[derive(Debug, Clone)]
pub struct SearchRequest<'a> {
    /// The search parameters used for filtering and ranking results.
    pub params: &'a Params,
}

impl Repository {
    /// Default page size when `params.per_page` is not provided.
    ///
    /// Retained as a `pub const` so external callers (HTML handlers) keep
    /// working; per-request overrides come from [`Params::per_page`].
    pub const RESULTS_PER_PAGE: u32 = 20;

    /// Maximum number of documents allowed in a single batch fetch.
    pub const MAX_BATCH_DOCUMENTS: usize = 25;

    /// Returns a reference to the underlying SQLite connection pool.
    ///
    /// This is used by the monitoring [`SqliteLayer`] which needs its own
    /// handle to the same database for batch-inserting tracing events.
    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    /// Creates a new repository instance.
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let database_url = format!("sqlite://{}?mode=rwc", path.as_ref().display());
        tracing::info!("Opening database at: {database_url}");

        let spellfix_path = spellfix_extension_path();
        tracing::debug!("Loading spellfix1 extension from: {spellfix_path}");

        let options = SqliteConnectOptions::from_str(&database_url)?
            .pragma("trusted_schema", "1")
            .pragma("journal_mode", "WAL")
            .pragma("busy_timeout", "5000")
            .extension(spellfix_path);

        let pool = SqlitePoolOptions::new()
            .max_connections(20)
            .connect_with(options)
            .await
            .context("Failed to connect to SQLite database")?;

        // Verify trusted_schema is enabled — spellfix1 requires it, and if it
        // somehow wasn't set (e.g. a SQLite build that ignores the pragma) we
        // want a clear error at startup rather than a cryptic failure later.
        let trusted: i32 = sqlx::query_scalar("PRAGMA trusted_schema")
            .fetch_one(&pool)
            .await?;
        tracing::debug!("trusted_schema = {}", trusted);
        anyhow::ensure!(
            trusted == 1,
            "trusted_schema is not enabled — the spellfix1 extension cannot be used"
        );

        let repo = Self { pool };
        repo.init_db().await?;
        Ok(repo)
    }

    /// Initializes the database schema.
    async fn init_db(&self) -> Result<()> {
        tracing::info!("Running database migrations");
        sqlx::migrate!("../../migrations").run(&self.pool).await?;
        Ok(())
    }

    /// Looks up spelling corrections for each search term using the spellfix1
    /// `search_vocab` table.
    ///
    /// Returns `None` when every term is already known (edit distance 0) or
    /// when spellfix cannot find a better candidate within a reasonable
    /// distance, avoiding spurious "corrections" of correctly spelled words.
    ///
    /// A `top=1` constraint makes spellfix return only the single best match
    /// per term, keeping the query fast.
    pub async fn correct_terms(&self, params: &Params) -> Result<Option<Vec<String>>> {
        if !params.has_query_terms() {
            return Ok(None);
        }

        let mut corrected: Vec<String> = Vec::new();
        let mut any_changed = false;

        for term in &params.terms {
            // Multi-word phrases come through as a single SearchTerm; correct
            // each constituent word independently then rejoin.
            let words: Vec<&str> = term.as_str().split_whitespace().collect();
            let mut corrected_words: Vec<String> = Vec::new();

            for word in &words {
                // Only attempt correction for purely alphabetic words of
                // reasonable length — numbers, symbols, and very short tokens
                // are not worth correcting.
                let is_alpha = word.chars().all(|c| c.is_ascii_alphabetic());
                if !is_alpha || word.len() < 3 {
                    corrected_words.push((*word).to_owned());
                    continue;
                }

                let row: Option<(String, i64)> = sqlx::query_as(
                    "SELECT word, distance
                     FROM search_vocab
                     WHERE word MATCH ?1
                       AND top = 1
                       AND scope = 2",
                )
                .bind(word.to_lowercase())
                .fetch_optional(&self.pool)
                .await?;

                match row {
                    // The suggestion matches the original word (case-insensitively),
                    // so there is nothing to correct — keep the user's original casing.
                    Some((suggestion, _)) if suggestion.to_lowercase() == word.to_lowercase() => {
                        corrected_words.push((*word).to_owned());
                    }
                    // A plausible correction within the accepted edit-distance budget.
                    // Distance > 200 is too far away and falls through to the catch-all.
                    Some((suggestion, distance)) if distance <= 200 => {
                        tracing::debug!(original = word, correction = %suggestion, distance, "spellfix correction");
                        corrected_words.push(suggestion);
                        any_changed = true;
                    }
                    _ => {
                        // No good correction found; keep the original word.
                        corrected_words.push((*word).to_owned());
                    }
                }
            }

            corrected.push(corrected_words.join(" "));
        }

        if any_changed {
            Ok(Some(corrected))
        } else {
            Ok(None)
        }
    }

    /// Inserts a new quote
    pub async fn insert_quote(&self, quote: &types::Quote) -> Result<()> {
        tracing::debug!("Inserting quote by: {}", quote.author);

        let date_str = quote.date.format("%Y-%m-%d").to_string();
        let url_str = quote.url.as_ref().map(|u| u.as_str());

        sqlx::query(
            r#"
            INSERT INTO quotes(text, author, url, date)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(text, author) DO UPDATE SET
                url = excluded.url,
                date = excluded.date
            "#,
        )
        .bind(&quote.text)
        .bind(&quote.author)
        .bind(url_str)
        .bind(&date_str)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Gets a random quote from the database
    pub async fn get_random_quote(&self) -> Result<Option<types::Quote>> {
        let row = sqlx::query(
            r#"
            SELECT text, author, url, date
            FROM quotes
            ORDER BY RANDOM()
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        let quote = row.map(|row| {
            let url: Option<String> = row.get("url");
            types::Quote {
                text: row.get("text"),
                author: row.get("author"),
                url: url.and_then(|u| Url::parse(&u).ok()),
                date: row.get("date"),
            }
        });

        Ok(quote)
    }

    /// Inserts a new article
    pub async fn insert_article(&self, article: &NewArticle) -> Result<i64> {
        tracing::debug!("Inserting article: {}", article.metadata.url);

        let date_str = article.metadata.date.format("%Y-%m-%d").to_string();
        let url_str = article.metadata.url.as_str();

        let article_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO articles(title, url, category, date, text, reference, word_count)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(url) DO UPDATE SET
                title = excluded.title,
                category = excluded.category,
                date = excluded.date,
                text = excluded.text,
                reference = excluded.reference,
                word_count = excluded.word_count
            RETURNING id
            "#,
        )
        .bind(&article.metadata.title)
        .bind(url_str)
        .bind(&article.metadata.category)
        .bind(&date_str)
        .bind(&article.text)
        .bind(article.reference.as_ref().filter(|r| !r.is_empty()))
        .bind(article.word_count)
        .fetch_one(&self.pool)
        .await?;

        Ok(article_id)
    }

    /// Inserts a new video
    pub async fn insert_video(&self, video: &VideoData) -> Result<i64> {
        tracing::debug!("Inserting video: {}", video.metadata.url);

        let date_str = video.metadata.date.format("%Y-%m-%d").to_string();
        let url_str = video.metadata.url.as_str();

        let video_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO videos(title, url, category, date, text, thumbnail_url, duration_seconds)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(url) DO UPDATE SET
                title = excluded.title,
                category = excluded.category,
                date = excluded.date,
                text = excluded.text,
                thumbnail_url = excluded.thumbnail_url,
                duration_seconds = excluded.duration_seconds
            RETURNING id
            "#,
        )
        .bind(&video.metadata.title)
        .bind(url_str)
        .bind(&video.metadata.category)
        .bind(&date_str)
        .bind(&video.text)
        .bind(&video.thumbnail_url)
        .bind(video.duration_seconds)
        .fetch_one(&self.pool)
        .await?;

        Ok(video_id)
    }

    /// Inserts a new podcast episode
    pub async fn insert_podcast_episode(&self, episode: &NewPodcastEpisode) -> Result<i64> {
        tracing::debug!("Inserting podcast episode: {}", episode.metadata.url);

        let date_str = episode.metadata.date.format("%Y-%m-%d").to_string();
        let url_str = episode.metadata.url.as_str();

        let episode_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO podcast_episodes(
                podcast_name,
                episode_name,
                date,
                duration_seconds,
                summary,
                url,
                thumbnail_url,
                transcript
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(url) DO UPDATE SET
                podcast_name = excluded.podcast_name,
                episode_name = excluded.episode_name,
                date = excluded.date,
                duration_seconds = excluded.duration_seconds,
                summary = excluded.summary,
                thumbnail_url = excluded.thumbnail_url,
                transcript = CASE
                    WHEN length(excluded.transcript) = 0 THEN podcast_episodes.transcript
                    ELSE excluded.transcript
                END
            RETURNING id
            "#,
        )
        .bind(&episode.podcast_name)
        .bind(&episode.episode_name)
        .bind(&date_str)
        .bind(episode.duration_seconds)
        .bind(&episode.summary)
        .bind(url_str)
        .bind(&episode.thumbnail_url)
        .bind(&episode.transcript)
        .fetch_one(&self.pool)
        .await?;

        Ok(episode_id)
    }

    /// Inserts a new research paper
    pub async fn insert_research_paper(&self, paper: &types::NewResearchPaper) -> Result<i64> {
        tracing::debug!("Inserting research paper: {}", paper.metadata.url);

        let date_str = paper.metadata.date.format("%Y-%m-%d").to_string();
        let url_str = paper.metadata.url.as_str();

        let paper_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO research_papers(
                title,
                url,
                category,
                date,
                authors,
                abstract_text,
                text,
                paper_id,
                publication
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(url) DO UPDATE SET
                title = excluded.title,
                category = excluded.category,
                date = excluded.date,
                authors = excluded.authors,
                abstract_text = excluded.abstract_text,
                text = excluded.text,
                paper_id = excluded.paper_id,
                publication = excluded.publication
            RETURNING id
            "#,
        )
        .bind(&paper.metadata.title)
        .bind(url_str)
        .bind(&paper.metadata.category)
        .bind(&date_str)
        .bind(&paper.authors)
        .bind(&paper.abstract_text)
        .bind(&paper.text)
        .bind(&paper.paper_id)
        .bind(&paper.publication)
        .fetch_one(&self.pool)
        .await?;

        Ok(paper_id)
    }

    /// Checks if a URL already exists in the database (articles, videos, or podcasts)
    pub async fn url_exists(&self, url: &Url) -> Result<bool> {
        let url_str = url.as_str();

        let result = sqlx::query(
            r#"
            SELECT 1 FROM articles WHERE url = ?
            UNION ALL
            SELECT 1 FROM videos WHERE url = ?
            UNION ALL
            SELECT 1 FROM podcast_episodes WHERE url = ?
            LIMIT 1
            "#,
        )
        .bind(url_str)
        .bind(url_str)
        .bind(url_str)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.is_some())
    }

    /// Checks if a research paper URL already exists in the research_papers table
    pub async fn research_paper_exists(&self, url: &Url) -> Result<bool> {
        let url_str = url.as_str();

        let result = sqlx::query(
            r#"
            SELECT 1 FROM research_papers WHERE url = ?
            LIMIT 1
            "#,
        )
        .bind(url_str)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.is_some())
    }

    /// Checks if a podcast episode URL already exists in the podcast_episodes table
    pub async fn podcast_episode_exists(&self, url: &Url) -> Result<bool> {
        let url_str = url.as_str();

        let result = sqlx::query(
            r#"
            SELECT 1 FROM podcast_episodes WHERE url = ?
            LIMIT 1
            "#,
        )
        .bind(url_str)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.is_some())
    }

    /// Inserts or updates a talk in the database
    pub async fn insert_talk(&self, talk: &NewTalk) -> Result<i64> {
        tracing::debug!("Inserting talk: {}", talk.website_url);

        let date_str = talk.date.format("%Y-%m-%d").to_string();
        let url_str = talk.website_url.as_str();

        let talk_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO talks(
                title,
                summary,
                transcript,
                conference,
                date,
                website_url,
                video_url,
                slides_url,
                thumbnail_url,
                duration_seconds
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(website_url) DO UPDATE SET
                title = excluded.title,
                summary = excluded.summary,
                transcript = CASE
                    WHEN excluded.transcript IS NULL
                    THEN talks.transcript
                    ELSE excluded.transcript
                END,
                conference = excluded.conference,
                date = excluded.date,
                video_url = COALESCE(excluded.video_url, talks.video_url),
                slides_url = COALESCE(excluded.slides_url, talks.slides_url),
                thumbnail_url = COALESCE(excluded.thumbnail_url, talks.thumbnail_url),
                duration_seconds = COALESCE(excluded.duration_seconds, talks.duration_seconds)
            RETURNING id
            "#,
        )
        .bind(&talk.title)
        .bind(&talk.summary)
        .bind(&talk.transcript)
        .bind(&talk.conference)
        .bind(&date_str)
        .bind(url_str)
        .bind(&talk.video_url)
        .bind(&talk.slides_url)
        .bind(&talk.thumbnail_url)
        .bind(talk.duration_seconds)
        .fetch_one(&self.pool)
        .await?;

        Ok(talk_id)
    }

    /// Checks if a talk URL already exists in the talks table
    pub async fn talk_exists(&self, url: &Url) -> Result<bool> {
        let url_str = url.as_str();

        let result = sqlx::query(
            r#"
            SELECT 1 FROM talks WHERE website_url = ?
            LIMIT 1
            "#,
        )
        .bind(url_str)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.is_some())
    }

    /// Gets a talk by its website URL
    pub async fn get_talk_by_url(&self, url: &Url) -> Result<Option<Talk>> {
        let url_str = url.as_str();

        let talk = sqlx::query_as::<_, Talk>(
            r#"
            SELECT id, title, summary, transcript, conference, date,
                   website_url, video_url, slides_url, thumbnail_url, duration_seconds
            FROM talks
            WHERE website_url = ?
            "#,
        )
        .bind(url_str)
        .fetch_optional(&self.pool)
        .await?;

        Ok(talk)
    }

    /// Gets an article by its primary key, including full text.
    pub async fn get_article_by_id(&self, id: i64) -> Result<Option<Article>> {
        let article = sqlx::query_as::<_, Article>(
            r#"
            SELECT id, title, url, category, date, text, reference, word_count
            FROM articles
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(article)
    }

    /// Gets a video by its primary key, including full transcript/text.
    pub async fn get_video_by_id(&self, id: i64) -> Result<Option<Video>> {
        let video = sqlx::query_as::<_, Video>(
            r#"
            SELECT id, title, url, category, date, text, thumbnail_url, duration_seconds
            FROM videos
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(video)
    }

    /// Gets a talk by its primary key, including full transcript.
    pub async fn get_talk_by_id(&self, id: i64) -> Result<Option<Talk>> {
        let talk = sqlx::query_as::<_, Talk>(
            r#"
            SELECT id, title, summary, transcript, conference, date,
                   website_url, video_url, slides_url, thumbnail_url, duration_seconds
            FROM talks
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(talk)
    }

    /// Gets a research paper by its primary key, including full text.
    pub async fn get_research_paper_by_id(&self, id: i64) -> Result<Option<ResearchPaper>> {
        let paper = sqlx::query_as::<_, ResearchPaper>(
            r#"
            SELECT id, title, url, category, date, authors, abstract_text, text,
                   paper_id, publication
            FROM research_papers
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(paper)
    }

    /// Gets a podcast episode by its primary key.
    pub async fn get_podcast_episode_by_id(&self, id: i64) -> Result<Option<PodcastEpisode>> {
        let episode = sqlx::query_as::<_, PodcastEpisode>(
            r#"
            SELECT
                id,
                episode_name as title,
                url,
                'Podcast' as category,
                date,
                podcast_name,
                episode_name,
                summary,
                thumbnail_url,
                duration_seconds,
                transcript
            FROM podcast_episodes
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(episode)
    }

    /// Gets the names of all guests linked to a podcast episode.
    pub async fn get_podcast_episode_guests(&self, episode_id: i64) -> Result<Vec<String>> {
        let names: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT g.name
            FROM podcast_guests g
            JOIN podcast_episode_guests eg ON eg.guest_id = g.id
            WHERE eg.episode_id = ?
            ORDER BY g.name
            "#,
        )
        .bind(episode_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(names)
    }

    /// Inserts or updates a speaker in the database, returning the speaker ID
    pub async fn upsert_speaker(&self, speaker: &NewSpeaker) -> Result<i64> {
        tracing::debug!("Upserting speaker: {}", speaker.name);

        let speaker_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO speakers(name)
            VALUES (?)
            ON CONFLICT(name) DO UPDATE SET
                name = excluded.name
            RETURNING id
            "#,
        )
        .bind(&speaker.name)
        .fetch_one(&self.pool)
        .await?;

        Ok(speaker_id)
    }

    /// Gets a speaker by name
    pub async fn get_speaker_by_name(&self, name: &str) -> Result<Option<Speaker>> {
        let speaker = sqlx::query_as::<_, Speaker>(
            r#"
            SELECT id, name
            FROM speakers
            WHERE name = ?
            "#,
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        Ok(speaker)
    }

    /// Links a speaker to a talk
    pub async fn link_speaker_to_talk(&self, talk_id: i64, speaker_id: i64) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO talk_speakers(talk_id, speaker_id)
            VALUES (?, ?)
            ON CONFLICT(talk_id, speaker_id) DO NOTHING
            "#,
        )
        .bind(talk_id)
        .bind(speaker_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Gets all speakers for a talk
    pub async fn get_speakers_for_talk(&self, talk_id: i64) -> Result<Vec<Speaker>> {
        let speakers = sqlx::query_as::<_, Speaker>(
            r#"
            SELECT s.id, s.name
            FROM speakers s
            JOIN talk_speakers ts ON s.id = ts.speaker_id
            WHERE ts.talk_id = ?
            ORDER BY s.name
            "#,
        )
        .bind(talk_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(speakers)
    }

    /// Gets the latest entry date from the database (across both tables)
    pub async fn get_latest_entry_date(&self) -> Result<Option<NaiveDate>> {
        let result = sqlx::query(
            r#"
            SELECT MAX(latest_date) as latest_date FROM (
                SELECT MAX(date) as latest_date FROM articles
                UNION ALL
                SELECT MAX(date) as latest_date FROM videos
            )
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = result {
            if let Some(date_str) = row.get::<Option<String>, _>("latest_date") {
                NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                    .map(Some)
                    .context("Failed to parse latest date from database")
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    /// Gets comprehensive statistics about the indexed content
    pub async fn get_stats(&self) -> Result<Stats> {
        // Get date range from both tables
        let date_range = sqlx::query(
            r#"
            SELECT MIN(min_date) as earliest, MAX(max_date) as latest FROM (
                SELECT MIN(date) as min_date, MAX(date) as max_date FROM articles
                UNION ALL
                SELECT MIN(date) as min_date, MAX(date) as max_date FROM videos
                UNION ALL
                SELECT MIN(date) as min_date, MAX(date) as max_date FROM podcast_episodes
                UNION ALL
                SELECT MIN(date) as min_date, MAX(date) as max_date FROM talks
                UNION ALL
                SELECT MIN(date) as min_date, MAX(date) as max_date FROM research_papers
            )
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        let earliest_date = date_range
            .get::<Option<String>, _>("earliest")
            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok());

        let latest_date = date_range
            .get::<Option<String>, _>("latest")
            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok());

        // Get unique domain count (across all content)
        let unique_domains = sqlx::query(
            r#"
            SELECT COUNT(DISTINCT domain) as count FROM (
                SELECT 
                    CASE 
                        WHEN url LIKE '%://%' THEN 
                            SUBSTR(
                                SUBSTR(url, INSTR(url, '://') + 3),
                                1,
                                CASE 
                                    WHEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') > 0 
                                    THEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') - 1
                                    ELSE LENGTH(SUBSTR(url, INSTR(url, '://') + 3))
                                END
                            )
                        ELSE url
                    END as domain
                FROM articles
                UNION ALL
                SELECT 
                    CASE 
                        WHEN url LIKE '%://%' THEN 
                            SUBSTR(
                                SUBSTR(url, INSTR(url, '://') + 3),
                                1,
                                CASE 
                                    WHEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') > 0 
                                    THEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') - 1
                                    ELSE LENGTH(SUBSTR(url, INSTR(url, '://') + 3))
                                END
                            )
                        ELSE url
                    END as domain
                FROM videos
                UNION ALL
                SELECT 
                    CASE 
                        WHEN url LIKE '%://%' THEN 
                            SUBSTR(
                                SUBSTR(url, INSTR(url, '://') + 3),
                                1,
                                CASE 
                                    WHEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') > 0 
                                    THEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') - 1
                                    ELSE LENGTH(SUBSTR(url, INSTR(url, '://') + 3))
                                END
                            )
                        ELSE url
                    END as domain
                FROM podcast_episodes
                UNION ALL
                SELECT 
                    CASE 
                        WHEN website_url LIKE '%://%' THEN 
                            SUBSTR(
                                SUBSTR(website_url, INSTR(website_url, '://') + 3),
                                1,
                                CASE 
                                    WHEN INSTR(SUBSTR(website_url, INSTR(website_url, '://') + 3), '/') > 0 
                                    THEN INSTR(SUBSTR(website_url, INSTR(website_url, '://') + 3), '/') - 1
                                    ELSE LENGTH(SUBSTR(website_url, INSTR(website_url, '://') + 3))
                                END
                            )
                        ELSE website_url
                    END as domain
                FROM talks
                UNION ALL
                SELECT 
                    CASE 
                        WHEN url LIKE '%://%' THEN 
                            SUBSTR(
                                SUBSTR(url, INSTR(url, '://') + 3),
                                1,
                                CASE 
                                    WHEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') > 0 
                                    THEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') - 1
                                    ELSE LENGTH(SUBSTR(url, INSTR(url, '://') + 3))
                                END
                            )
                        ELSE url
                    END as domain
                FROM research_papers
            )
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        let total_unique_domains: i64 = unique_domains.get("count");

        let article_stats = self.get_article_stats().await?;
        let video_stats = self.get_video_stats().await?;

        let podcast_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM podcast_episodes")
            .fetch_one(&self.pool)
            .await?;

        let talk_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM talks")
            .fetch_one(&self.pool)
            .await?;

        let research_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM research_papers")
            .fetch_one(&self.pool)
            .await?;

        let rfc_total: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM articles WHERE category LIKE '%RFC%'")
                .fetch_one(&self.pool)
                .await?;

        let total_entries =
            article_stats.total + video_stats.total + podcast_total + talk_total + research_total;

        Ok(Stats {
            total_entries,
            earliest_date,
            latest_date,
            total_unique_domains,
            articles: article_stats,
            videos: video_stats,
            podcasts: types::PodcastStats {
                total: podcast_total,
            },
            talks: types::TalkStats { total: talk_total },
            research: types::ResearchStats {
                total: research_total,
            },
            rfcs: types::RfcStats { total: rfc_total },
        })
    }

    /// Gets article-specific statistics
    async fn get_article_stats(&self) -> Result<ArticleStats> {
        // Basic counts
        let overview = sqlx::query(
            r#"
            SELECT 
                COUNT(*) as total,
                COALESCE(SUM(LENGTH(text)), 0) as total_chars,
                COALESCE(AVG(LENGTH(text)), 0) as avg_size,
                COALESCE(SUM(word_count), 0) as total_words,
                COALESCE(AVG(word_count), 0) as avg_words
            FROM articles
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        let total: i64 = overview.get("total");
        let total_characters: i64 = overview.get("total_chars");
        let avg_size_chars: i64 = overview.get::<f64, _>("avg_size") as i64;
        let total_words: i64 = overview.get("total_words");
        let avg_word_count: i64 = overview.get::<f64, _>("avg_words") as i64;

        // Categories
        let category_rows = sqlx::query(
            "SELECT category, COUNT(*) as count FROM articles GROUP BY category ORDER BY count DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut categories: Vec<CategoryStats> = category_rows
            .into_iter()
            .map(|row| CategoryStats {
                category: row.get("category"),
                count: row.get("count"),
                percentage: 0,
            })
            .collect();

        if let Some(max_count) = categories.first().map(|c| c.count) {
            for cat in &mut categories {
                cat.percentage = if max_count > 0 {
                    (cat.count * 100) / max_count
                } else {
                    0
                };
            }
        }

        // Per year
        let year_rows = sqlx::query(
            r#"
            SELECT CAST(strftime('%Y', date) as INTEGER) as year, COUNT(*) as count
            FROM articles GROUP BY year ORDER BY year
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut per_year: Vec<YearStats> = year_rows
            .into_iter()
            .map(|row| YearStats {
                year: row.get("year"),
                count: row.get("count"),
                percentage: 0,
            })
            .collect();

        if let Some(max_count) = per_year.iter().map(|y| y.count).max() {
            for year in &mut per_year {
                year.percentage = if max_count > 0 {
                    (year.count * 100) / max_count
                } else {
                    0
                };
            }
        }

        // Per month
        let month_rows = sqlx::query(
            r#"
            SELECT 
                strftime('%Y-%m', date) as year_month,
                CAST(strftime('%Y', date) as INTEGER) as year,
                CAST(strftime('%m', date) as INTEGER) as month,
                COUNT(*) as count
            FROM articles GROUP BY year_month ORDER BY year_month
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut per_month: Vec<types::MonthStats> = month_rows
            .into_iter()
            .map(|row| types::MonthStats {
                year_month: row.get("year_month"),
                year: row.get("year"),
                month: row.get("month"),
                count: row.get("count"),
                percentage: 0,
            })
            .collect();

        if let Some(max_count) = per_month.iter().map(|m| m.count).max() {
            for month in &mut per_month {
                month.percentage = if max_count > 0 {
                    (month.count * 100) / max_count
                } else {
                    0
                };
            }
        }

        // Top domains by year
        let domain_rows = sqlx::query(
            r#"
            SELECT 
                CAST(strftime('%Y', date) as INTEGER) as year,
                CASE 
                    WHEN url LIKE '%://%' THEN 
                        SUBSTR(
                            SUBSTR(url, INSTR(url, '://') + 3),
                            1,
                            CASE 
                                WHEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') > 0 
                                THEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') - 1
                                ELSE LENGTH(SUBSTR(url, INSTR(url, '://') + 3))
                            END
                        )
                    ELSE url
                END as domain,
                COUNT(*) as count
            FROM articles
            GROUP BY year, domain
            ORDER BY year, count DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut domains_by_year: std::collections::HashMap<i32, Vec<types::DomainStats>> =
            std::collections::HashMap::new();
        for row in domain_rows {
            let year: i32 = row.get("year");
            let domain: String = row.get("domain");
            let count: i64 = row.get("count");

            let entry = domains_by_year.entry(year).or_default();
            if entry.len() < 10 {
                entry.push(types::DomainStats { domain, count });
            }
        }

        let mut top_domains_by_year: Vec<types::YearlyDomainStats> = domains_by_year
            .into_iter()
            .map(|(year, domains)| types::YearlyDomainStats { year, domains })
            .collect();
        top_domains_by_year.sort_by_key(|b| std::cmp::Reverse(b.year));

        // Top domain overall
        let top_domain = sqlx::query(
            r#"
            SELECT 
                CASE 
                    WHEN url LIKE '%://%' THEN 
                        SUBSTR(
                            SUBSTR(url, INSTR(url, '://') + 3),
                            1,
                            CASE 
                                WHEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') > 0 
                                THEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') - 1
                                ELSE LENGTH(SUBSTR(url, INSTR(url, '://') + 3))
                            END
                        )
                    ELSE url
                END as domain,
                COUNT(*) as count
            FROM articles
            GROUP BY domain
            ORDER BY count DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        let top_domain_overall = top_domain.map(|row| types::DomainStats {
            domain: row.get("domain"),
            count: row.get("count"),
        });

        Ok(ArticleStats {
            total,
            avg_size_chars,
            total_characters,
            avg_word_count,
            total_words,
            per_year,
            per_month,
            categories,
            top_domains_by_year,
            top_domain_overall,
        })
    }

    /// Gets video-specific statistics
    async fn get_video_stats(&self) -> Result<VideoStats> {
        // Basic counts and duration stats
        let overview = sqlx::query(
            r#"
            SELECT 
                COUNT(*) as total,
                COALESCE(SUM(duration_seconds), 0) as total_duration,
                COALESCE(AVG(duration_seconds), 0) as avg_duration
            FROM videos
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        let total: i64 = overview.get("total");
        let total_duration_seconds: i64 = overview.get("total_duration");

        // Median duration (SQLite doesn't have a built-in median, so we compute it)
        // Only consider videos with non-null duration
        let videos_with_duration: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM videos WHERE duration_seconds IS NOT NULL AND duration_seconds > 0",
        )
        .fetch_one(&self.pool)
        .await?;

        let median_duration_seconds = if videos_with_duration > 0 {
            let median_row = sqlx::query(
                r#"
                SELECT duration_seconds FROM videos
                WHERE duration_seconds IS NOT NULL AND duration_seconds > 0
                ORDER BY duration_seconds
                LIMIT 1 OFFSET ?
                "#,
            )
            .bind(videos_with_duration / 2)
            .fetch_optional(&self.pool)
            .await?;

            median_row
                .map(|r| r.get::<i64, _>("duration_seconds"))
                .unwrap_or(0)
        } else {
            0
        };

        // Longest video
        let longest = sqlx::query(
            r#"
            SELECT title, url, duration_seconds
            FROM videos
            ORDER BY duration_seconds DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        let longest_video = longest.map(|row| VideoDurationRecord {
            title: row.get("title"),
            url: row.get("url"),
            duration_seconds: row.get("duration_seconds"),
        });

        // Shortest video (excluding 0-duration)
        let shortest = sqlx::query(
            r#"
            SELECT title, url, duration_seconds
            FROM videos
            WHERE duration_seconds > 0
            ORDER BY duration_seconds ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        let shortest_video = shortest.map(|row| VideoDurationRecord {
            title: row.get("title"),
            url: row.get("url"),
            duration_seconds: row.get("duration_seconds"),
        });

        // Categories
        let category_rows = sqlx::query(
            "SELECT category, COUNT(*) as count FROM videos GROUP BY category ORDER BY count DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut categories: Vec<CategoryStats> = category_rows
            .into_iter()
            .map(|row| CategoryStats {
                category: row.get("category"),
                count: row.get("count"),
                percentage: 0,
            })
            .collect();

        if let Some(max_count) = categories.first().map(|c| c.count) {
            for cat in &mut categories {
                cat.percentage = if max_count > 0 {
                    (cat.count * 100) / max_count
                } else {
                    0
                };
            }
        }

        // Per year
        let year_rows = sqlx::query(
            r#"
            SELECT CAST(strftime('%Y', date) as INTEGER) as year, COUNT(*) as count
            FROM videos GROUP BY year ORDER BY year
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut per_year: Vec<YearStats> = year_rows
            .into_iter()
            .map(|row| YearStats {
                year: row.get("year"),
                count: row.get("count"),
                percentage: 0,
            })
            .collect();

        if let Some(max_count) = per_year.iter().map(|y| y.count).max() {
            for year in &mut per_year {
                year.percentage = if max_count > 0 {
                    (year.count * 100) / max_count
                } else {
                    0
                };
            }
        }

        // Per month
        let month_rows = sqlx::query(
            r#"
            SELECT 
                strftime('%Y-%m', date) as year_month,
                CAST(strftime('%Y', date) as INTEGER) as year,
                CAST(strftime('%m', date) as INTEGER) as month,
                COUNT(*) as count
            FROM videos GROUP BY year_month ORDER BY year_month
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut per_month: Vec<types::MonthStats> = month_rows
            .into_iter()
            .map(|row| types::MonthStats {
                year_month: row.get("year_month"),
                year: row.get("year"),
                month: row.get("month"),
                count: row.get("count"),
                percentage: 0,
            })
            .collect();

        if let Some(max_count) = per_month.iter().map(|m| m.count).max() {
            for month in &mut per_month {
                month.percentage = if max_count > 0 {
                    (month.count * 100) / max_count
                } else {
                    0
                };
            }
        }

        // Top channels (domains with video count and total duration)
        // Normalize YouTube URL variants (youtu.be, m.youtube.com, youtube.com -> www.youtube.com)
        let channel_rows = sqlx::query(
            r#"
            SELECT 
                CASE 
                    WHEN url LIKE '%youtu.be%' OR url LIKE '%youtube.com%' THEN 'youtube.com'
                    WHEN url LIKE '%://%' THEN 
                        SUBSTR(
                            SUBSTR(url, INSTR(url, '://') + 3),
                            1,
                            CASE 
                                WHEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') > 0 
                                THEN INSTR(SUBSTR(url, INSTR(url, '://') + 3), '/') - 1
                                ELSE LENGTH(SUBSTR(url, INSTR(url, '://') + 3))
                            END
                        )
                    ELSE url
                END as channel,
                COUNT(*) as video_count,
                COALESCE(SUM(duration_seconds), 0) as total_duration
            FROM videos
            GROUP BY channel
            ORDER BY video_count DESC
            LIMIT 10
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let top_channels: Vec<ChannelStats> = channel_rows
            .into_iter()
            .map(|row| ChannelStats {
                channel: row.get("channel"),
                video_count: row.get("video_count"),
                total_duration_seconds: row.get("total_duration"),
            })
            .collect();

        Ok(VideoStats {
            total,
            total_duration_seconds,
            median_duration_seconds,
            longest_video,
            shortest_video,
            per_year,
            per_month,
            categories,
            top_channels,
        })
    }

    /// Clamps the offset so it doesn't exceed the last valid page.
    ///
    /// When `total_count` is known ahead of the data query we can avoid
    /// scanning past the end of the result set entirely.
    fn clamp_offset(offset: u32, total_count: i64, per_page: u32) -> u32 {
        if total_count <= 0 {
            return 0;
        }
        let per_page = i64::from(per_page.max(1));
        let last_page_offset = ((total_count - 1) / per_page) * per_page;
        offset.min(last_page_offset as u32)
    }

    /// Pushes ` AND <alias>.<id_column> IN (?, ?, ...)` (or a tautologically
    /// empty match if the scope contains zero ids for this kind) onto the
    /// query.
    ///
    /// Returns `Some(true)` when results are possible, `Some(false)` when the
    /// scope explicitly excludes this kind (caller should short-circuit), and
    /// `None` when no scope filter is active.
    fn apply_id_scope(
        query: &mut QueryBuilder<'_, Sqlite>,
        params: &Params,
        kind: DocumentKind,
        alias: &str,
        id_column: &str,
    ) -> Option<bool> {
        let scope = params.id_scope.as_ref()?;
        let Some(ids) = scope.ids_for(kind) else {
            // Scope is set but excludes this kind entirely.
            query.push(" AND 0=1");
            return Some(false);
        };
        if ids.is_empty() {
            query.push(" AND 0=1");
            return Some(false);
        }
        query.push(format!(" AND {alias}.{id_column} IN ("));
        let mut sep = query.separated(", ");
        for id in ids {
            sep.push_bind(*id);
        }
        query.push(")");
        Some(true)
    }

    /// Dispatches a search request to the appropriate per-type query based on
    /// the `content_type` filter in the request params. Honors any `id_scope`
    /// in `params` by short-circuiting kinds that are excluded.
    async fn search_by_type(
        &self,
        request: &SearchRequest<'_>,
        offset: u32,
    ) -> Result<(Vec<SearchResult>, i64)> {
        // If an id_scope is set with a specific content type and that kind
        // isn't in the scope, return empty results without hitting the DB.
        if let Some(scope) = request.params.id_scope.as_ref()
            && let Some(ct) = request.params.content_type
        {
            let kind = DocumentKind::from_content_type(ct);
            if scope.ids_for(kind).is_none_or(<[i64]>::is_empty) {
                return Ok((vec![], 0));
            }
        }

        match request.params.content_type {
            Some(ContentType::Articles) => self.search_articles(request, offset).await,
            Some(ContentType::Video) => self.search_videos(request, offset).await,
            Some(ContentType::Podcast) => self.search_podcasts(request, offset).await,
            Some(ContentType::Research) => self.search_research_papers(request, offset).await,
            Some(ContentType::Talks) => self.search_talks(request, offset).await,
            None => self.search_all(request, offset).await,
        }
    }

    /// Searches the index, automatically falling back to spellfix-corrected
    /// terms when the primary query returns zero results.
    pub async fn search(&self, request: &SearchRequest<'_>) -> Result<(Vec<SearchResult>, i64)> {
        let page_num = request.params.page.max(1);
        let per_page = request.params.per_page.max(1);
        let offset = (page_num - 1) * per_page;

        let (results, count) = self.search_by_type(request, offset).await?;

        // If the primary query found nothing and the user typed search terms,
        // ask spellfix1 for corrections and retry once with the corrected terms.
        if count == 0
            && request.params.has_query_terms()
            && let Some(corrected_words) = self.correct_terms(request.params).await?
        {
            let corrected_params = request.params.with_corrected_terms(corrected_words)?;
            let corrected_request = SearchRequest {
                params: &corrected_params,
            };
            return self.search_by_type(&corrected_request, offset).await;
        }

        Ok((results, count))
    }

    fn apply_generic_filters(
        &self,
        query: &mut QueryBuilder<'_, Sqlite>,
        params: &Params,
        alias: &str,
    ) {
        if let Some(site) = params.site_filter() {
            query.push(format!(" AND {alias}.url LIKE "));
            query.push_bind(format!("%{}%", site.as_str()));
        }

        query.push(format!(" AND {alias}.date >= "));
        query.push_bind(format!("{}-01-01", params.start_year));

        query.push(format!(" AND {alias}.date <= "));
        query.push_bind(format!("{}-12-31", params.end_year));
    }

    fn apply_talk_filters(
        &self,
        query: &mut QueryBuilder<'_, Sqlite>,
        params: &Params,
        alias: &str,
    ) {
        if let Some(site) = params.site_filter() {
            query.push(format!(" AND {alias}.website_url LIKE "));
            query.push_bind(format!("%{}%", site.as_str()));
        }

        query.push(format!(" AND {alias}.date >= "));
        query.push_bind(format!("{}-01-01", params.start_year));

        query.push(format!(" AND {alias}.date <= "));
        query.push_bind(format!("{}-12-31", params.end_year));
    }

    async fn search_articles(
        &self,
        request: &SearchRequest<'_>,
        offset: u32,
    ) -> Result<(Vec<SearchResult>, i64)> {
        if !request.params.has_query_terms() {
            return Ok((vec![], 0));
        }

        let fts_query = request
            .params
            .escaped_fts_query()
            .unwrap()
            .as_str()
            .to_string();

        // 1. Get Count
        let mut count_query = QueryBuilder::new(
            r#"
            SELECT COUNT(*) FROM articles_fts
            JOIN articles a ON articles_fts.rowid = a.id
            WHERE articles_fts MATCH "#,
        );
        count_query.push_bind(fts_query.clone());

        self.apply_generic_filters(&mut count_query, request.params, "a");
        Self::apply_id_scope(
            &mut count_query,
            request.params,
            DocumentKind::Article,
            "a",
            "id",
        );

        let total_count: i64 = count_query
            .build_query_as::<(i64,)>()
            .fetch_one(&self.pool)
            .await?
            .0;

        let offset = Self::clamp_offset(offset, total_count, request.params.per_page);

        // 2. Get Results
        let mut query = QueryBuilder::new(
            r#"
            SELECT
                'article' as content_type,
                a.id, a.title, a.url, a.category, a.date, a.text,
                a.reference, a.word_count,
                NULL as thumbnail_url, NULL as duration_seconds,
                bm25(articles_fts, 10.0, 1.0, 1.0) as rank,
                snippet(articles_fts, 2, '<mark>', '</mark>', '...', 50) as snippet,
                highlight(articles_fts, 0, '<mark>', '</mark>') as highlighted_title
            FROM articles_fts
            JOIN articles a ON articles_fts.rowid = a.id
            WHERE articles_fts MATCH "#,
        );
        query.push_bind(fts_query);

        self.apply_generic_filters(&mut query, request.params, "a");
        Self::apply_id_scope(&mut query, request.params, DocumentKind::Article, "a", "id");

        match request.params.sort_by {
            SortOrder::DateDesc => query.push(" ORDER BY a.date DESC"),
            SortOrder::DateAsc => query.push(" ORDER BY a.date ASC"),
            _ => query.push(" ORDER BY rank"),
        };

        query.push(" LIMIT ");
        query.push_bind(i64::from(request.params.per_page));
        query.push(" OFFSET ");
        query.push_bind(offset as i64);

        let rows = query.build().fetch_all(&self.pool).await?;
        let mut results = Vec::with_capacity(rows.len());

        for row in rows {
            results.push(SearchResult::from_row(&row)?);
        }

        Ok((results, total_count))
    }

    async fn search_videos(
        &self,
        request: &SearchRequest<'_>,
        offset: u32,
    ) -> Result<(Vec<SearchResult>, i64)> {
        if !request.params.has_query_terms() {
            return Ok((vec![], 0));
        }

        let fts_query = request
            .params
            .escaped_fts_query()
            .unwrap()
            .as_str()
            .to_string();

        // 1. Get Count
        let mut count_query = QueryBuilder::new(
            r#"
            SELECT COUNT(*) FROM videos_fts
            JOIN videos v ON videos_fts.rowid = v.id
            WHERE videos_fts MATCH "#,
        );
        count_query.push_bind(fts_query.clone());

        self.apply_generic_filters(&mut count_query, request.params, "v");
        Self::apply_id_scope(
            &mut count_query,
            request.params,
            DocumentKind::Video,
            "v",
            "id",
        );

        let total_count: i64 = count_query
            .build_query_as::<(i64,)>()
            .fetch_one(&self.pool)
            .await?
            .0;

        let offset = Self::clamp_offset(offset, total_count, request.params.per_page);

        // 2. Get Results
        let mut query = QueryBuilder::new(
            r#"
            SELECT
                'video' as content_type,
                v.id, v.title, v.url, v.category, v.date, v.text,
                NULL as reference, NULL as word_count,
                v.thumbnail_url, v.duration_seconds,
                bm25(videos_fts, 10.0, 1.0, 1.0) as rank,
                snippet(videos_fts, 2, '<mark>', '</mark>', '...', 50) as snippet,
                highlight(videos_fts, 0, '<mark>', '</mark>') as highlighted_title
            FROM videos_fts
            JOIN videos v ON videos_fts.rowid = v.id
            WHERE videos_fts MATCH "#,
        );
        query.push_bind(fts_query);

        self.apply_generic_filters(&mut query, request.params, "v");
        Self::apply_id_scope(&mut query, request.params, DocumentKind::Video, "v", "id");

        match request.params.sort_by {
            SortOrder::DateDesc => query.push(" ORDER BY v.date DESC"),
            SortOrder::DateAsc => query.push(" ORDER BY v.date ASC"),
            _ => query.push(" ORDER BY rank"),
        };

        query.push(" LIMIT ");
        query.push_bind(i64::from(request.params.per_page));
        query.push(" OFFSET ");
        query.push_bind(offset as i64);

        let rows = query.build().fetch_all(&self.pool).await?;
        let mut results = Vec::with_capacity(rows.len());

        for row in rows {
            results.push(SearchResult::from_row(&row)?);
        }

        Ok((results, total_count))
    }

    async fn search_podcasts(
        &self,
        request: &SearchRequest<'_>,
        offset: u32,
    ) -> Result<(Vec<SearchResult>, i64)> {
        if !request.params.has_query_terms() {
            return Ok((vec![], 0));
        }

        let fts_query = request
            .params
            .escaped_fts_query()
            .unwrap()
            .as_str()
            .to_string();

        // 1. Get Count
        let mut count_query = QueryBuilder::new(
            r#"
            SELECT COUNT(*) FROM podcast_episodes_fts
            JOIN podcast_episodes p ON podcast_episodes_fts.rowid = p.id
            WHERE podcast_episodes_fts MATCH "#,
        );
        count_query.push_bind(fts_query.clone());

        self.apply_generic_filters(&mut count_query, request.params, "p");
        Self::apply_id_scope(
            &mut count_query,
            request.params,
            DocumentKind::Podcast,
            "p",
            "id",
        );

        let total_count: i64 = count_query
            .build_query_as::<(i64,)>()
            .fetch_one(&self.pool)
            .await?
            .0;

        let offset = Self::clamp_offset(offset, total_count, request.params.per_page);

        // 2. Get Results
        let mut query = QueryBuilder::new(
            r#"
            SELECT
                'podcast' as content_type,
                p.id, p.episode_name as title, p.url, 'Podcast' as category, p.date,
                p.podcast_name, p.episode_name,
                p.summary, p.thumbnail_url, p.duration_seconds, p.transcript,
                bm25(podcast_episodes_fts, 2.0, 8.0, 2.0, 1.0) as rank,
                CASE
                    WHEN length(p.transcript) > 0 THEN snippet(podcast_episodes_fts, 3, '<mark>', '</mark>', '...', 50)
                    ELSE snippet(podcast_episodes_fts, 2, '<mark>', '</mark>', '...', 50)
                END as snippet,
                highlight(podcast_episodes_fts, 1, '<mark>', '</mark>') as highlighted_title
            FROM podcast_episodes_fts
            JOIN podcast_episodes p ON podcast_episodes_fts.rowid = p.id
            WHERE podcast_episodes_fts MATCH "#,
        );
        query.push_bind(fts_query);

        self.apply_generic_filters(&mut query, request.params, "p");
        Self::apply_id_scope(&mut query, request.params, DocumentKind::Podcast, "p", "id");

        match request.params.sort_by {
            SortOrder::DateDesc => query.push(" ORDER BY p.date DESC"),
            SortOrder::DateAsc => query.push(" ORDER BY p.date ASC"),
            _ => query.push(" ORDER BY rank"),
        };

        query.push(" LIMIT ");
        query.push_bind(i64::from(request.params.per_page));
        query.push(" OFFSET ");
        query.push_bind(offset as i64);

        let rows = query.build().fetch_all(&self.pool).await?;
        let mut results = Vec::with_capacity(rows.len());

        for row in rows {
            results.push(SearchResult::from_row(&row)?);
        }

        Ok((results, total_count))
    }

    async fn search_research_papers(
        &self,
        request: &SearchRequest<'_>,
        offset: u32,
    ) -> Result<(Vec<SearchResult>, i64)> {
        if !request.params.has_query_terms() {
            return Ok((vec![], 0));
        }

        let fts_query = request
            .params
            .escaped_fts_query()
            .unwrap()
            .as_str()
            .to_string();

        // 1. Get Count
        let mut count_query = QueryBuilder::new(
            r#"
            SELECT COUNT(*) FROM research_papers_fts
            JOIN research_papers r ON research_papers_fts.rowid = r.id
            WHERE research_papers_fts MATCH "#,
        );
        count_query.push_bind(fts_query.clone());

        self.apply_generic_filters(&mut count_query, request.params, "r");
        Self::apply_id_scope(
            &mut count_query,
            request.params,
            DocumentKind::Research,
            "r",
            "id",
        );

        let total_count: i64 = count_query
            .build_query_as::<(i64,)>()
            .fetch_one(&self.pool)
            .await?
            .0;

        let offset = Self::clamp_offset(offset, total_count, request.params.per_page);

        // 2. Get Results
        let mut query = QueryBuilder::new(
            r#"
            SELECT
                'research' as content_type,
                r.id, r.title, r.url, r.category, r.date,
                r.authors,
                snippet(research_papers_fts, 3, '', '', '...', 32) as abstract_text,
                r.text, r.paper_id, r.publication,
                bm25(research_papers_fts, 10.0, 1.0, 1.0, 4.0, 2.0) as rank,
                snippet(research_papers_fts, 3, '<mark>', '</mark>', '...', 32) as snippet,
                highlight(research_papers_fts, 0, '<mark>', '</mark>') as highlighted_title
            FROM research_papers_fts
            JOIN research_papers r ON research_papers_fts.rowid = r.id
            WHERE research_papers_fts MATCH "#,
        );
        query.push_bind(fts_query);

        self.apply_generic_filters(&mut query, request.params, "r");
        Self::apply_id_scope(
            &mut query,
            request.params,
            DocumentKind::Research,
            "r",
            "id",
        );

        match request.params.sort_by {
            SortOrder::DateDesc => query.push(" ORDER BY r.date DESC"),
            SortOrder::DateAsc => query.push(" ORDER BY r.date ASC"),
            _ => query.push(" ORDER BY rank"),
        };

        query.push(" LIMIT ");
        query.push_bind(i64::from(request.params.per_page));
        query.push(" OFFSET ");
        query.push_bind(offset as i64);

        let rows = query.build().fetch_all(&self.pool).await?;
        let mut results = Vec::with_capacity(rows.len());

        for row in rows {
            results.push(SearchResult::from_row(&row)?);
        }

        Ok((results, total_count))
    }

    async fn search_talks(
        &self,
        request: &SearchRequest<'_>,
        offset: u32,
    ) -> Result<(Vec<SearchResult>, i64)> {
        if !request.params.has_query_terms() {
            return Ok((vec![], 0));
        }

        let fts_query = request
            .params
            .escaped_fts_query()
            .unwrap()
            .as_str()
            .to_string();

        // 1. Get Count
        let mut count_query = QueryBuilder::new(
            r#"
            SELECT COUNT(*) FROM talks_fts
            JOIN talks t ON talks_fts.rowid = t.id
            WHERE talks_fts MATCH "#,
        );
        count_query.push_bind(fts_query.clone());

        self.apply_talk_filters(&mut count_query, request.params, "t");
        Self::apply_id_scope(
            &mut count_query,
            request.params,
            DocumentKind::Talk,
            "t",
            "id",
        );

        let total_count: i64 = count_query
            .build_query_as::<(i64,)>()
            .fetch_one(&self.pool)
            .await?
            .0;

        let offset = Self::clamp_offset(offset, total_count, request.params.per_page);

        // 2. Get Results
        let mut query = QueryBuilder::new(
            r#"
            SELECT
                'talk' as content_type,
                t.id, t.title, t.summary, t.transcript, t.conference, t.date,
                t.website_url as url,
                t.video_url, t.slides_url, t.thumbnail_url, t.duration_seconds,
                bm25(talks_fts, 10.0, 2.0, 1.0, 1.0) as rank,
                CASE
                    WHEN length(t.transcript) > 0 THEN snippet(talks_fts, 2, '<mark>', '</mark>', '...', 50)
                    ELSE snippet(talks_fts, 1, '<mark>', '</mark>', '...', 50)
                END as snippet,
                highlight(talks_fts, 0, '<mark>', '</mark>') as highlighted_title
            FROM talks_fts
            JOIN talks t ON talks_fts.rowid = t.id
            WHERE talks_fts MATCH "#,
        );
        query.push_bind(fts_query);

        self.apply_talk_filters(&mut query, request.params, "t");
        Self::apply_id_scope(&mut query, request.params, DocumentKind::Talk, "t", "id");

        match request.params.sort_by {
            SortOrder::DateDesc => query.push(" ORDER BY t.date DESC"),
            SortOrder::DateAsc => query.push(" ORDER BY t.date ASC"),
            _ => query.push(" ORDER BY rank"),
        };

        query.push(" LIMIT ");
        query.push_bind(i64::from(request.params.per_page));
        query.push(" OFFSET ");
        query.push_bind(offset as i64);

        let rows = query.build().fetch_all(&self.pool).await?;
        let mut results = Vec::with_capacity(rows.len());

        for row in rows {
            results.push(SearchResult::from_row(&row)?);
        }

        Ok((results, total_count))
    }

    /// Searches all five kinds (articles, videos, podcasts, talks, research
    /// papers) and merges results by BM25 rank.
    ///
    /// # Performance: Top-N Optimization
    ///
    /// FTS5 has a Top-N optimization: when you `ORDER BY rank LIMIT N`, it doesn't
    /// score every match—it uses the index to find the N best and stops early.
    ///
    /// A naive `UNION ALL` defeats this: SQLite must materialize ALL matches from
    /// each table, sort them, then take the top N. This is orders of magnitude
    /// slower.
    ///
    /// The fix is to "push down" the LIMIT into each subquery:
    /// ```sql
    /// SELECT * FROM (
    ///     SELECT ... FROM articles_fts ... ORDER BY rank LIMIT 20
    ///     UNION ALL
    ///     SELECT ... FROM videos_fts ... ORDER BY rank LIMIT 20
    ///     UNION ALL
    ///     ...
    /// )
    /// ORDER BY rank LIMIT 20
    /// ```
    ///
    /// This way each FTS query uses Top-N optimization (fast), and we only sort
    /// at most `N * num_kinds` rows instead of potentially thousands.
    async fn search_all(
        &self,
        request: &SearchRequest<'_>,
        offset: u32,
    ) -> Result<(Vec<SearchResult>, i64)> {
        // Determine sort order for inner queries (one per alias) and outer.
        let (inner_order_a, outer_order) = match request.params.sort_by {
            SortOrder::DateDesc => ("ORDER BY a.date DESC", "ORDER BY date DESC"),
            SortOrder::DateAsc => ("ORDER BY a.date ASC", "ORDER BY date ASC"),
            _ => ("ORDER BY rank", "ORDER BY rank"),
        };
        let inner_order_v = match request.params.sort_by {
            SortOrder::DateDesc => "ORDER BY v.date DESC",
            SortOrder::DateAsc => "ORDER BY v.date ASC",
            _ => "ORDER BY rank",
        };
        let inner_order_p = match request.params.sort_by {
            SortOrder::DateDesc => "ORDER BY p.date DESC",
            SortOrder::DateAsc => "ORDER BY p.date ASC",
            _ => "ORDER BY rank",
        };
        let inner_order_t = match request.params.sort_by {
            SortOrder::DateDesc => "ORDER BY t.date DESC",
            SortOrder::DateAsc => "ORDER BY t.date ASC",
            _ => "ORDER BY rank",
        };
        let inner_order_r = match request.params.sort_by {
            SortOrder::DateDesc => "ORDER BY r.date DESC",
            SortOrder::DateAsc => "ORDER BY r.date ASC",
            _ => "ORDER BY rank",
        };

        // Separate query to count total results across all five kinds.
        let mut count_query = if request.params.has_query_terms() {
            let fts_query = request
                .params
                .escaped_fts_query()
                .unwrap()
                .as_str()
                .to_string();

            let mut q = QueryBuilder::new(
                r#"
                SELECT SUM(count) FROM (
                    SELECT COUNT(*) as count FROM articles_fts
                    JOIN articles a ON articles_fts.rowid = a.id
                    WHERE articles_fts MATCH "#,
            );
            q.push_bind(fts_query.clone());
            self.apply_generic_filters(&mut q, request.params, "a");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Article, "a", "id");

            q.push(
                r#"
                    UNION ALL
                    SELECT COUNT(*) as count FROM videos_fts
                    JOIN videos v ON videos_fts.rowid = v.id
                    WHERE videos_fts MATCH "#,
            );
            q.push_bind(fts_query.clone());
            self.apply_generic_filters(&mut q, request.params, "v");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Video, "v", "id");

            q.push(
                r#"
                    UNION ALL
                    SELECT COUNT(*) as count FROM podcast_episodes_fts
                    JOIN podcast_episodes p ON podcast_episodes_fts.rowid = p.id
                    WHERE podcast_episodes_fts MATCH "#,
            );
            q.push_bind(fts_query.clone());
            self.apply_generic_filters(&mut q, request.params, "p");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Podcast, "p", "id");

            q.push(
                r#"
                    UNION ALL
                    SELECT COUNT(*) as count FROM talks_fts
                    JOIN talks t ON talks_fts.rowid = t.id
                    WHERE talks_fts MATCH "#,
            );
            q.push_bind(fts_query.clone());
            self.apply_talk_filters(&mut q, request.params, "t");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Talk, "t", "id");

            q.push(
                r#"
                    UNION ALL
                    SELECT COUNT(*) as count FROM research_papers_fts
                    JOIN research_papers r ON research_papers_fts.rowid = r.id
                    WHERE research_papers_fts MATCH "#,
            );
            q.push_bind(fts_query);
            self.apply_generic_filters(&mut q, request.params, "r");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Research, "r", "id");

            q.push(")");
            q
        } else {
            let mut q = QueryBuilder::new(
                r#"
                SELECT SUM(count) FROM (
                    SELECT COUNT(*) as count FROM articles a WHERE 1=1"#,
            );
            self.apply_generic_filters(&mut q, request.params, "a");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Article, "a", "id");

            q.push(
                r#"
                    UNION ALL
                    SELECT COUNT(*) as count FROM videos v WHERE 1=1"#,
            );
            self.apply_generic_filters(&mut q, request.params, "v");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Video, "v", "id");

            q.push(
                r#"
                    UNION ALL
                    SELECT COUNT(*) as count FROM podcast_episodes p WHERE 1=1"#,
            );
            self.apply_generic_filters(&mut q, request.params, "p");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Podcast, "p", "id");

            q.push(
                r#"
                    UNION ALL
                    SELECT COUNT(*) as count FROM talks t WHERE 1=1"#,
            );
            self.apply_talk_filters(&mut q, request.params, "t");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Talk, "t", "id");

            q.push(
                r#"
                    UNION ALL
                    SELECT COUNT(*) as count FROM research_papers r WHERE 1=1"#,
            );
            self.apply_generic_filters(&mut q, request.params, "r");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Research, "r", "id");

            q.push(")");
            q
        };

        let total_count: i64 = count_query
            .build_query_as::<(i64,)>()
            .fetch_one(&self.pool)
            .await?
            .0;

        let offset = Self::clamp_offset(offset, total_count, request.params.per_page);

        // We need to fetch enough results from each table to satisfy pagination.
        // For page N with per_page results, we need offset + per_page results.
        let inner_limit = i64::from(offset) + i64::from(request.params.per_page);

        let mut query = if request.params.has_query_terms() {
            let fts_query = request
                .params
                .escaped_fts_query()
                .unwrap()
                .as_str()
                .to_string();

            let mut q = QueryBuilder::new(
                r#"
                SELECT * FROM (
                    SELECT * FROM (
                        SELECT
                            'article' as content_type,
                            a.id, a.title, a.url, a.category, a.date,
                            NULL as podcast_name, NULL as episode_name,
                            a.text,
                            a.reference, a.word_count,
                            NULL as summary, NULL as transcript,
                            NULL as thumbnail_url, NULL as duration_seconds,
                            NULL as authors, NULL as abstract_text,
                            NULL as paper_id, NULL as publication,
                            NULL as conference, NULL as video_url, NULL as slides_url,
                            bm25(articles_fts, 10.0, 1.0, 1.0) as rank,
                            snippet(articles_fts, 2, '<mark>', '</mark>', '...', 50) as snippet,
                            highlight(articles_fts, 0, '<mark>', '</mark>') as highlighted_title
                        FROM articles_fts
                        JOIN articles a ON articles_fts.rowid = a.id
                        WHERE articles_fts MATCH "#,
            );
            q.push_bind(fts_query.clone());
            self.apply_generic_filters(&mut q, request.params, "a");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Article, "a", "id");
            q.push(" ");
            q.push(inner_order_a);
            q.push(" LIMIT ");
            q.push_bind(inner_limit);

            q.push(
                r#")
                    UNION ALL
                    SELECT * FROM (
                        SELECT
                            'video' as content_type,
                            v.id, v.title, v.url, v.category, v.date,
                            NULL as podcast_name, NULL as episode_name,
                            v.text,
                            NULL as reference, NULL as word_count,
                            NULL as summary, NULL as transcript,
                            v.thumbnail_url, v.duration_seconds,
                            NULL as authors, NULL as abstract_text,
                            NULL as paper_id, NULL as publication,
                            NULL as conference, NULL as video_url, NULL as slides_url,
                            bm25(videos_fts, 10.0, 1.0, 1.0) as rank,
                            snippet(videos_fts, 2, '<mark>', '</mark>', '...', 50) as snippet,
                            highlight(videos_fts, 0, '<mark>', '</mark>') as highlighted_title
                        FROM videos_fts
                        JOIN videos v ON videos_fts.rowid = v.id
                        WHERE videos_fts MATCH "#,
            );
            q.push_bind(fts_query.clone());
            self.apply_generic_filters(&mut q, request.params, "v");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Video, "v", "id");
            q.push(" ");
            q.push(inner_order_v);
            q.push(" LIMIT ");
            q.push_bind(inner_limit);

            q.push(
                r#")
                    UNION ALL
                    SELECT * FROM (
                        SELECT
                            'podcast' as content_type,
                            p.id, p.episode_name as title, p.url, 'Podcast' as category, p.date,
                            p.podcast_name, p.episode_name,
                            p.transcript as text,
                            NULL as reference, NULL as word_count,
                            p.summary, p.transcript,
                            p.thumbnail_url, p.duration_seconds,
                            NULL as authors, NULL as abstract_text,
                            NULL as paper_id, NULL as publication,
                            NULL as conference, NULL as video_url, NULL as slides_url,
                            bm25(podcast_episodes_fts, 2.0, 8.0, 2.0, 1.0) as rank,
                            CASE
                                WHEN length(p.transcript) > 0 THEN snippet(podcast_episodes_fts, 3, '<mark>', '</mark>', '...', 50)
                                ELSE snippet(podcast_episodes_fts, 2, '<mark>', '</mark>', '...', 50)
                            END as snippet,
                            highlight(podcast_episodes_fts, 1, '<mark>', '</mark>') as highlighted_title
                        FROM podcast_episodes_fts
                        JOIN podcast_episodes p ON podcast_episodes_fts.rowid = p.id
                        WHERE podcast_episodes_fts MATCH "#,
            );
            q.push_bind(fts_query.clone());
            self.apply_generic_filters(&mut q, request.params, "p");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Podcast, "p", "id");
            q.push(" ");
            q.push(inner_order_p);
            q.push(" LIMIT ");
            q.push_bind(inner_limit);

            q.push(
                r#")
                    UNION ALL
                    SELECT * FROM (
                        SELECT
                            'talk' as content_type,
                            t.id, t.title, t.website_url as url, NULL as category, t.date,
                            NULL as podcast_name, NULL as episode_name,
                            NULL as text,
                            NULL as reference, NULL as word_count,
                            t.summary, t.transcript,
                            t.thumbnail_url, t.duration_seconds,
                            NULL as authors, NULL as abstract_text,
                            NULL as paper_id, NULL as publication,
                            t.conference, t.video_url, t.slides_url,
                            bm25(talks_fts, 10.0, 2.0, 1.0, 1.0) as rank,
                            CASE
                                WHEN length(t.transcript) > 0 THEN snippet(talks_fts, 2, '<mark>', '</mark>', '...', 50)
                                ELSE snippet(talks_fts, 1, '<mark>', '</mark>', '...', 50)
                            END as snippet,
                            highlight(talks_fts, 0, '<mark>', '</mark>') as highlighted_title
                        FROM talks_fts
                        JOIN talks t ON talks_fts.rowid = t.id
                        WHERE talks_fts MATCH "#,
            );
            q.push_bind(fts_query.clone());
            self.apply_talk_filters(&mut q, request.params, "t");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Talk, "t", "id");
            q.push(" ");
            q.push(inner_order_t);
            q.push(" LIMIT ");
            q.push_bind(inner_limit);

            q.push(
                r#")
                    UNION ALL
                    SELECT * FROM (
                        SELECT
                            'research' as content_type,
                            r.id, r.title, r.url, r.category, r.date,
                            NULL as podcast_name, NULL as episode_name,
                            r.text,
                            NULL as reference, NULL as word_count,
                            NULL as summary, NULL as transcript,
                            NULL as thumbnail_url, NULL as duration_seconds,
                            r.authors,
                            snippet(research_papers_fts, 3, '', '', '...', 32) as abstract_text,
                            r.paper_id, r.publication,
                            NULL as conference, NULL as video_url, NULL as slides_url,
                            bm25(research_papers_fts, 10.0, 1.0, 1.0, 4.0, 2.0) as rank,
                            snippet(research_papers_fts, 3, '<mark>', '</mark>', '...', 32) as snippet,
                            highlight(research_papers_fts, 0, '<mark>', '</mark>') as highlighted_title
                        FROM research_papers_fts
                        JOIN research_papers r ON research_papers_fts.rowid = r.id
                        WHERE research_papers_fts MATCH "#,
            );
            q.push_bind(fts_query);
            self.apply_generic_filters(&mut q, request.params, "r");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Research, "r", "id");
            q.push(" ");
            q.push(inner_order_r);
            q.push(" LIMIT ");
            q.push_bind(inner_limit);

            q.push("))");
            q
        } else {
            // No search terms - simple UNION without FTS
            let mut q = QueryBuilder::new(
                r#"
                SELECT * FROM (
                    SELECT * FROM (
                        SELECT
                            'article' as content_type,
                            a.id, a.title, a.url, a.category, a.date,
                            NULL as podcast_name, NULL as episode_name,
                            a.text,
                            a.reference, a.word_count,
                            NULL as summary, NULL as transcript,
                            NULL as thumbnail_url, NULL as duration_seconds,
                            NULL as authors, NULL as abstract_text,
                            NULL as paper_id, NULL as publication,
                            NULL as conference, NULL as video_url, NULL as slides_url,
                            0.0 as rank,
                            substr(a.text, 1, 300) as snippet,
                            NULL as highlighted_title
                        FROM articles a
                        WHERE 1=1"#,
            );
            self.apply_generic_filters(&mut q, request.params, "a");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Article, "a", "id");
            q.push(" ");
            q.push(inner_order_a);
            q.push(" LIMIT ");
            q.push_bind(inner_limit);

            q.push(
                r#")
                    UNION ALL
                    SELECT * FROM (
                        SELECT
                            'video' as content_type,
                            v.id, v.title, v.url, v.category, v.date,
                            NULL as podcast_name, NULL as episode_name,
                            v.text,
                            NULL as reference, NULL as word_count,
                            NULL as summary, NULL as transcript,
                            v.thumbnail_url, v.duration_seconds,
                            NULL as authors, NULL as abstract_text,
                            NULL as paper_id, NULL as publication,
                            NULL as conference, NULL as video_url, NULL as slides_url,
                            0.0 as rank,
                            substr(v.text, 1, 300) as snippet,
                            NULL as highlighted_title
                        FROM videos v
                        WHERE 1=1"#,
            );
            self.apply_generic_filters(&mut q, request.params, "v");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Video, "v", "id");
            q.push(" ");
            q.push(inner_order_v);
            q.push(" LIMIT ");
            q.push_bind(inner_limit);

            q.push(
                r#")
                    UNION ALL
                    SELECT * FROM (
                        SELECT
                            'podcast' as content_type,
                            p.id, p.episode_name as title, p.url, 'Podcast' as category, p.date,
                            p.podcast_name, p.episode_name,
                            p.transcript as text,
                            NULL as reference, NULL as word_count,
                            p.summary, p.transcript,
                            p.thumbnail_url, p.duration_seconds,
                            NULL as authors, NULL as abstract_text,
                            NULL as paper_id, NULL as publication,
                            NULL as conference, NULL as video_url, NULL as slides_url,
                            0.0 as rank,
                            COALESCE(substr(p.summary, 1, 300), substr(p.transcript, 1, 300)) as snippet,
                            NULL as highlighted_title
                        FROM podcast_episodes p
                        WHERE 1=1"#,
            );
            self.apply_generic_filters(&mut q, request.params, "p");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Podcast, "p", "id");
            q.push(" ");
            q.push(inner_order_p);
            q.push(" LIMIT ");
            q.push_bind(inner_limit);

            q.push(
                r#")
                    UNION ALL
                    SELECT * FROM (
                        SELECT
                            'talk' as content_type,
                            t.id, t.title, t.website_url as url, NULL as category, t.date,
                            NULL as podcast_name, NULL as episode_name,
                            NULL as text,
                            NULL as reference, NULL as word_count,
                            t.summary, t.transcript,
                            t.thumbnail_url, t.duration_seconds,
                            NULL as authors, NULL as abstract_text,
                            NULL as paper_id, NULL as publication,
                            t.conference, t.video_url, t.slides_url,
                            0.0 as rank,
                            substr(t.summary, 1, 300) as snippet,
                            NULL as highlighted_title
                        FROM talks t
                        WHERE 1=1"#,
            );
            self.apply_talk_filters(&mut q, request.params, "t");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Talk, "t", "id");
            q.push(" ");
            q.push(inner_order_t);
            q.push(" LIMIT ");
            q.push_bind(inner_limit);

            q.push(
                r#")
                    UNION ALL
                    SELECT * FROM (
                        SELECT
                            'research' as content_type,
                            r.id, r.title, r.url, r.category, r.date,
                            NULL as podcast_name, NULL as episode_name,
                            r.text,
                            NULL as reference, NULL as word_count,
                            NULL as summary, NULL as transcript,
                            NULL as thumbnail_url, NULL as duration_seconds,
                            r.authors,
                            substr(r.abstract_text, 1, 300) as abstract_text,
                            r.paper_id, r.publication,
                            NULL as conference, NULL as video_url, NULL as slides_url,
                            0.0 as rank,
                            substr(r.abstract_text, 1, 300) as snippet,
                            NULL as highlighted_title
                        FROM research_papers r
                        WHERE 1=1"#,
            );
            self.apply_generic_filters(&mut q, request.params, "r");
            Self::apply_id_scope(&mut q, request.params, DocumentKind::Research, "r", "id");
            q.push(" ");
            q.push(inner_order_r);
            q.push(" LIMIT ");
            q.push_bind(inner_limit);

            q.push("))");
            q
        };

        // Outer ORDER BY sorts the combined results from all subqueries
        query.push(" ");
        query.push(outer_order);

        // Final pagination
        query.push(" LIMIT ");
        query.push_bind(i64::from(request.params.per_page));
        query.push(" OFFSET ");
        query.push_bind(i64::from(offset));

        let results: Vec<SearchResult> = query.build_query_as().fetch_all(&self.pool).await?;
        Ok((results, total_count))
    }

    /// Returns up to `limit` search suggestions whose phrase starts with `prefix`.
    ///
    /// Suggestions are pre-materialised bigrams and trigrams from all indexed
    /// titles, ranked by co-occurrence frequency. The query is a simple
    /// index-range scan — sub-millisecond.
    pub async fn get_suggestions(&self, prefix: &str, limit: u32) -> Result<Vec<String>> {
        if prefix.trim().is_empty() {
            return Ok(vec![]);
        }
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT phrase FROM suggestions
             WHERE phrase LIKE ? || '%'
             ORDER BY cnt DESC
             LIMIT ?",
        )
        .bind(prefix.to_lowercase())
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(phrase,)| phrase).collect())
    }

    // -----------------------------------------------------------------------
}
