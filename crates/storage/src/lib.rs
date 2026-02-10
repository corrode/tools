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
    Pool, QueryBuilder, Row, Sqlite,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::path::Path;
use std::str::FromStr;
use types::Url;
use types::{
    ArticleStats, CategoryStats, ChannelStats, ContentType, NewArticle, NewVideo, SearchResult,
    Stats, VideoDurationRecord, VideoStats, YearStats,
};

/// Manages storage and retrieval of search entries
pub struct Repository {
    pool: Pool<Sqlite>,
}

/// Configuration for search queries, bundling all search parameters
struct SearchConfig<'a> {
    escaped_query: &'a str,
    has_search_terms: bool,
    site_filter: &'a Option<String>,
    start_year: Option<i32>,
    end_year: Option<i32>,
    sort_by: Option<&'a str>,
    offset: u32,
}

impl Repository {
    /// Number of results per page
    pub const RESULTS_PER_PAGE: u32 = 20;

    /// Creates a new repository instance.
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let database_url = format!("sqlite://{}?mode=rwc", path.as_ref().display());
        log::info!("Opening database at: {database_url}");

        let options = SqliteConnectOptions::from_str(&database_url)?.pragma("trusted_schema", "1");

        let pool = SqlitePoolOptions::new()
            .max_connections(20)
            .connect_with(options)
            .await
            .context("Failed to connect to SQLite database")?;

        // Verify trusted_schema is enabled
        let trusted: i32 = sqlx::query_scalar("PRAGMA trusted_schema")
            .fetch_one(&pool)
            .await?;
        log::debug!("trusted_schema = {}", trusted);

        let repo = Self { pool };
        repo.init_db().await?;
        Ok(repo)
    }

    /// Initializes the database schema
    async fn init_db(&self) -> Result<()> {
        sqlx::migrate!("../../migrations").run(&self.pool).await?;

        Ok(())
    }

    /// Inserts a new quote
    pub async fn insert_quote(&self, quote: &types::Quote) -> Result<()> {
        log::debug!("Inserting quote by: {}", quote.author);

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
        log::debug!("Inserting article: {}", article.metadata.url);

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
    pub async fn insert_video(&self, video: &NewVideo) -> Result<i64> {
        log::debug!("Inserting video: {}", video.metadata.url);

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

    /// Checks if a URL already exists in the database (articles or videos)
    pub async fn url_exists(&self, url: &Url) -> Result<bool> {
        let url_str = url.as_str();

        let result = sqlx::query(
            r#"
            SELECT 1 FROM articles WHERE url = ?
            UNION ALL
            SELECT 1 FROM videos WHERE url = ?
            LIMIT 1
            "#,
        )
        .bind(url_str)
        .bind(url_str)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.is_some())
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
            )
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        let total_unique_domains: i64 = unique_domains.get("count");

        let article_stats = self.get_article_stats().await?;
        let video_stats = self.get_video_stats().await?;

        let total_entries = article_stats.total + video_stats.total;

        Ok(Stats {
            total_entries,
            earliest_date,
            latest_date,
            total_unique_domains,
            articles: article_stats,
            videos: video_stats,
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
        top_domains_by_year.sort_by(|a, b| b.year.cmp(&a.year));

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

    /// Parses a search query, extracting site: operators
    fn parse_query(query: &str) -> (Vec<String>, Option<String>) {
        let mut search_terms = Vec::new();
        let mut site_filter = None;

        let mut chars = query.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                ' ' => continue,
                '"' => {
                    // Quoted phrase
                    let mut phrase = String::new();
                    for c in chars.by_ref() {
                        if c == '"' {
                            break;
                        }
                        phrase.push(c);
                    }
                    if !phrase.is_empty() {
                        search_terms.push(phrase);
                    }
                }
                _ => {
                    // Regular word or site: operator
                    let mut word = String::from(c);
                    while let Some(&c) = chars.peek() {
                        if c == ' ' || c == '"' {
                            break;
                        }
                        word.push(chars.next().unwrap());
                    }

                    if let Some(site) = word.strip_prefix("site:") {
                        site_filter = Some(site.to_string());
                    } else if !word.is_empty() {
                        search_terms.push(word);
                    }
                }
            }
        }

        (search_terms, site_filter)
    }

    /// Counts total matching results for a search query (without pagination)
    pub async fn count_search_results(
        &self,
        query: &str,
        start_year: Option<i32>,
        end_year: Option<i32>,
        content_type: ContentType,
    ) -> Result<i64> {
        let (search_terms, site_filter) = Self::parse_query(query);
        let has_search_terms = !search_terms.is_empty();

        let escaped_query = if has_search_terms {
            search_terms
                .iter()
                .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(" AND ")
        } else {
            String::new()
        };

        let count = match content_type {
            ContentType::Articles => {
                self.count_articles(
                    &escaped_query,
                    has_search_terms,
                    &site_filter,
                    start_year,
                    end_year,
                )
                .await?
            }
            ContentType::Video => {
                self.count_videos(
                    &escaped_query,
                    has_search_terms,
                    &site_filter,
                    start_year,
                    end_year,
                )
                .await?
            }
            ContentType::All => {
                let articles = self
                    .count_articles(
                        &escaped_query,
                        has_search_terms,
                        &site_filter,
                        start_year,
                        end_year,
                    )
                    .await?;
                let videos = self
                    .count_videos(
                        &escaped_query,
                        has_search_terms,
                        &site_filter,
                        start_year,
                        end_year,
                    )
                    .await?;
                articles + videos
            }
        };

        Ok(count)
    }

    async fn count_articles(
        &self,
        escaped_query: &str,
        has_search_terms: bool,
        site_filter: &Option<String>,
        start_year: Option<i32>,
        end_year: Option<i32>,
    ) -> Result<i64> {
        let mut query = if has_search_terms {
            let mut q = QueryBuilder::new(
                r#"
                SELECT COUNT(*) as total
                FROM articles_fts
                JOIN articles a ON articles_fts.rowid = a.id
                WHERE articles_fts MATCH "#,
            );
            q.push_bind(escaped_query);
            q
        } else {
            QueryBuilder::new(
                r#"
                SELECT COUNT(*) as total
                FROM articles a
                WHERE 1=1"#,
            )
        };

        if let Some(site) = site_filter {
            query.push(" AND a.url LIKE ");
            query.push_bind(format!("%{site}%"));
        }
        if let Some(start) = start_year {
            query.push(" AND a.date >= ");
            query.push_bind(format!("{start}-01-01"));
        }
        if let Some(end) = end_year {
            query.push(" AND a.date <= ");
            query.push_bind(format!("{end}-12-31"));
        }

        let row = query.build().fetch_one(&self.pool).await?;
        Ok(row.get("total"))
    }

    async fn count_videos(
        &self,
        escaped_query: &str,
        has_search_terms: bool,
        site_filter: &Option<String>,
        start_year: Option<i32>,
        end_year: Option<i32>,
    ) -> Result<i64> {
        let mut query = if has_search_terms {
            let mut q = QueryBuilder::new(
                r#"
                SELECT COUNT(*) as total
                FROM videos_fts
                JOIN videos v ON videos_fts.rowid = v.id
                WHERE videos_fts MATCH "#,
            );
            q.push_bind(escaped_query);
            q
        } else {
            QueryBuilder::new(
                r#"
                SELECT COUNT(*) as total
                FROM videos v
                WHERE 1=1"#,
            )
        };

        if let Some(site) = site_filter {
            query.push(" AND v.url LIKE ");
            query.push_bind(format!("%{site}%"));
        }
        if let Some(start) = start_year {
            query.push(" AND v.date >= ");
            query.push_bind(format!("{start}-01-01"));
        }
        if let Some(end) = end_year {
            query.push(" AND v.date <= ");
            query.push_bind(format!("{end}-12-31"));
        }

        let row = query.build().fetch_one(&self.pool).await?;
        Ok(row.get("total"))
    }

    /// Searches for entries matching the given query
    pub async fn search(
        &self,
        query: &str,
        start_year: Option<i32>,
        end_year: Option<i32>,
        sort_by: Option<&str>,
        content_type: ContentType,
        page: Option<u32>,
    ) -> Result<Vec<SearchResult>> {
        let (search_terms, site_filter) = Self::parse_query(query);
        let has_search_terms = !search_terms.is_empty();

        let escaped_query = if has_search_terms {
            search_terms
                .iter()
                .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(" AND ")
        } else {
            String::new()
        };

        let page_num = page.unwrap_or(1).max(1);
        let offset = (page_num - 1) * Self::RESULTS_PER_PAGE;

        match content_type {
            ContentType::Articles => {
                let config = SearchConfig {
                    escaped_query: &escaped_query,
                    has_search_terms,
                    site_filter: &site_filter,
                    start_year,
                    end_year,
                    sort_by,
                    offset,
                };
                self.search_articles(&config).await
            }
            ContentType::Video => {
                let config = SearchConfig {
                    escaped_query: &escaped_query,
                    has_search_terms,
                    site_filter: &site_filter,
                    start_year,
                    end_year,
                    sort_by,
                    offset,
                };
                self.search_videos(&config).await
            }
            ContentType::All => {
                let config = SearchConfig {
                    escaped_query: &escaped_query,
                    has_search_terms,
                    site_filter: &site_filter,
                    start_year,
                    end_year,
                    sort_by,
                    offset,
                };
                self.search_all(&config).await
            }
        }
    }

    async fn search_articles(&self, config: &SearchConfig<'_>) -> Result<Vec<SearchResult>> {
        let mut query = if config.has_search_terms {
            let mut q = QueryBuilder::new(
                r#"
                SELECT
                    'article' as content_type,
                    a.id, a.title, a.url, a.category, a.date, a.text,
                    a.reference, a.word_count,
                    NULL as thumbnail_url, NULL as duration_seconds,
                    rank,
                    snippet(articles_fts, 2, '<mark>', '</mark>', '...', 50) as snippet
                FROM articles_fts
                JOIN articles a ON articles_fts.rowid = a.id
                WHERE articles_fts MATCH "#,
            );
            q.push_bind(config.escaped_query);
            q
        } else {
            QueryBuilder::new(
                r#"
                SELECT
                    'article' as content_type,
                    a.id, a.title, a.url, a.category, a.date, a.text,
                    a.reference, a.word_count,
                    NULL as thumbnail_url, NULL as duration_seconds,
                    0.0 as rank,
                    NULL as snippet
                FROM articles a
                WHERE 1=1"#,
            )
        };

        if let Some(site) = config.site_filter {
            query.push(" AND a.url LIKE ");
            query.push_bind(format!("%{site}%"));
        }
        if let Some(start) = config.start_year {
            query.push(" AND a.date >= ");
            query.push_bind(format!("{start}-01-01"));
        }
        if let Some(end) = config.end_year {
            query.push(" AND a.date <= ");
            query.push_bind(format!("{end}-12-31"));
        }

        match config.sort_by {
            Some("date-desc") => query.push(" ORDER BY a.date DESC"),
            Some("date-asc") => query.push(" ORDER BY a.date ASC"),
            _ => query.push(" ORDER BY rank"),
        };

        query.push(" LIMIT ");
        query.push_bind(Self::RESULTS_PER_PAGE as i64);
        query.push(" OFFSET ");
        query.push_bind(config.offset as i64);

        let results: Vec<SearchResult> = query.build_query_as().fetch_all(&self.pool).await?;
        Ok(results)
    }

    async fn search_videos(&self, config: &SearchConfig<'_>) -> Result<Vec<SearchResult>> {
        let mut query = if config.has_search_terms {
            let mut q = QueryBuilder::new(
                r#"
                SELECT
                    'video' as content_type,
                    v.id, v.title, v.url, v.category, v.date, v.text,
                    NULL as reference, NULL as word_count,
                    v.thumbnail_url, v.duration_seconds,
                    rank,
                    snippet(videos_fts, 2, '<mark>', '</mark>', '...', 50) as snippet
                FROM videos_fts
                JOIN videos v ON videos_fts.rowid = v.id
                WHERE videos_fts MATCH "#,
            );
            q.push_bind(config.escaped_query);
            q
        } else {
            QueryBuilder::new(
                r#"
                SELECT
                    'video' as content_type,
                    v.id, v.title, v.url, v.category, v.date, v.text,
                    NULL as reference, NULL as word_count,
                    v.thumbnail_url, v.duration_seconds,
                    0.0 as rank,
                    NULL as snippet
                FROM videos v
                WHERE 1=1"#,
            )
        };

        if let Some(site) = config.site_filter {
            query.push(" AND v.url LIKE ");
            query.push_bind(format!("%{site}%"));
        }
        if let Some(start) = config.start_year {
            query.push(" AND v.date >= ");
            query.push_bind(format!("{start}-01-01"));
        }
        if let Some(end) = config.end_year {
            query.push(" AND v.date <= ");
            query.push_bind(format!("{end}-12-31"));
        }

        match config.sort_by {
            Some("date-desc") => query.push(" ORDER BY v.date DESC"),
            Some("date-asc") => query.push(" ORDER BY v.date ASC"),
            _ => query.push(" ORDER BY rank"),
        };

        query.push(" LIMIT ");
        query.push_bind(Self::RESULTS_PER_PAGE as i64);
        query.push(" OFFSET ");
        query.push_bind(config.offset as i64);

        let results: Vec<SearchResult> = query.build_query_as().fetch_all(&self.pool).await?;
        Ok(results)
    }

    /// Searches both articles and videos, merging results by BM25 rank.
    ///
    /// # Performance: Top-N Optimization
    ///
    /// FTS5 has a Top-N optimization: when you `ORDER BY rank LIMIT N`, it doesn't
    /// score every match—it uses the index to find the N best and stops early.
    ///
    /// A naive `UNION ALL` defeats this: SQLite must materialize ALL matches from
    /// both tables, sort them, then take the top N. This is orders of magnitude slower.
    ///
    /// The fix is to "push down" the LIMIT into each subquery:
    /// ```sql
    /// SELECT * FROM (
    ///     SELECT ... FROM articles_fts ... ORDER BY rank LIMIT 20
    ///     UNION ALL
    ///     SELECT ... FROM videos_fts ... ORDER BY rank LIMIT 20
    /// )
    /// ORDER BY rank LIMIT 20
    /// ```
    ///
    /// This way each FTS query uses Top-N optimization (fast), and we only sort
    /// 40 rows instead of potentially thousands.
    async fn search_all(&self, config: &SearchConfig<'_>) -> Result<Vec<SearchResult>> {
        // We need to fetch enough results from each table to satisfy pagination.
        // For page N with 20 results per page, we need offset + limit results.
        let inner_limit = config.offset as i64 + Self::RESULTS_PER_PAGE as i64;

        // Determine sort order for inner queries
        let (inner_order, outer_order) = match config.sort_by {
            Some("date-desc") => ("ORDER BY a.date DESC", "ORDER BY date DESC"),
            Some("date-asc") => ("ORDER BY a.date ASC", "ORDER BY date ASC"),
            _ => ("ORDER BY rank", "ORDER BY rank"),
        };
        let (inner_order_v, _) = match config.sort_by {
            Some("date-desc") => ("ORDER BY v.date DESC", ""),
            Some("date-asc") => ("ORDER BY v.date ASC", ""),
            _ => ("ORDER BY rank", ""),
        };

        let mut query = if config.has_search_terms {
            let mut q = QueryBuilder::new(
                r#"
                SELECT * FROM (
                    SELECT * FROM (
                        SELECT
                            'article' as content_type,
                            a.id, a.title, a.url, a.category, a.date, a.text,
                            a.reference, a.word_count,
                            NULL as thumbnail_url, NULL as duration_seconds,
                            rank,
                            snippet(articles_fts, 2, '<mark>', '</mark>', '...', 50) as snippet
                        FROM articles_fts
                        JOIN articles a ON articles_fts.rowid = a.id
                        WHERE articles_fts MATCH "#,
            );
            q.push_bind(config.escaped_query);

            // Add article filters
            if let Some(site) = config.site_filter {
                q.push(" AND a.url LIKE ");
                q.push_bind(format!("%{site}%"));
            }
            if let Some(start) = config.start_year {
                q.push(" AND a.date >= ");
                q.push_bind(format!("{start}-01-01"));
            }
            if let Some(end) = config.end_year {
                q.push(" AND a.date <= ");
                q.push_bind(format!("{end}-12-31"));
            }

            // Push down ORDER BY and LIMIT for Top-N optimization
            q.push(" ");
            q.push(inner_order);
            q.push(" LIMIT ");
            q.push_bind(inner_limit);

            q.push(
                r#")
                    UNION ALL
                    SELECT * FROM (
                        SELECT
                            'video' as content_type,
                            v.id, v.title, v.url, v.category, v.date, v.text,
                            NULL as reference, NULL as word_count,
                            v.thumbnail_url, v.duration_seconds,
                            rank,
                            snippet(videos_fts, 2, '<mark>', '</mark>', '...', 50) as snippet
                        FROM videos_fts
                        JOIN videos v ON videos_fts.rowid = v.id
                        WHERE videos_fts MATCH "#,
            );
            q.push_bind(config.escaped_query);

            // Add video filters
            if let Some(site) = config.site_filter {
                q.push(" AND v.url LIKE ");
                q.push_bind(format!("%{site}%"));
            }
            if let Some(start) = config.start_year {
                q.push(" AND v.date >= ");
                q.push_bind(format!("{start}-01-01"));
            }
            if let Some(end) = config.end_year {
                q.push(" AND v.date <= ");
                q.push_bind(format!("{end}-12-31"));
            }

            // Push down ORDER BY and LIMIT for Top-N optimization
            q.push(" ");
            q.push(inner_order_v);
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
                            a.id, a.title, a.url, a.category, a.date, a.text,
                            a.reference, a.word_count,
                            NULL as thumbnail_url, NULL as duration_seconds,
                            0.0 as rank,
                            NULL as snippet
                        FROM articles a
                        WHERE 1=1"#,
            );

            if let Some(site) = config.site_filter {
                q.push(" AND a.url LIKE ");
                q.push_bind(format!("%{site}%"));
            }
            if let Some(start) = config.start_year {
                q.push(" AND a.date >= ");
                q.push_bind(format!("{start}-01-01"));
            }
            if let Some(end) = config.end_year {
                q.push(" AND a.date <= ");
                q.push_bind(format!("{end}-12-31"));
            }

            // Push down ORDER BY and LIMIT
            q.push(" ");
            q.push(inner_order);
            q.push(" LIMIT ");
            q.push_bind(inner_limit);

            q.push(
                r#")
                    UNION ALL
                    SELECT * FROM (
                        SELECT
                            'video' as content_type,
                            v.id, v.title, v.url, v.category, v.date, v.text,
                            NULL as reference, NULL as word_count,
                            v.thumbnail_url, v.duration_seconds,
                            0.0 as rank,
                            NULL as snippet
                        FROM videos v
                        WHERE 1=1"#,
            );

            if let Some(site) = config.site_filter {
                q.push(" AND v.url LIKE ");
                q.push_bind(format!("%{site}%"));
            }
            if let Some(start) = config.start_year {
                q.push(" AND v.date >= ");
                q.push_bind(format!("{start}-01-01"));
            }
            if let Some(end) = config.end_year {
                q.push(" AND v.date <= ");
                q.push_bind(format!("{end}-12-31"));
            }

            // Push down ORDER BY and LIMIT
            q.push(" ");
            q.push(inner_order_v);
            q.push(" LIMIT ");
            q.push_bind(inner_limit);

            q.push("))");
            q
        };

        // Outer ORDER BY sorts the combined results from both subqueries
        query.push(" ");
        query.push(outer_order);

        // Final pagination
        query.push(" LIMIT ");
        query.push_bind(Self::RESULTS_PER_PAGE as i64);
        query.push(" OFFSET ");
        query.push_bind(config.offset as i64);

        let results: Vec<SearchResult> = query.build_query_as().fetch_all(&self.pool).await?;
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_query_with_site() {
        let (terms, site) = Repository::parse_query("rust site:github.com");
        assert_eq!(terms, vec!["rust"]);
        assert_eq!(site, Some("github.com".to_string()));
    }

    #[test]
    fn test_parse_query_site_only() {
        let (terms, site) = Repository::parse_query("site:example.com");
        assert!(terms.is_empty());
        assert_eq!(site, Some("example.com".to_string()));
    }

    #[test]
    fn test_parse_query_no_site() {
        let (terms, site) = Repository::parse_query("rust async await");
        assert_eq!(terms, vec!["rust", "async", "await"]);
        assert_eq!(site, None);
    }

    #[test]
    fn test_parse_query_multiple_terms_with_site() {
        let (terms, site) = Repository::parse_query("error handling site:doc.rust-lang.org");
        assert_eq!(terms, vec!["error", "handling"]);
        assert_eq!(site, Some("doc.rust-lang.org".to_string()));
    }
}
