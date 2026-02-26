//! RustConf 2023 schedule parser.
//!
//! Data is sourced from JSON endpoints on the conference site.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use std::sync::LazyLock;
use tracing::{debug, info};
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{
    ConferenceMetadata, ParsedTalk, ScheduleParser, base_url, static_url,
};

/// Parser for RustConf 2023
pub struct RustConf2023;

static RUSTCONF_2023_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://rustconf-2023.netlify.app"));
static RUSTCONF_2023_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=MTnIexTt9Dk&list=PL85XCvVPmGQgR1aCC-b0xx7sidGfopjCj",
    )
});

#[async_trait]
impl ScheduleParser for RustConf2023 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustconf-2023",
            conference: "RustConf",
            year: "2023",
            url: (*RUSTCONF_2023_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTCONF_2023_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let base_url = base_url(&self.metadata().url)?;
        let events_url = base_url
            .join("data/events.json")
            .context("Failed to build events URL")?;
        let speakers_url = base_url
            .join("data/speakers.json")
            .context("Failed to build speakers URL")?;

        info!("Fetching events from: {}", events_url);
        info!("Fetching speakers from: {}", speakers_url);

        let events_response = client
            .get(events_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .context("Failed to fetch events JSON")?;

        if !events_response.status().is_success() {
            bail!(
                "Failed to fetch events JSON: HTTP {}",
                events_response.status()
            );
        }

        let speakers_response = client
            .get(speakers_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .context("Failed to fetch speakers JSON")?;

        if !speakers_response.status().is_success() {
            bail!(
                "Failed to fetch speakers JSON: HTTP {}",
                speakers_response.status()
            );
        }

        let events: EventsResponse = events_response
            .json()
            .await
            .context("Failed to parse events JSON")?;

        let speakers: SpeakersResponse = speakers_response
            .json()
            .await
            .context("Failed to parse speakers JSON")?;

        let speaker_map = speakers
            .speakers
            .into_iter()
            .filter_map(|record| record.fields.name.map(|name| (record.id, name)))
            .collect::<HashMap<_, _>>();

        debug!("Resolved {} speakers for RustConf 2023", speaker_map.len());

        self.parse_events(events.events, &speaker_map)
    }
}

impl RustConf2023 {
    fn parse_events(
        &self,
        events: Vec<AirtableRecord<EventFields>>,
        speaker_map: &HashMap<String, String>,
    ) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        let default_date = NaiveDate::from_ymd_opt(2023, 9, 13).context("Invalid default date")?;
        let base_url = base_url(&self.metadata().url)?;

        for event in events {
            if event.fields.is_break.unwrap_or(false) {
                continue;
            }

            let title = event.fields.name.unwrap_or_default().trim().to_string();
            if title.is_empty() {
                continue;
            }

            let speaker_names = event
                .fields
                .speakers
                .unwrap_or_default()
                .into_iter()
                .filter_map(|id| speaker_map.get(&id).cloned())
                .collect::<Vec<_>>();

            if speaker_names.is_empty() {
                debug!("Skipping event with no speakers: {}", title);
                continue;
            }

            let date = event
                .fields
                .start_time
                .as_deref()
                .and_then(Self::parse_start_date)
                .unwrap_or(default_date);

            let summary = event
                .fields
                .description
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("Talk by {}", speaker_names.join(", ")));

            let website_url = base_url
                .join(&format!("schedule#{}", event.id))
                .with_context(|| format!("Invalid URL for event {}", event.id))?;

            let talk = NewTalk {
                title,
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

            let speaker_list = speaker_names
                .into_iter()
                .map(|name| NewSpeaker { name })
                .collect::<Vec<_>>();

            talks.push(ParsedTalk {
                talk,
                speakers: speaker_list,
            });
        }

        if talks.is_empty() {
            bail!("No talks found in RustConf 2023 events data.");
        }

        info!("Parsed {} talks from events data", talks.len());
        Ok(talks)
    }

    fn parse_start_date(value: &str) -> Option<NaiveDate> {
        DateTime::parse_from_rfc3339(value)
            .map(|dt| dt.with_timezone(&Utc).date_naive())
            .ok()
    }
}

#[derive(Debug, Deserialize)]
struct EventsResponse {
    events: Vec<AirtableRecord<EventFields>>,
}

#[derive(Debug, Deserialize)]
struct SpeakersResponse {
    speakers: Vec<AirtableRecord<SpeakerFields>>,
}

#[derive(Debug, Deserialize)]
struct AirtableRecord<T> {
    id: String,
    fields: T,
}

#[derive(Debug, Deserialize)]
struct EventFields {
    #[serde(rename = "name")]
    name: Option<String>,
    #[serde(rename = "description")]
    description: Option<String>,
    #[serde(rename = "startTime")]
    start_time: Option<String>,
    #[serde(rename = "speakers")]
    speakers: Option<Vec<String>>,
    #[serde(rename = "isBreak")]
    is_break: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SpeakerFields {
    #[serde(rename = "name")]
    name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustconf_2023_metadata() {
        let parser = RustConf2023;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustconf-2023");
        assert_eq!(metadata.conference, "RustConf");
        assert_eq!(metadata.year, "2023");
        assert_eq!(
            metadata.url,
            Url::parse("https://rustconf-2023.netlify.app").expect("valid RustConf 2023 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=MTnIexTt9Dk&list=PL85XCvVPmGQgR1aCC-b0xx7sidGfopjCj"
                )
                .expect("valid RustConf 2023 playlist URL")
            )
        );
    }

    #[test]
    fn test_parse_start_date() {
        let date = RustConf2023::parse_start_date("2023-09-13T10:00:00.000Z").unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2023, 9, 13).unwrap());
    }
}
