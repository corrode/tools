//! RustNation 2026 schedule parser.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use log::{debug, info};
use scraper::Html;
use std::sync::LazyLock;
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{ConferenceMetadata, ParsedTalk, ScheduleParser, static_url};
use crate::tools::css::css;

/// Parser for RustNation 2026
pub struct RustNation2026;

static RUSTNATION_2026_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://www.rustnationuk.com/"));
static RUSTNATION_2026_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url("https://www.youtube.com/playlist?list=PL1AoGvxomykSUNbvPx4lEKvw4WbbcOtnf")
});

#[async_trait]
impl ScheduleParser for RustNation2026 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustnation-2026",
            conference: "RustNation",
            year: "2026",
            url: (*RUSTNATION_2026_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTNATION_2026_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let base_url = &*RUSTNATION_2026_BASE_URL;
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

impl RustNation2026 {
    fn parse_schedule(&self, document: &Html, base_url: &Url) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        let next_data_selector = css("script#__NEXT_DATA__")?;
        let next_data = document
            .select(&next_data_selector)
            .next()
            .context("Could not find __NEXT_DATA__ script")?;

        let json_text = next_data.inner_html();
        let data: serde_json::Value = serde_json::from_str(&json_text)?;

        let schedule = data
            .pointer("/props/pageProps/schedule")
            .and_then(|v| v.as_array())
            .context("Could not find schedule in JSON data")?;

        for day in schedule {
            let day_name = day
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();

            // February 18-19, 2026
            let date = if day_name.contains("19") {
                NaiveDate::from_ymd_opt(2026, 2, 19).unwrap()
            } else {
                NaiveDate::from_ymd_opt(2026, 2, 18).unwrap()
            };

            if let Some(slots) = day.get("slots").and_then(|v| v.as_array()) {
                for slot in slots {
                    let slottype = slot.get("slottype").and_then(|v| v.as_str()).unwrap_or("");

                    if slottype == "Break" || slottype.is_empty() {
                        continue;
                    }

                    if let Some(tracks) = slot.get("tracks").and_then(|v| v.as_array()) {
                        for track_obj in tracks {
                            if let Some(track) = track_obj.get("track") {
                                let title = track
                                    .get("title")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .trim()
                                    .to_string();

                                if title.is_empty()
                                    || title.to_lowercase().contains("registration")
                                    || title.to_lowercase().contains("refreshment")
                                {
                                    continue;
                                }

                                let description = track
                                    .get("description")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .trim()
                                    // Basic tag stripping
                                    .replace("<p>", "")
                                    .replace("</p>", "\n")
                                    .trim()
                                    .to_string();

                                let speaker_name = track
                                    .get("speaker")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .trim()
                                    .to_string();

                                let mut speakers = Vec::new();
                                if !speaker_name.is_empty() {
                                    speakers.push(NewSpeaker {
                                        name: speaker_name.clone(),
                                    });
                                }

                                // Fallback description if empty
                                let summary = if description.is_empty() {
                                    if !speakers.is_empty() {
                                        format!("Talk by {}", speaker_name)
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
                                    "Parsed RustNation 2026 talk: {} ({} speakers)",
                                    title,
                                    speakers.len()
                                );
                                talks.push(ParsedTalk { talk, speakers });
                            }
                        }
                    }
                }
            }
        }

        if talks.is_empty() {
            bail!("No talks found in JSON schedule data.");
        }

        info!("Parsed {} talks from schedule", talks.len());
        Ok(talks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustnation_2026_metadata() {
        let parser = RustNation2026;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustnation-2026");
        assert_eq!(metadata.conference, "RustNation");
        assert_eq!(metadata.year, "2026");
        assert_eq!(
            metadata.url,
            Url::parse("https://www.rustnationuk.com/").expect("valid RustNation 2026 base URL")
        );
    }
}
