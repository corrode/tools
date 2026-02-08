//! Indexer for Rust RFCs
//!
//! This fetches RFC files from the rust-lang/rfcs GitHub repository, and
//! extracts metadata such as title, date, and tags from the markdown files.

use super::Indexer;
use anyhow::Result;
use async_trait::async_trait;
use chrono::NaiveDate;
use log::{debug, info, warn};
use regex::Regex;
use reqwest::header;
use serde::Deserialize;
use storage::Repository;
use types::{Entry, EntryId, Url};

/// GitHub API URL for the RFCs folder
const GH_RFCS_FOLDER_URL: &str = "https://api.github.com/repos/rust-lang/rfcs/contents/text";

#[derive(Debug, Deserialize)]
struct GithubFile {
    name: String,
    download_url: Option<String>,
    #[serde(rename = "type")]
    file_type: String,
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
            header::HeaderValue::from_static("corrode-search-crawler"),
        );

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

    /// Set dry run mode
    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    /// Set overwrite mode
    pub fn with_overwrite(mut self, overwrite: bool) -> Self {
        self.overwrite = overwrite;
        self
    }

    async fn fetch_file_list(&self) -> Result<Vec<GithubFile>> {
        let response = self.client.get(GH_RFCS_FOLDER_URL).send().await?;
        let files: Vec<GithubFile> = response.json().await?;
        Ok(files
            .into_iter()
            .filter(|f| f.file_type == "file" && f.name.ends_with(".md"))
            .collect())
    }

    fn parse_metadata(&self, content: &str) -> (Option<NaiveDate>, Option<String>) {
        let mut date = None;
        let mut title = None;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if let Some(rest) = line.strip_prefix("- ") {
                if let Some((key, value)) = rest.split_once(':') {
                    let key = key.trim();
                    let value = value.trim();

                    match key {
                        "Start Date" => {
                            if let Ok(d) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
                                date = Some(d);
                            }
                        }
                        "Feature Name" => {
                            title = Some(value.to_string());
                        }
                        _ => {}
                    }
                }
            } else if line.starts_with('#') {
                // Header found, stop parsing metadata
                break;
            }
        }

        (date, title)
    }

    fn clean_title(&self, filename: &str) -> String {
        let name = filename.strip_suffix(".md").unwrap_or(filename);
        // Remove leading numbers and dash: 0001-foo -> foo
        // Using unwrap is safe because regex is static valid
        let re = Regex::new(r"^\d+-").unwrap();
        let name = re.replace(name, "");

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

        for file in files {
            let url_str = format!(
                "https://github.com/rust-lang/rfcs/blob/master/text/{}",
                file.name
            );
            let url = Url::parse(&url_str)?;

            if !self.overwrite && repo.url_exists(&url).await? {
                debug!("Skipping existing RFC: {}", file.name);
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

            let (date_opt, title_opt) = self.parse_metadata(&content);

            let mut reference = None;

            if let Some(filename) = file.name.strip_suffix(".md")
                && let Some(first_part) = filename.split('-').next()
                && let Ok(num) = first_part.parse::<u32>()
            {
                reference = Some(format!("RFC #{}", num));
            }

            let date = date_opt.unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());

            let title = title_opt.unwrap_or_else(|| self.clean_title(&file.name));

            let entry_id = EntryId {
                title: title.clone(),
                url: url.clone(),
                category: "RFC".to_string(),
                date,
            };

            let entry = Entry {
                id: entry_id,
                text: Some(content),
                thumbnail_url: None,
                reference,
                duration_seconds: None,
            };

            if let Err(e) = repo.insert_entry(&entry).await {
                warn!("Failed to insert RFC {title}: {e}");
            } else {
                info!("Indexed RFC: {title}");
            }
        }

        Ok(())
    }
}
