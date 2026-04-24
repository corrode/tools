//! Indexer for Rust RFCs
//!
//! This fetches RFC files from the rust-lang/rfcs GitHub repository, and
//! extracts metadata such as title, date, and tags from the markdown files.
//!
//! Set the `GITHUB_TOKEN` environment variable to avoid rate limits.

use super::Indexer;
use crate::tools::markdown;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use regex::Regex;
use reqwest::header;
use serde::Deserialize;
use std::sync::LazyLock;
use storage::Repository;
use tracing::{debug, info, warn};
use types::{Metadata, NewArticle, Url};

/// Regex to strip leading RFC number from filename (e.g., "0001-foo" -> "foo")
static RFC_NUMBER_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+-").unwrap());

/// Stats collected during indexing
#[derive(Debug, Default)]
struct RfcStats {
    processed: usize,
    skipped_existing: usize,
    skipped_no_date: usize,
    failed: usize,
}

/// GitHub API URL for the RFCs folder
const GH_RFCS_FOLDER_URL: &str = "https://api.github.com/repos/rust-lang/rfcs/contents/text";

/// GitHub API URL for commits on a specific file
const GH_COMMITS_URL: &str = "https://api.github.com/repos/rust-lang/rfcs/commits";

#[derive(Debug, Deserialize)]
struct GithubFile {
    name: String,
    download_url: Option<String>,
    #[serde(rename = "type")]
    file_type: String,
}

#[derive(Debug, Deserialize)]
struct GithubCommit {
    commit: CommitInfo,
}

#[derive(Debug, Deserialize)]
struct CommitInfo {
    author: CommitAuthor,
}

#[derive(Debug, Deserialize)]
struct CommitAuthor {
    date: DateTime<Utc>,
}

/// Indexer for Rust RFCs
pub struct Rfc {
    client: reqwest::Client,
    dry_run: bool,
    overwrite: bool,
}

impl Default for Rfc {
    fn default() -> Self {
        Self::new()
    }
}

impl Rfc {
    /// Creates a new RFC indexer
    pub fn new() -> Self {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static("corrode/search crawler"),
        );

        // Add GitHub token if available for higher rate limits
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if let Ok(auth_value) = header::HeaderValue::from_str(&format!("Bearer {}", token)) {
                headers.insert(header::AUTHORIZATION, auth_value);
                info!("Using GITHUB_TOKEN for authentication");
            }
        } else {
            warn!("GITHUB_TOKEN not set - GitHub API rate limits will apply");
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to build reqwest client");

        Self {
            client,
            dry_run: false,
            overwrite: false,
        }
    }

    async fn fetch_file_list(&self) -> Result<Vec<GithubFile>> {
        let response = self.client.get(GH_RFCS_FOLDER_URL).send().await?;

        // Check for error status codes before parsing
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            if status.as_u16() == 403 && body.contains("rate limit") {
                anyhow::bail!(
                    "GitHub API rate limit exceeded. Set GITHUB_TOKEN environment variable to increase limits.\nResponse: {}",
                    body
                );
            }
            anyhow::bail!("GitHub API error ({}): {}", status, body);
        }

        let files: Vec<GithubFile> = response
            .json()
            .await
            .context("Failed to parse GitHub API response as file list")?;

        Ok(files
            .into_iter()
            .filter(|f| f.file_type == "file" && f.name.ends_with(".md"))
            .collect())
    }

    async fn fetch_first_commit_date(&self, filename: &str) -> Option<NaiveDate> {
        let url = format!("{}?path=text/{}", GH_COMMITS_URL, filename);
        let response = self.client.get(&url).send().await.ok()?;

        // Check for success before parsing
        if !response.status().is_success() {
            debug!(
                "Failed to fetch commit history for {}: {}",
                filename,
                response.status()
            );
            return None;
        }

        let commits: Vec<GithubCommit> = response.json().await.ok()?;
        // Get the last commit (oldest/first) in the list
        let first_commit = commits.last()?;
        Some(first_commit.commit.author.date.date_naive())
    }

    /// Parses RFC metadata and returns (date, title, content_without_metadata)
    fn parse_metadata(content: &str) -> (Option<NaiveDate>, Option<String>, String) {
        let mut date = None;
        let mut title = None;
        let mut first_header = None;
        let mut content_start_line = 0;

        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            if trimmed.is_empty() {
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("- ") {
                if let Some((key, value)) = rest.split_once(':') {
                    let key = key.trim();
                    let value = value.trim();

                    match key {
                        "Start Date" => {
                            if let Ok(d) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
                                date = Some(d);
                            }
                        }
                        "Feature Name"
                            // Ignore N/A or empty feature names
                            if !value.is_empty()
                                && !value.eq_ignore_ascii_case("n/a")
                                && !value.eq_ignore_ascii_case("none") =>
                        {
                            title = Some(value.to_string());
                        }
                        _ => {}
                    }
                }
            } else if trimmed.starts_with('#') {
                // Any markdown header marks the end of metadata
                content_start_line = line_num;

                // Capture first header as fallback title (strip all leading #'s and whitespace)
                if first_header.is_none() {
                    let header = trimmed.trim_start_matches('#').trim();
                    // Skip generic headers like "Summary"
                    if !header.eq_ignore_ascii_case("summary")
                        && !header.eq_ignore_ascii_case("motivation")
                    {
                        first_header = Some(header.to_string());
                    }
                }
                break;
            }
        }

        // Use first header as fallback if no feature name found
        if title.is_none() {
            title = first_header;
        }

        // Extract content without the metadata header
        let clean_content: String = content
            .lines()
            .skip(content_start_line)
            .collect::<Vec<_>>()
            .join("\n");

        (date, title, clean_content)
    }

    fn clean_title(filename: &str) -> String {
        let name = filename.strip_suffix(".md").unwrap_or(filename);
        // Remove leading numbers and dash: 0001-foo -> foo
        let name = RFC_NUMBER_REGEX.replace(name, "");

        // Replace dashes with spaces
        let name = name.replace('-', " ");

        // Simple capitalization of first letter
        let mut chars = name.chars();
        match chars.next() {
            None => String::new(),
            Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
        }
    }
}

#[async_trait]
impl Indexer for Rfc {
    fn name(&self) -> &'static str {
        "rfc"
    }

    fn set_dry_run(&mut self, value: bool) {
        self.dry_run = value;
    }

    fn set_overwrite(&mut self, value: bool) {
        self.overwrite = value;
    }

    async fn index(&mut self, repo: &Repository) -> Result<()> {
        info!("Fetching RFC list from GitHub...");
        let files = self.fetch_file_list().await?;
        info!("Found {} RFC files.", files.len());

        let mut stats = RfcStats::default();

        for file in files {
            let url_str = format!(
                "https://github.com/rust-lang/rfcs/blob/master/text/{}",
                file.name
            );
            let url = Url::parse(&url_str)?;

            if !self.overwrite && repo.url_exists(&url).await? {
                debug!("Skipping existing RFC: {}", file.name);
                stats.skipped_existing += 1;
                continue;
            }

            if self.dry_run {
                info!("[DRY RUN] Would process: {}", file.name);
                continue;
            }

            let Some(download_url) = file.download_url else {
                warn!("No download URL for {}", file.name);
                continue;
            };

            info!("Processing RFC: {}", file.name);

            let content = match self.client.get(&download_url).send().await {
                Ok(resp) => resp.text().await?,
                Err(e) => {
                    warn!("Failed to download {}: {}", file.name, e);
                    continue;
                }
            };

            let (date_opt, title_opt, clean_content) = Self::parse_metadata(&content);

            // Skip if content is empty (parsing failed or empty file)
            if clean_content.trim().is_empty() {
                warn!("Empty content for RFC {}, skipping", file.name);
                stats.failed += 1;
                continue;
            }

            let mut reference = None;

            if let Some(filename) = file.name.strip_suffix(".md")
                && let Some(first_part) = filename.split('-').next()
                && let Ok(num) = first_part.parse::<u32>()
            {
                reference = Some(format!("RFC #{}", num));
            }

            // Try to get date from document, otherwise fetch from GitHub commit history
            let date = match date_opt {
                Some(d) => d,
                None => {
                    info!(
                        "No date in document, fetching commit date for {}",
                        file.name
                    );
                    match self.fetch_first_commit_date(&file.name).await {
                        Some(d) => d,
                        None => {
                            warn!("Could not determine date for RFC {}, skipping", file.name);
                            stats.skipped_no_date += 1;
                            continue;
                        }
                    }
                }
            };

            let title = title_opt.unwrap_or_else(|| Self::clean_title(&file.name));

            let metadata = Metadata {
                title: title.clone(),
                url: url.clone(),
                category: "RFC".to_string(),
                date,
            };

            let plain_content = markdown::to_plaintext(&clean_content);
            let word_count = plain_content.split_whitespace().count() as i64;

            let article = NewArticle {
                metadata,
                text: plain_content,
                reference,
                word_count,
            };

            if let Err(e) = repo.insert_article(&article).await {
                warn!("Failed to insert RFC {title}: {e:?}");
                stats.failed += 1;
            } else {
                info!("Indexed RFC: {title}");
                stats.processed += 1;
            }
        }

        info!("RFC indexing complete:");
        info!("  Processed: {}", stats.processed);
        info!("  Skipped (existing): {}", stats.skipped_existing);
        info!("  Skipped (no date): {}", stats.skipped_no_date);
        info!("  Failed: {}", stats.failed);

        Ok(())
    }
}
