//! RustConf 2021 schedule parser.
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

/// Parser for RustConf 2021
pub struct RustConf2021;

static RUSTCONF_2021_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://2021.rustconf.com"));
static RUSTCONF_2021_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=ylOpCXI2EMM&list=PL85XCvVPmGQgACNMZlhlRZ4zlKZG_iWH5",
    )
});

#[async_trait]
impl ScheduleParser for RustConf2021 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustconf-2021",
            conference: "RustConf",
            year: "2021",
            url: (*RUSTCONF_2021_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTCONF_2021_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let metadata = self.metadata();
        let base_url = base_url(&metadata.url)?;
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

        self.parse_events(events.events, &speaker_map)
    }
}

impl RustConf2021 {
    fn parse_events(
        &self,
        events: Vec<AirtableRecord<EventFields>>,
        speaker_map: &HashMap<String, String>,
    ) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        // Default date for RustConf 2021 (virtual conference)
        let default_date = NaiveDate::from_ymd_opt(2021, 9, 14).context("Invalid default date")?;
        let base_url = base_url(&self.metadata().url)?;
        debug!("RustConf 2021: parsing {} events", events.len());

        for event in events {
            let name = event.fields.name.unwrap_or_default().trim().to_string();
            if name.is_empty() {
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
                debug!("Skipping event with no speakers: {}", name);
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
            let video_url = event
                .fields
                .youtube_id
                .as_deref()
                .map(|id| format!("https://www.youtube.com/watch?v={id}"));

            let talk = NewTalk {
                title: name,
                summary,
                transcript: None,
                conference: self.metadata().conference.to_string(),
                date,
                website_url: website_url.into(),
                video_url,
                slides_url: event.fields.slides_url.clone(),
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
            bail!("No talks found in RustConf 2021 events data.");
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
    #[serde(rename = "youtubeId")]
    youtube_id: Option<String>,
    #[serde(rename = "slidesURL")]
    slides_url: Option<String>,

    #[serde(rename = "speakers")]
    speakers: Option<Vec<String>>,
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
    fn test_parse_start_date() {
        let date = RustConf2021::parse_start_date("2021-09-14T09:30:00.000Z").unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2021, 9, 14).unwrap());
    }
}
