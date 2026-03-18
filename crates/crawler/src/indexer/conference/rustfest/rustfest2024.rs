//! RustFest 2024 schedule parser.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use scraper::Html;
use std::sync::LazyLock;
use tracing::{debug, info};
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{ConferenceMetadata, ParsedTalk, ScheduleParser, static_url};
use crate::tools::css::{css, text};

/// Parser for RustFest 2024 (Zürich)
pub struct RustFest2024;

static RUSTFEST_2024_BASE_URL: LazyLock<Url> = LazyLock::new(|| static_url("https://rustfest.ch/"));

#[async_trait]
impl ScheduleParser for RustFest2024 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustfest-2024",
            conference: "RustFest",
            year: "2024",
            url: (*RUSTFEST_2024_BASE_URL).clone(),
            youtube_playlist_url: None,
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let base_url = &*RUSTFEST_2024_BASE_URL;
        let schedule_url = base_url
            .join("schedule/")
            .context("Failed to build schedule URL")?;
        info!("Fetching schedule from: {}", schedule_url);

        let response = client
            .get(schedule_url.to_string())
            .send()
            .await
            .context("Failed to fetch schedule page")?;

        if !response.status().is_success() {
            bail!("Failed to fetch schedule page: HTTP {}", response.status());
        }

        let html = response
            .text()
            .await
            .context("Failed to read schedule page body")?;

        let document = Html::parse_document(&html);

        self.parse_schedule(&document, base_url)
    }
}

impl RustFest2024 {
    fn parse_schedule(&self, document: &Html, base_url: &Url) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        let table_selector = css("table.time-schedule")?;
        let tr_selector = css("tbody tr")?;
        let td_selector = css("td")?;
        let a_selector = css("a")?;

        for (i, table) in document.select(&table_selector).enumerate() {
            // Day 1: June 21, Day 2: June 22
            let date = if i == 0 {
                NaiveDate::from_ymd_opt(2024, 6, 21).unwrap()
            } else {
                NaiveDate::from_ymd_opt(2024, 6, 22).unwrap()
            };

            for row in table.select(&tr_selector) {
                let tds: Vec<_> = row.select(&td_selector).collect();
                if tds.len() < 2 {
                    continue;
                }

                let content_td = tds[1];

                if content_td.value().classes().any(|c| c == "joint-event") {
                    continue;
                }

                let mut speakers = Vec::new();
                for a in content_td.select(&a_selector) {
                    let name = text(a).trim().to_string();
                    if !name.is_empty() {
                        speakers.push(NewSpeaker { name });
                    }
                }

                let mut title = text(content_td);
                for s in &speakers {
                    title = title.replace(&s.name, "");
                }
                title = title.trim().to_string();

                if title.is_empty()
                    || title.to_lowercase().contains("lightning talks")
                    || title.to_lowercase().contains("closing words")
                {
                    continue;
                }

                debug!(
                    "Parsed RustFest 2024 talk candidate: {} ({} speakers)",
                    title,
                    speakers.len()
                );

                // Links go to speaker profiles, so we just link the talk to the schedule page.
                let website_url = base_url
                    .join("schedule/")
                    .with_context(|| format!("Invalid URL for talk: {}", title))?;

                let speaker_names = speakers
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");

                let talk = NewTalk {
                    title: title.clone(),
                    summary: format!("Talk by {}", speaker_names),
                    transcript: None,
                    conference: self.metadata().conference.to_string(),
                    date,
                    website_url: website_url.into(),
                    video_url: None,
                    slides_url: None,
                    thumbnail_url: None,
                    duration_seconds: None,
                };

                talks.push(ParsedTalk { talk, speakers });
            }
        }

        if talks.is_empty() {
            bail!(
                "No talks found in RustFest 2024 schedule page. HTML length: {} chars.",
                document.html().len()
            );
        }

        info!("Parsed {} talks from schedule", talks.len());
        Ok(talks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustfest_2024_metadata() {
        let parser = RustFest2024;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustfest-2024");
        assert_eq!(metadata.conference, "RustFest");
        assert_eq!(metadata.year, "2024");
        assert_eq!(
            metadata.url,
            Url::parse("https://rustfest.ch/").expect("valid RustFest 2024 base URL")
        );
        assert_eq!(metadata.youtube_playlist_url, None);
    }
}
