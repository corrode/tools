//! RustConf 2020 schedule parser.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate};
use serde_json::Value;
use std::sync::LazyLock;
use tracing::{debug, info};
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{
    ConferenceMetadata, ParsedTalk, ScheduleParser, base_url, static_url,
};

/// Parser for RustConf 2020
pub struct RustConf2020;

static RUSTCONF_2020_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://2020.rustconf.com"));
static RUSTCONF_2020_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=IwPRu5FhfIQ&list=PL85XCvVPmGQijqvMcMBfYAwExx1eBu1Ei",
    )
});

#[async_trait]
impl ScheduleParser for RustConf2020 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustconf-2020",
            conference: "RustConf",
            year: "2020",
            url: (*RUSTCONF_2020_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTCONF_2020_PLAYLIST_URL).clone()),
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
        debug!("RustConf 2020 endpoints ready");

        let events: Value = client
            .get(events_url)
            .send()
            .await
            .context("Failed to fetch events data")?
            .json()
            .await
            .context("Failed to parse events JSON")?;

        let speakers: Value = client
            .get(speakers_url)
            .send()
            .await
            .context("Failed to fetch speakers data")?
            .json()
            .await
            .context("Failed to parse speakers JSON")?;

        self.parse_events(&events, &speakers)
    }
}

impl RustConf2020 {
    fn parse_events(&self, events: &Value, speakers: &Value) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        let speaker_map = self.build_speaker_map(speakers)?;
        let event_list = events
            .get("events")
            .and_then(Value::as_array)
            .context("Missing events array")?;

        let base_url = base_url(&self.metadata().url)?;

        for event in event_list {
            let fields = match event.get("fields") {
                Some(Value::Object(map)) => map,
                _ => continue,
            };

            let title = fields
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("");

            if title.is_empty() {
                continue;
            }

            if self.should_skip_title(title) {
                continue;
            }

            let description = fields
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("");

            let start_time = fields
                .get("startTime")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .context("Missing startTime for event")?;

            let date = self
                .parse_date(start_time)
                .with_context(|| format!("Invalid startTime for event: {}", title))?;

            let speaker_names = self.resolve_speakers(fields, &speaker_map);

            if speaker_names.is_empty() {
                debug!("Skipping event without speakers: {}", title);
                continue;
            }

            let summary = if description.is_empty() {
                format!("Talk by {}", speaker_names.join(", "))
            } else {
                description.to_string()
            };

            let slug = Self::slugify(title);
            let website_url = base_url
                .join(&format!("schedule#{}", slug))
                .with_context(|| format!("Invalid URL for talk: {}", title))?;
            let video_url = fields
                .get("youtubeId")
                .and_then(Value::as_str)
                .map(|id| format!("https://www.youtube.com/watch?v={id}"));

            let talk = NewTalk {
                title: title.to_string(),
                summary,
                transcript: None,
                conference: self.metadata().conference.to_string(),
                date,
                website_url: website_url.into(),
                video_url,
                slides_url: None,
                thumbnail_url: None,
                duration_seconds: None,
            };

            let speaker_list: Vec<NewSpeaker> = speaker_names
                .into_iter()
                .map(|name| NewSpeaker { name })
                .collect();

            talks.push(ParsedTalk {
                talk,
                speakers: speaker_list,
            });
        }

        if talks.is_empty() {
            bail!("No talks found in RustConf 2020 events data.");
        }

        info!("Parsed {} talks from events", talks.len());
        Ok(talks)
    }

    fn build_speaker_map(
        &self,
        speakers: &Value,
    ) -> Result<std::collections::HashMap<String, String>> {
        let mut map = std::collections::HashMap::new();

        let speaker_list = speakers
            .get("speakers")
            .and_then(Value::as_array)
            .context("Missing speakers array")?;

        for speaker in speaker_list {
            let id = speaker
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string);

            let name = speaker
                .get("fields")
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);

            if let (Some(id), Some(name)) = (id, name) {
                map.insert(id, name);
            }
        }

        Ok(map)
    }

    fn resolve_speakers(
        &self,
        fields: &serde_json::Map<String, Value>,
        speaker_map: &std::collections::HashMap<String, String>,
    ) -> Vec<String> {
        let speaker_ids = fields
            .get("speakers")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        speaker_ids
            .into_iter()
            .filter_map(|id| id.as_str().and_then(|key| speaker_map.get(key)).cloned())
            .collect()
    }

    fn parse_date(&self, start_time: &str) -> Result<NaiveDate> {
        let parsed = DateTime::parse_from_rfc3339(start_time)
            .with_context(|| format!("Failed to parse RFC3339 date: {}", start_time))?;
        Ok(parsed.date_naive())
    }

    fn should_skip_title(&self, title: &str) -> bool {
        let lower = title.to_lowercase();
        lower.contains("break")
            || lower.contains("lunch")
            || lower.contains("reception")
            || lower.contains("registration")
            || lower.contains("closing remarks")
            || lower.contains("opening remarks")
            || lower.contains("welcome")
    }

    fn slugify(title: &str) -> String {
        let mut slug = String::new();
        let mut last_was_dash = false;

        for ch in title.to_lowercase().chars() {
            if ch.is_ascii_alphanumeric() {
                slug.push(ch);
                last_was_dash = false;
            } else if (ch.is_whitespace() || ch == '-') && !last_was_dash && !slug.is_empty() {
                slug.push('-');
                last_was_dash = true;
            }
        }

        while slug.ends_with('-') {
            slug.pop();
        }

        if slug.is_empty() {
            "talk".to_string()
        } else {
            slug
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustconf_2020_metadata() {
        let parser = RustConf2020;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustconf-2020");
        assert_eq!(metadata.conference, "RustConf");
        assert_eq!(metadata.year, "2020");
        assert_eq!(
            metadata.url,
            Url::parse("https://2020.rustconf.com").expect("valid RustConf 2020 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=IwPRu5FhfIQ&list=PL85XCvVPmGQijqvMcMBfYAwExx1eBu1Ei"
                )
                .expect("valid RustConf 2020 playlist URL")
            )
        );
    }
}
