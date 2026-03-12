//! RustLab 2026 schedule parser.

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::NaiveDate;
use scraper::Html;
use std::sync::LazyLock;
use tracing::{debug, info};
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{ConferenceMetadata, ParsedTalk, ScheduleParser, static_url};
use crate::tools::css::css;

/// Parser for RustLab 2026
pub struct RustLab2026;

static RUSTLAB_2026_BASE_URL: LazyLock<Url> = LazyLock::new(|| static_url("https://rustlab.it/"));
static RUSTLAB_2026_PLAYLIST_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://www.youtube.com/@rustlabconference3671"));

#[async_trait]
impl ScheduleParser for RustLab2026 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustlab-2026",
            conference: "RustLab",
            year: "2026",
            url: (*RUSTLAB_2026_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTLAB_2026_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let base_url = &*RUSTLAB_2026_BASE_URL;
        let schedule_url = base_url
            .join("schedule")
            .context("Failed to build schedule URL")?;
        info!("Fetching schedule from: {}", schedule_url);

        let response = client
            .get(schedule_url.to_string())
            .send()
            .await
            .context("Failed to fetch schedule page")?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch schedule page: HTTP {}", response.status());
        }

        let html = response
            .text()
            .await
            .context("Failed to read schedule page body")?;

        let document = Html::parse_document(&html);
        self.parse_schedule(&document, base_url)
    }
}

impl RustLab2026 {
    fn parse_schedule(&self, document: &Html, base_url: &Url) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        let next_data_selector = css("script#__NEXT_DATA__")?;
        let next_data = document
            .select(&next_data_selector)
            .next()
            .context("Could not find __NEXT_DATA__ script")?;

        let json_text = next_data.inner_html();
        let data: serde_json::Value = serde_json::from_str(&json_text)?;

        let days = data
            .pointer("/props/pageProps/edition/days")
            .and_then(|v| v.as_array())
            .context("Could not find schedule days in JSON data")?;

        for day in days {
            let date_str = day.get("date").and_then(|v| v.as_str()).unwrap_or("");

            let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .unwrap_or_else(|_| NaiveDate::from_ymd_opt(2026, 11, 1).unwrap());

            if let Some(schedule) = day.get("schedule").and_then(|v| v.as_array()) {
                for event in schedule {
                    let title = event
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();

                    let event_type = event
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();

                    if title.is_empty()
                        || event_type.to_lowercase() == "break"
                        || title.to_lowercase().contains("registration")
                    {
                        continue;
                    }

                    let description = event
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim()
                        // Basic tag stripping
                        .replace("<p>", "")
                        .replace("</p>", "\n")
                        .trim()
                        .to_string();

                    let mut speakers = Vec::new();
                    if let Some(speakers_array) = event.get("speakers").and_then(|v| v.as_array()) {
                        for speaker_obj in speakers_array {
                            if let Some(name) = speaker_obj.get("name").and_then(|v| v.as_str()) {
                                let name = name.trim().to_string();
                                if !name.is_empty() {
                                    speakers.push(NewSpeaker { name });
                                }
                            }
                        }
                    }

                    let summary = if description.is_empty() {
                        let speaker_names = speakers
                            .iter()
                            .map(|s| s.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        if !speaker_names.is_empty() {
                            format!("Talk by {}", speaker_names)
                        } else {
                            title.clone()
                        }
                    } else {
                        description
                    };

                    let website_url = base_url
                        .join("schedule")
                        .with_context(|| format!("Invalid URL for talk: {}", title))?;

                    let talk = NewTalk {
                        title: title.clone(),
                        summary,
                        transcript: None,
                        conference: self.metadata().conference.to_string(),
                        date,
                        website_url: website_url.into(),
                        video_url: None,
                        slides_url: None,
                        thumbnail_url: None,
                        duration_seconds: None,
                    };

                    debug!(
                        "Parsed RustLab 2026 talk: {} ({} speakers)",
                        title,
                        speakers.len()
                    );
                    talks.push(ParsedTalk { talk, speakers });
                }
            }
        }

        if talks.is_empty() {
            tracing::warn!(
                "No talks found in RustLab 2026 schedule data yet (might be too early for the schedule to be released)."
            );
        } else {
            info!("Parsed {} talks from schedule", talks.len());
        }

        Ok(talks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustlab_2026_metadata() {
        let parser = RustLab2026;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustlab-2026");
        assert_eq!(metadata.conference, "RustLab");
        assert_eq!(metadata.year, "2026");
        assert_eq!(
            metadata.url,
            Url::parse("https://rustlab.it/").expect("valid RustLab 2026 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse("https://www.youtube.com/@rustlabconference3671")
                    .expect("valid RustLab 2026 playlist URL")
            )
        );
    }
}
