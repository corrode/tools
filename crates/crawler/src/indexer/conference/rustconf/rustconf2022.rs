//! RustConf 2022 schedule parser.
//!
//! This edition exposes a JSON data feed at `/data/speakers.json`.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use serde::Deserialize;
use std::sync::LazyLock;
use tracing::{debug, info};
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{
    ConferenceMetadata, ParsedTalk, ScheduleParser, base_url, static_url,
};

/// Parser for RustConf 2022
pub struct RustConf2022;

static RUSTCONF_2022_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://2022.rustconf.com"));
static RUSTCONF_2022_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=37yASSgrdGE&list=PL85XCvVPmGQhXeH3QiYct6eMLH1un1dcu",
    )
});

#[async_trait]
impl ScheduleParser for RustConf2022 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustconf-2022",
            conference: "RustConf",
            year: "2022",
            url: (*RUSTCONF_2022_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTCONF_2022_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let base_url = base_url(&self.metadata().url)?;
        let data_url = base_url
            .join("data/speakers.json")
            .context("Failed to build speakers data URL")?;
        info!("Fetching speakers data from: {}", data_url);

        let response = client
            .get(data_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .context("Failed to fetch speakers data")?;

        if !response.status().is_success() {
            bail!("Failed to fetch speakers data: HTTP {}", response.status());
        }

        let body = response
            .text()
            .await
            .context("Failed to read response body")?;
        let payload: SpeakersPayload =
            serde_json::from_str(&body).context("Failed to parse speakers JSON")?;

        // RustConf 2022 main conference day
        let date = NaiveDate::from_ymd_opt(2022, 8, 5).context("Invalid date")?;

        self.parse_speakers(payload, date)
    }
}

impl RustConf2022 {
    fn parse_speakers(&self, payload: SpeakersPayload, date: NaiveDate) -> Result<Vec<ParsedTalk>> {
        let mut talks_by_title: HashMap<String, TalkAccumulator> = HashMap::new();
        let base_url = base_url(&self.metadata().url)?;

        for speaker in payload.speakers {
            let fields = speaker.fields;
            let title = match fields.session_title.as_ref() {
                Some(title) if !title.trim().is_empty() => title.trim().to_string(),
                _ => {
                    debug!("Skipping speaker with missing session title");
                    continue;
                }
            };

            let speaker_name = match fields.name.as_ref() {
                Some(name) if !name.trim().is_empty() => name.trim().to_string(),
                _ => {
                    debug!("Skipping speaker with missing name");
                    continue;
                }
            };

            let entry = talks_by_title
                .entry(title.clone())
                .or_insert_with(|| TalkAccumulator {
                    title: title.clone(),
                    summary: fields
                        .abstract_text
                        .as_ref()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                    speakers: Vec::new(),
                });

            if entry.summary.is_none()
                && let Some(summary) = fields
                    .abstract_text
                    .as_ref()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            {
                entry.summary = Some(summary);
            }

            entry.speakers.push(speaker_name);
        }

        let mut talks = Vec::new();

        for (_, acc) in talks_by_title {
            let summary = acc
                .summary
                .unwrap_or_else(|| format!("Talk by {}", acc.speakers.join(", ")));

            let website_url = base_url
                .join(&format!("schedule#{}", super::slugify(&acc.title)))
                .with_context(|| format!("Invalid URL for talk: {}", acc.title))?;

            let talk = NewTalk {
                title: acc.title.clone(),
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

            let speaker_list: Vec<NewSpeaker> = acc
                .speakers
                .into_iter()
                .map(|name| NewSpeaker { name })
                .collect();

            talks.push(ParsedTalk {
                talk,
                speakers: speaker_list,
            });
        }

        if talks.is_empty() {
            bail!("No talks found in speakers data");
        }

        info!("Parsed {} talks from speakers data", talks.len());
        Ok(talks)
    }
}

#[derive(Debug, Deserialize)]
struct SpeakersPayload {
    speakers: Vec<SpeakerRecord>,
}

#[derive(Debug, Deserialize)]
struct SpeakerRecord {
    fields: SpeakerFields,
}

#[derive(Debug, Deserialize)]
struct SpeakerFields {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "Session Title")]
    session_title: Option<String>,
    #[serde(rename = "Abstract")]
    abstract_text: Option<String>,
}

struct TalkAccumulator {
    title: String,
    summary: Option<String>,
    speakers: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustconf_2022_metadata() {
        let parser = RustConf2022;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustconf-2022");
        assert_eq!(metadata.conference, "RustConf");
        assert_eq!(metadata.year, "2022");
        assert_eq!(
            metadata.url,
            Url::parse("https://2022.rustconf.com").expect("valid RustConf 2022 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=37yASSgrdGE&list=PL85XCvVPmGQhXeH3QiYct6eMLH1un1dcu"
                )
                .expect("valid RustConf 2022 playlist URL")
            )
        );
    }
}
