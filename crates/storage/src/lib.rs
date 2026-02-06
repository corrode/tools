#![deny(missing_docs)]
#![deny(rustdoc::missing_crate_level_docs)]

//! # Rust Search Storage
//!
//! This crate provides a repository for storing and retrieving TWiR entries.
//! It uses SQLite as the underlying storage engine.
//!
//! Articles can be searched through a full-text search index based on the FTS5
//! extension provided by SQLite.

use anyhow::Context;
use anyhow::Result;
use chrono::NaiveDate;
use sqlx::{Pool, QueryBuilder, Row, Sqlite, sqlite::SqlitePoolOptions};
use std::path::Path;
use types::{CategoryStats, ContentType, Entry, EntryId, SearchResult, Stats, YearStats};

/// Manages storage and retrieval of TWiR entries
pub struct Repository {
    pool: Pool<Sqlite>,
}

impl Repository {
    /// Number of results per page
    pub const RESULTS_PER_PAGE: u32 = 20;

    /// Creates a new repository instance.
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let database_url = format!("sqlite://{}?mode=rwc", path.as_ref().display());
        log::info!("Opening database at: {database_url}");

        let pool = SqlitePoolOptions::new()
            .max_connections(20)
            .connect(&database_url)
            .await
            .context("Failed to connect to SQLite database")?;

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

    /// Retrieves a random quote from the database
    pub async fn get_random_quote(&self) -> Result<Option<types::Quote>> {
        use sqlx::Row;
        let quote = sqlx::query(
            r#"
            SELECT text, author, url, date
            FROM quotes
            ORDER BY RANDOM()
            LIMIT 1
            "#,
        )
        .map(|row: sqlx::sqlite::SqliteRow| {
            let url_str: Option<String> = row.get("url");
            let url = url_str.and_then(|u| url::Url::parse(&u).ok());
            types::Quote {
                text: row.get("text"),
                author: row.get("author"),
                url,
                date: row.get("date"),
            }
        })
        .fetch_optional(&self.pool)
        .await?;

        Ok(quote)
    }

    /// Inserts a new entry
    pub async fn insert_entry(&self, entry: &Entry) -> Result<()> {
        log::debug!("Inserting entry: {}", entry.id.url);
        let mut tx = self.pool.begin().await?;

        let date_str = entry.id.date.format("%Y-%m-%d").to_string();
        let url_str = entry.id.url.as_str();

        let entry_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO entries_meta(title, url, category, date, text, entry_type, thumbnail_url, reference)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(url) DO UPDATE SET
                title = excluded.title,
                category = excluded.category,
                date = excluded.date,
                text = excluded.text,
                thumbnail_url = excluded.thumbnail_url,
                reference = excluded.reference
            RETURNING id
            "#,
        )
        .bind(&entry.id.title)
        .bind(url_str)
        .bind(&entry.id.category)
        .bind(&date_str)
        .bind(entry.text.as_deref().unwrap_or(""))
        .bind("article")
        .bind(&entry.thumbnail_url)
        .bind(&entry.reference)
        .fetch_one(&mut *tx)
        .await?;

        // Note: entry_id is used by the query above, keeping the variable binding
        let _ = entry_id;

        tx.commit().await?;
        Ok(())
    }

    /// Checks if a URL already exists in the database
    pub async fn url_exists(&self, url: &url::Url) -> Result<bool> {
        let url_str = url.as_str();
        let result = sqlx::query("SELECT 1 FROM entries_meta WHERE url = ? LIMIT 1")
            .bind(url_str)
            .fetch_optional(&self.pool)
            .await?;

        Ok(result.is_some())
    }

    /// Gets the latest entry date from the database
    pub async fn get_latest_entry_date(&self) -> Result<Option<NaiveDate>> {
        let result = sqlx::query("SELECT MAX(date) as latest_date FROM entries_meta")
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
        // Get total articles and total characters
        let overview = sqlx::query(
            "SELECT COUNT(*) as total, SUM(LENGTH(text)) as total_chars, AVG(LENGTH(text)) as avg_size FROM entries_meta"
        )
        .fetch_one(&self.pool)
        .await?;

        let total_articles: i64 = overview.get("total");
        let total_characters: i64 = overview.get::<Option<i64>, _>("total_chars").unwrap_or(0);
        let avg_article_size: i64 =
            overview.get::<Option<f64>, _>("avg_size").unwrap_or(0.0) as i64;

        // Get category stats
        let category_rows = sqlx::query(
            "SELECT category, COUNT(*) as count FROM entries_meta GROUP BY category ORDER BY count DESC"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut categories: Vec<CategoryStats> = category_rows
            .into_iter()
            .map(|row| CategoryStats {
                category: row.get("category"),
                count: row.get("count"),
                percentage: 0, // Will calculate below
            })
            .collect();

        // Calculate percentages for categories
        let max_category_count = categories.iter().map(|c| c.count).max().unwrap_or(1);
        for category in &mut categories {
            category.percentage = (category.count * 100) / max_category_count;
        }

        // Get articles per year
        let year_rows = sqlx::query(
            r#"
            SELECT
                CAST(strftime('%Y', date) AS INTEGER) as year,
                COUNT(*) as count
            FROM entries_meta
            GROUP BY year
            ORDER BY year DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut articles_per_year: Vec<YearStats> = year_rows
            .into_iter()
            .map(|row| YearStats {
                year: row.get("year"),
                count: row.get("count"),
                percentage: 0, // Will calculate below
            })
            .collect();

        // Calculate percentages for years
        let max_year_count = articles_per_year.iter().map(|y| y.count).max().unwrap_or(1);
        for year in &mut articles_per_year {
            year.percentage = (year.count * 100) / max_year_count;
        }

        // Get articles per month
        let month_rows = sqlx::query(
            r#"
            SELECT
                strftime('%Y-%m', date) as year_month,
                CAST(strftime('%Y', date) AS INTEGER) as year,
                CAST(strftime('%m', date) AS INTEGER) as month,
                COUNT(*) as count
            FROM entries_meta
            GROUP BY year_month
            ORDER BY year_month ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut articles_per_month: Vec<types::MonthStats> = month_rows
            .into_iter()
            .map(|row| types::MonthStats {
                year_month: row.get("year_month"),
                year: row.get("year"),
                month: row.get("month"),
                count: row.get("count"),
                percentage: 0, // Will calculate below
            })
            .collect();

        // Calculate percentages for months
        let max_month_count = articles_per_month
            .iter()
            .map(|m| m.count)
            .max()
            .unwrap_or(1);
        for month in &mut articles_per_month {
            month.percentage = (month.count * 100) / max_month_count;
        }

        // Get earliest and latest dates
        let earliest_date = sqlx::query("SELECT MIN(date) as earliest FROM entries_meta")
            .fetch_one(&self.pool)
            .await?
            .get::<Option<String>, _>("earliest")
            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok());

        let latest_date = sqlx::query("SELECT MAX(date) as latest FROM entries_meta")
            .fetch_one(&self.pool)
            .await?
            .get::<Option<String>, _>("latest")
            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok());

        // Get top domains by year (top 5 per year)
        let domain_rows = sqlx::query(
            r#"
            WITH domain_counts AS (
                SELECT
                    CAST(strftime('%Y', date) AS INTEGER) as year,
                    CASE
                        WHEN url LIKE 'http://%' THEN substr(url, 8, instr(substr(url, 8), '/') - 1)
                        WHEN url LIKE 'https://%' THEN substr(url, 9, instr(substr(url, 9), '/') - 1)
                        ELSE url
                    END as domain,
                    COUNT(*) as count
                FROM entries_meta
                GROUP BY year, domain
            )
            SELECT year, domain, count,
                   ROW_NUMBER() OVER (PARTITION BY year ORDER BY count DESC) as rank
            FROM domain_counts
            WHERE domain != ''
            ORDER BY year DESC, count DESC
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let mut top_domains_by_year: Vec<types::YearlyDomainStats> = Vec::new();
        let mut current_year: Option<i32> = None;
        let mut current_domains: Vec<types::DomainStats> = Vec::new();

        for row in domain_rows {
            let year: i32 = row.get("year");
            let rank: i64 = row.get("rank");

            if rank > 10 {
                continue; // Only top 10 per year
            }

            if current_year != Some(year) {
                if let Some(y) = current_year {
                    top_domains_by_year.push(types::YearlyDomainStats {
                        year: y,
                        domains: current_domains.clone(),
                    });
                    current_domains.clear();
                }
                current_year = Some(year);
            }

            current_domains.push(types::DomainStats {
                domain: row.get("domain"),
                count: row.get("count"),
            });
        }

        if let Some(y) = current_year {
            top_domains_by_year.push(types::YearlyDomainStats {
                year: y,
                domains: current_domains,
            });
        }

        // Get top domain overall
        let top_domain_overall = sqlx::query(
            r#"
            SELECT
                CASE
                    WHEN url LIKE 'http://%' THEN substr(url, 8, instr(substr(url, 8), '/') - 1)
                    WHEN url LIKE 'https://%' THEN substr(url, 9, instr(substr(url, 9), '/') - 1)
                    ELSE url
                END as domain,
                COUNT(*) as count
            FROM entries_meta
            GROUP BY domain
            ORDER BY count DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?
        .map(|row| types::DomainStats {
            domain: row.get("domain"),
            count: row.get("count"),
        });

        // Get total unique domains
        let total_unique_domains = sqlx::query(
            r#"
            SELECT COUNT(DISTINCT
                CASE
                    WHEN url LIKE 'http://%' THEN substr(url, 8, instr(substr(url, 8), '/') - 1)
                    WHEN url LIKE 'https://%' THEN substr(url, 9, instr(substr(url, 9), '/') - 1)
                    ELSE url
                END
            ) as count
            FROM entries_meta
            "#,
        )
        .fetch_one(&self.pool)
        .await?
        .get("count");

        // Get top keywords by year (top 10 per year from titles and text)
        // Clean all special characters that could break JSON parsing
        let keyword_rows = sqlx::query(
            r#"
            WITH yearly_words AS (
                SELECT
                    CAST(strftime('%Y', date) AS INTEGER) as year,
                    lower(
                        replace(
                            replace(
                                replace(
                                    replace(
                                        replace(
                                            replace(
                                                replace(
                                                    replace(
                                                        replace(title, '"', ' '),
                                                    "'", ' '),
                                                '\', ' '),
                                            '[', ' '),
                                        ']', ' '),
                                    '(', ' '),
                                ',', ' '),
                            '.', ' '),
                        ':', ' ')
                    ) as text
                FROM entries_meta
            ),
            split_words AS (
                SELECT year, value as word
                FROM yearly_words, json_each('["' || replace(text, ' ', '","') || '"]')
                WHERE length(value) >= 4
                    -- Filter out common English stopwords
                    AND value NOT IN ('rust', 'this', 'week', 'with', 'from', 'that', 'have', 'for', 'and', 'the', 'are', 'was', 'but', 'not', 'you', 'all', 'can', 'her', 'has', 'had', 'when', 'your', 'about', 'which', 'their', 'will', 'said', 'each', 'tell', 'does', 'these', 'been', 'what', 'some', 'than', 'them', 'would', 'into', 'time', 'could', 'other', 'more', 'very', 'also', 'only', 'well', 'just', 'where', 'most', 'after', 'back', 'good', 'much', 'work', 'over', 'such', 'even', 'take', 'make', 'know', 'here', 'there', 'being', 'because', 'should', 'through', 'before', 'between', 'under', 'while', 'those', 'both')
                    -- Filter out years
                    AND value NOT IN ('2013', '2014', '2015', '2016', 'rust2017', '2018', '2019', '2020', '2021', '2022', '2023', '2024', '2025', '2026')
                    -- Filter out generic programming/blog terms
                    AND value NOT IN ('part', 'code', 'coding', 'build', 'building', 'using', 'engineer', 'software', 'programming', 'writing', 'release', 'released', 'announcing', 'month', 'weekly', 'episode', 'episodes', 'weeks', 'video', 'videos', 'meetup', 'meetups', 'docs', 'documentation', 'changelog', 'series', 'tutorial', 'guide', 'post', 'blog', 'article', 'notes', 'update', 'updates', 'introducing', 'announcement', 'first', 'second', 'third', 'rustacean', 'rustaceans', 'edition', 'version', 'report', 'status', 'call', 'call-for-participation', 'berlin', 'london', 'tokyo', 'year', 'years', 'simple', 'project', 'projects', 'session', 'sessions', 'learning', 'issue', 'issues', 'interview', 'interviews', 'handling', 'workshop', 'workshops', 'user', 'users', 'type', 'types', 'steps', 'january', 'february', 'march', 'april', 'june', 'july', 'august', 'september', 'october', 'november', 'december')
                    -- Filter out Rust-specific common terms and possessives
                    AND value NOT IN ('rust)', 'rust-analyzer', 'rust-lang', 'rustc', 'cargo', 'crate', 'crates', 'rustconf')
                    -- Filter out fragments with special characters
                    AND word NOT LIKE '%?%'
                    AND word NOT LIKE '%!%'
                    AND word NOT LIKE '%/%'
                    AND word NOT LIKE '%#%'
                    -- Filter out common non-English words
                    AND value NOT IN ('setmanal', 'sessió', 'codificació', 'diseño', 'código')
            ),
            word_counts AS (
                SELECT year, word, COUNT(*) as count
                FROM split_words
                GROUP BY year, word
            )
            SELECT year, word, count,
                   ROW_NUMBER() OVER (PARTITION BY year ORDER BY count DESC) as rank
            FROM word_counts
            ORDER BY year DESC, count DESC
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let mut top_keywords_by_year: Vec<types::YearlyKeywordStats> = Vec::new();
        let mut current_kw_year: Option<i32> = None;
        let mut current_keywords: Vec<types::KeywordStats> = Vec::new();

        for row in keyword_rows {
            let year: i32 = row.get("year");
            let rank: i64 = row.get("rank");

            if rank > 10 {
                continue; // Only top 10 per year
            }

            if current_kw_year != Some(year) {
                if let Some(y) = current_kw_year {
                    top_keywords_by_year.push(types::YearlyKeywordStats {
                        year: y,
                        keywords: current_keywords.clone(),
                    });
                    current_keywords.clear();
                }
                current_kw_year = Some(year);
            }

            current_keywords.push(types::KeywordStats {
                keyword: row.get("word"),
                count: row.get("count"),
            });
        }

        if let Some(y) = current_kw_year {
            top_keywords_by_year.push(types::YearlyKeywordStats {
                year: y,
                keywords: current_keywords,
            });
        }

        Ok(Stats {
            total_articles,
            avg_article_size,
            total_characters,
            categories,
            articles_per_year,
            articles_per_month,
            earliest_date,
            latest_date,
            top_domains_by_year,
            top_keywords_by_year,
            total_unique_domains,
            top_domain_overall,
        })
    }

    /// Parses search query to extract operators like site:
    /// Returns (search_terms, site_filter)
    /// Quoted phrases are kept as single terms
    fn parse_query(query: &str) -> (Vec<String>, Option<String>) {
        let mut search_terms = Vec::new();
        let mut site_filter = None;
        let mut in_quotes = false;
        let mut current_word = String::new();

        for ch in query.chars() {
            match ch {
                '"' => {
                    in_quotes = !in_quotes;
                    if !in_quotes && !current_word.is_empty() {
                        // End of quoted phrase
                        search_terms.push(current_word.clone());
                        current_word.clear();
                    }
                }
                ' ' if !in_quotes => {
                    if !current_word.is_empty() {
                        // Check for site: operator
                        if let Some(site) = current_word.strip_prefix("site:") {
                            site_filter = Some(site.to_string());
                        } else {
                            search_terms.push(current_word.clone());
                        }
                        current_word.clear();
                    }
                }
                _ => {
                    current_word.push(ch);
                }
            }
        }

        // Handle remaining word
        if !current_word.is_empty() {
            if let Some(site) = current_word.strip_prefix("site:") {
                site_filter = Some(site.to_string());
            } else {
                search_terms.push(current_word);
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
        // Parse query for site: operator
        let (search_terms, site_filter) = Self::parse_query(query);

        // Handle the case where we have no search terms (only site: filter)
        let has_search_terms = !search_terms.is_empty();

        // Escape query for FTS5 (needs to be outside the if block for lifetime)
        // Each term is already parsed (quoted phrases are single terms)
        let escaped_query = if has_search_terms {
            search_terms
                .iter()
                .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(" AND ")
        } else {
            String::new()
        };

        let mut query = if has_search_terms {
            let mut q = QueryBuilder::new(
                r#"
            SELECT COUNT(*) as total
            FROM entries_fts
            JOIN entries_meta m ON entries_fts.rowid = m.id
            WHERE entries_fts MATCH "#,
            );
            q.push_bind(&escaped_query);
            q
        } else {
            // No search terms, just site filter - don't use FTS5 MATCH
            QueryBuilder::new(
                r#"
            SELECT COUNT(*) as total
            FROM entries_meta m
            WHERE 1=1"#,
            )
        };

        // Add site filter if present
        if let Some(site) = site_filter {
            query.push(" AND m.url LIKE ");
            query.push_bind(format!("%{site}%"));
        }

        // Add date range filter
        if let Some(start) = start_year {
            query.push(" AND m.date >= ");
            query.push_bind(format!("{start}-01-01"));
        }
        if let Some(end) = end_year {
            query.push(" AND m.date <= ");
            query.push_bind(format!("{end}-12-31"));
        }

        // Add content type filter
        match content_type {
            ContentType::All => {}
            ContentType::Articles => {
                query.push(" AND m.url NOT LIKE '%youtube.com%' AND m.url NOT LIKE '%youtu.be%'");
            }
            ContentType::Video => {
                query.push(" AND (m.url LIKE '%youtube.com%' OR m.url LIKE '%youtu.be%')");
            }
        }

        let row = query.build().fetch_one(&self.pool).await?;
        Ok(row.get("total"))
    }

    /// Performs a full-text search on entries
    pub async fn search(
        &self,
        query: &str,
        start_year: Option<i32>,
        end_year: Option<i32>,
        sort_by: Option<&str>,
        content_type: ContentType,
        page: Option<u32>,
    ) -> Result<Vec<SearchResult>> {
        // Parse query for site: operator
        let (search_terms, site_filter) = Self::parse_query(query);

        // Build query using QueryBuilder for safe SQL construction
        // Handle the case where we have no search terms (only site: filter)
        let has_search_terms = !search_terms.is_empty();

        // Escape query for FTS5 (needs to be outside the if block for lifetime)
        // Each term is already parsed (quoted phrases are single terms)
        // Join with AND to match all terms
        // Reference: https://sqlite.org/fts5.html (see "FTS5 Strings" section)
        let escaped_query = if has_search_terms {
            search_terms
                .iter()
                .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(" AND ")
        } else {
            String::new()
        };

        let mut query = if has_search_terms {
            let mut q = QueryBuilder::new(
                r#"
            SELECT
                m.title, m.url, m.category, m.date, m.text, m.thumbnail_url, m.reference,
                rank,
                snippet(entries_fts, -1, '<mark>', '</mark>', '...', 50) as snippet
            FROM entries_fts
            JOIN entries_meta m ON entries_fts.rowid = m.id
            WHERE entries_fts MATCH "#,
            );
            q.push_bind(&escaped_query);
            q
        } else {
            // No search terms, just site filter - don't use FTS5 MATCH
            QueryBuilder::new(
                r#"
            SELECT
                m.title, m.url, m.category, m.date, m.text, m.thumbnail_url, m.reference,
                0.0 as rank,
                NULL as snippet
            FROM entries_meta m
            WHERE 1=1"#,
            )
        };

        // Add site filter if present
        if let Some(site) = site_filter {
            query.push(" AND m.url LIKE ");
            query.push_bind(format!("%{site}%"));
        }

        // Add date range filter
        if let Some(start) = start_year {
            query.push(" AND m.date >= ");
            query.push_bind(format!("{start}-01-01"));
        }
        if let Some(end) = end_year {
            query.push(" AND m.date <= ");
            query.push_bind(format!("{end}-12-31"));
        }

        // Add content type filter
        match content_type {
            ContentType::All => {}
            ContentType::Articles => {
                query.push(" AND m.url NOT LIKE '%youtube.com%' AND m.url NOT LIKE '%youtu.be%'");
            }
            ContentType::Video => {
                query.push(" AND (m.url LIKE '%youtube.com%' OR m.url LIKE '%youtu.be%')");
            }
        }

        // Add ORDER BY clause
        match sort_by {
            Some("date-desc") => query.push(" ORDER BY m.date DESC"),
            Some("date-asc") => query.push(" ORDER BY m.date ASC"),
            _ => query.push(" ORDER BY rank"),
        };

        // Add pagination
        let page_num = page.unwrap_or(1).max(1);
        let offset = (page_num - 1) * Self::RESULTS_PER_PAGE;

        query.push(" LIMIT ");
        query.push_bind(Self::RESULTS_PER_PAGE as i64);
        query.push(" OFFSET ");
        query.push_bind(offset as i64);

        let rows = query.build().fetch_all(&self.pool).await?;

        let results = rows
            .into_iter()
            .filter_map(|row| {
                url::Url::parse(row.get("url")).ok().map(|url| {
                    let date = row.get::<String, _>("date");
                    SearchResult {
                        entry: Entry {
                            thumbnail_url: row.try_get("thumbnail_url").ok(),
                            reference: row.try_get("reference").ok(),
                            id: EntryId {
                                title: row.get("title"),
                                url,
                                category: row.get("category"),
                                date: NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                                    .unwrap_or_default(),
                            },
                            text: row.get("text"),
                        },
                        rank: row.get("rank"),
                        snippet: row.get("snippet"),
                    }
                })
            })
            .collect();

        Ok(results)
    }

    /// Retrieves entries for a specific date
    pub async fn get_entries_by_date(&self, date: NaiveDate) -> Result<Vec<Entry>> {
        let date_str = date.format("%Y-%m-%d").to_string();

        let rows = sqlx::query(
            r#"
            SELECT
                title,
                url,
                category,
                date,
                text,
                thumbnail_url,
                reference
            FROM entries_meta
            WHERE date = ?
            ORDER BY category, title
            "#,
        )
        .bind(&date_str)
        .fetch_all(&self.pool)
        .await?;

        let entries = rows
            .into_iter()
            .filter_map(|row| {
                let Ok(date) = NaiveDate::parse_from_str(row.get::<&str, _>("date"), "%Y-%m-%d")
                else {
                    log::info!(
                        "Cannot convert row date to NaiveDate: {}",
                        row.get::<&str, _>("date")
                    );
                    return None;
                };

                url::Url::parse(row.get("url")).ok().map(|url| Entry {
                    thumbnail_url: row.try_get("thumbnail_url").ok(),
                    reference: row.try_get("reference").ok(),
                    id: EntryId {
                        title: row.get("title"),
                        url,
                        category: row.get("category"),
                        date,
                    },
                    text: row.get("text"),
                })
            })
            .collect();

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_query_with_site() {
        let (terms, site) = Repository::parse_query("linux site:corrode.dev");
        assert_eq!(terms, vec!["linux"]);
        assert_eq!(site, Some("corrode.dev".to_string()));
    }

    #[test]
    fn test_parse_query_site_only() {
        let (terms, site) = Repository::parse_query("site:example.com");
        assert_eq!(terms, Vec::<String>::new());
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
        let (terms, site) = Repository::parse_query("embedded systems site:rust-lang.org");
        assert_eq!(terms, vec!["embedded", "systems"]);
        assert_eq!(site, Some("rust-lang.org".to_string()));
    }
}
