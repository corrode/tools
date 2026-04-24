//! RustConf 2023 schedule parser.
//!
//! Data is sourced from JSON endpoints on the conference site, which is backed
//! by Airtable. There are two endpoints:
//!
//! - `/data/speakers.json` — list of speaker records with names and bios
//! - `/data/events.json`   — list of schedule events, each referencing speaker
//!   IDs from the speakers feed
//!
//! Notable quirks handled here:
//!
//! - The "Closing Keynote" event uses a generic name; the real talk title lives
//!   in the `description` field.
//! - Training-day sessions (paid add-ons) have speakers but are not conference
//!   talks and must be filtered out.
//! - Descriptions may contain HTML markup that must be stripped.
//! - Website URL anchors are built from slugified titles to produce clean links.

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

/// Event names that are generic keynote labels rather than real talk titles.
/// When we encounter one of these we promote the description to the title.
const GENERIC_KEYNOTE_NAMES: &[&str] = &["closing keynote", "opening keynote", "keynote"];

/// Substrings (case-insensitive) that identify paid training sessions which
/// should not appear in the talk index.
const TRAINING_MARKERS: &[&str] = &[
    "fearless concurrency",
    "ultimate rust crash course",
    "training",
];

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

        // Build a map from Airtable record ID → speaker name.
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
            // Skip explicit break/meal/social slots.
            if event.fields.is_break.unwrap_or(false) {
                continue;
            }

            let raw_name = event.fields.name.unwrap_or_default();
            let raw_name = raw_name.trim().to_string();

            if raw_name.is_empty() {
                continue;
            }

            // Skip paid training sessions — they have speakers but are not
            // conference talks.
            let lower_name = raw_name.to_lowercase();
            if TRAINING_MARKERS
                .iter()
                .any(|marker| lower_name.contains(marker))
            {
                debug!("Skipping training session: {}", raw_name);
                continue;
            }

            // Skip sponsored placeholder slots (no real content).
            if lower_name.contains("sponsored session") {
                debug!("Skipping sponsored session: {}", raw_name);
                continue;
            }

            // Skip UnConf / registration / miscellaneous admin entries that
            // have no speaker list anyway, but be explicit about it.
            if lower_name.contains("unconconf")
                || lower_name.contains("unconf")
                || lower_name.contains("registration")
                || lower_name.contains("early registration")
            {
                debug!("Skipping non-talk entry: {}", raw_name);
                continue;
            }

            // Resolve speaker names from the speaker map.
            let speaker_names = event
                .fields
                .speakers
                .unwrap_or_default()
                .into_iter()
                .filter_map(|id| speaker_map.get(&id).cloned())
                .collect::<Vec<_>>();

            if speaker_names.is_empty() {
                debug!("Skipping event with no speakers: {}", raw_name);
                continue;
            }

            // For generic keynote labels the description holds the real title.
            let (title, description_text) = if GENERIC_KEYNOTE_NAMES.contains(&lower_name.as_str())
            {
                // Description is the real title; we have no separate summary.
                let desc = event
                    .fields
                    .description
                    .as_deref()
                    .map(strip_html)
                    .unwrap_or_default();
                let desc = desc.trim().to_string();
                if desc.is_empty() {
                    // Fallback: keep the generic name so we never lose the talk.
                    (raw_name, None)
                } else {
                    (desc, None)
                }
            } else {
                let desc = event
                    .fields
                    .description
                    .as_deref()
                    .map(strip_html)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                (raw_name, desc)
            };

            let summary =
                description_text.unwrap_or_else(|| format!("Talk by {}", speaker_names.join(", ")));

            let date = event
                .fields
                .start_time
                .as_deref()
                .and_then(Self::parse_start_date)
                .unwrap_or(default_date);

            // Build a clean anchor URL from the slugified title so that the
            // link is human-readable (the site uses title-based anchors on its
            // schedule page).
            let slug = super::slugify(&title);
            let website_url = base_url
                .join(&format!("schedule#{}", slug))
                .with_context(|| format!("Invalid URL for event: {}", title))?;

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

/// Strip HTML tags from a string and decode a handful of common entities.
///
/// This is intentionally simple — the descriptions from the Airtable feed
/// contain only basic inline markup (`<em>`, `<strong>`, `<a>`, `<br>`, etc.)
/// so a full HTML parser is not needed.
fn strip_html(input: &str) -> String {
    // Replace block/line-break elements with a space so that adjacent words
    // don't get merged when the tags are removed.
    let s = input
        .replace("<br>", " ")
        .replace("<br/>", " ")
        .replace("<br />", " ")
        .replace("</p>", " ")
        .replace("</li>", " ");

    // Remove all remaining tags.
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    // Decode common HTML entities.
    let result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");

    // Collapse runs of whitespace that were introduced by tag removal.
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Serde types
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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

    #[test]
    fn test_parse_start_date_invalid() {
        assert!(RustConf2023::parse_start_date("not-a-date").is_none());
        assert!(RustConf2023::parse_start_date("").is_none());
    }

    #[test]
    fn test_strip_html_plain_text() {
        assert_eq!(strip_html("Hello, world!"), "Hello, world!");
    }

    #[test]
    fn test_strip_html_removes_tags() {
        assert_eq!(
            strip_html("This is <em>optional paid-addon</em> training."),
            "This is optional paid-addon training."
        );
    }

    #[test]
    fn test_strip_html_br_becomes_space() {
        assert_eq!(strip_html("line one<br>line two"), "line one line two");
        assert_eq!(strip_html("line one<br/>line two"), "line one line two");
    }

    #[test]
    fn test_strip_html_decodes_entities() {
        assert_eq!(strip_html("Rust &amp; Go"), "Rust & Go");
        assert_eq!(strip_html("a &lt; b"), "a < b");
        assert_eq!(strip_html("it&#39;s"), "it's");
    }

    #[test]
    fn test_strip_html_collapses_whitespace() {
        assert_eq!(strip_html("  lots   of   space  "), "lots of space");
    }

    #[test]
    fn test_closing_keynote_title_promotion() {
        let parser = RustConf2023;

        // Simulate the events/speakers that mirror the actual Airtable data.
        let speaker_id = "recXKzmu558g8KTDs".to_string();
        let events = vec![AirtableRecord {
            id: "recO8GdOhCtbvrUCH".to_string(),
            fields: EventFields {
                name: Some("Closing Keynote".to_string()),
                description: Some(
                    "Organizational Boundary Problems: too many cooks or not enough kitchens?"
                        .to_string(),
                ),
                start_time: Some("2023-09-14T16:35:00.000Z".to_string()),
                speakers: Some(vec![speaker_id.clone()]),
                is_break: None,
            },
        }];

        let mut speaker_map = HashMap::new();
        speaker_map.insert(speaker_id, "Elizabeth Ayer".to_string());

        let talks = parser.parse_events(events, &speaker_map).unwrap();
        assert_eq!(talks.len(), 1);
        assert_eq!(
            talks[0].talk.title,
            "Organizational Boundary Problems: too many cooks or not enough kitchens?"
        );
        assert_eq!(talks[0].speakers[0].name, "Elizabeth Ayer");
    }

    #[test]
    fn test_training_sessions_filtered() {
        let parser = RustConf2023;

        let speaker_id = "recSHGzE9AsvcHV5k".to_string();
        let events = vec![
            AirtableRecord {
                id: "recvra89qrydcWxxV".to_string(),
                fields: EventFields {
                    name: Some("Fearless Concurrency with Rust (AM)".to_string()),
                    description: Some("Paid training add-on.".to_string()),
                    start_time: Some("2023-09-12T09:30:00.000Z".to_string()),
                    speakers: Some(vec![speaker_id.clone()]),
                    is_break: None,
                },
            },
            AirtableRecord {
                id: "rec9n5kdM0n9Z5krq".to_string(),
                fields: EventFields {
                    name: Some("Ultimate Rust Crash Course (PM)".to_string()),
                    description: Some("Paid training add-on.".to_string()),
                    start_time: Some("2023-09-12T13:00:00.000Z".to_string()),
                    speakers: Some(vec![speaker_id.clone()]),
                    is_break: None,
                },
            },
        ];

        let mut speaker_map = HashMap::new();
        speaker_map.insert(speaker_id, "Herbert Wolverson".to_string());

        // Both training sessions must be filtered; parse_events should bail
        // because no valid talks remain.
        let result = parser.parse_events(events, &speaker_map);
        assert!(
            result.is_err(),
            "Expected an error when all events are filtered out"
        );
    }

    #[test]
    fn test_breaks_filtered() {
        let parser = RustConf2023;

        let events = vec![AirtableRecord {
            id: "recYyKj0zgP7C0ei8".to_string(),
            fields: EventFields {
                name: Some("Lunch Break".to_string()),
                description: None,
                start_time: Some("2023-09-13T12:30:00.000Z".to_string()),
                speakers: None,
                is_break: Some(true),
            },
        }];

        let result = parser.parse_events(events, &HashMap::new());
        assert!(result.is_err(), "Break-only events should yield no talks");
    }

    #[test]
    fn test_normal_talk_parsed() {
        let parser = RustConf2023;

        let speaker_id = "recFSDHYk3zphgO51".to_string();
        let events = vec![AirtableRecord {
            id: "recGQPbMXWrIJUtdE".to_string(),
            fields: EventFields {
                name: Some("Extending Rust's Effect System".to_string()),
                description: Some(
                    "Effects are notations used on types, functions, and traits.".to_string(),
                ),
                start_time: Some("2023-09-13T14:00:00.000Z".to_string()),
                speakers: Some(vec![speaker_id.clone()]),
                is_break: None,
            },
        }];

        let mut speaker_map = HashMap::new();
        speaker_map.insert(speaker_id, "Yoshua Wuyts".to_string());

        let talks = parser.parse_events(events, &speaker_map).unwrap();
        assert_eq!(talks.len(), 1);

        let talk = &talks[0].talk;
        assert_eq!(talk.title, "Extending Rust's Effect System");
        assert_eq!(
            talk.summary,
            "Effects are notations used on types, functions, and traits."
        );
        assert_eq!(talk.date, NaiveDate::from_ymd_opt(2023, 9, 13).unwrap());
        assert!(
            talk.website_url
                .to_string()
                .ends_with("schedule#extending-rusts-effect-system"),
            "URL should use slugified title anchor, got: {}",
            talk.website_url
        );
        assert_eq!(talks[0].speakers[0].name, "Yoshua Wuyts");
    }

    #[test]
    fn test_multi_speaker_talk() {
        let parser = RustConf2023;

        let sid1 = "recJ7ktfrTfgtS8Sm".to_string();
        let sid2 = "recXHwPZWxRPF2duL".to_string();
        let events = vec![AirtableRecord {
            id: "recqugcgt2ZVRnTz4".to_string(),
            fields: EventFields {
                name: Some(
                    "Implementing a Blazingly Fast Quantum State Simulator in Rust".to_string(),
                ),
                description: Some("We used Rust to implement Spinoza.".to_string()),
                start_time: Some("2023-09-13T14:00:00.000Z".to_string()),
                speakers: Some(vec![sid1.clone(), sid2.clone()]),
                is_break: None,
            },
        }];

        let mut speaker_map = HashMap::new();
        speaker_map.insert(sid1, "Saveliy Yusufov".to_string());
        speaker_map.insert(sid2, "Charlee Stefanski".to_string());

        let talks = parser.parse_events(events, &speaker_map).unwrap();
        assert_eq!(talks.len(), 1);
        assert_eq!(talks[0].speakers.len(), 2);
    }

    #[test]
    fn test_sponsored_session_filtered() {
        let parser = RustConf2023;

        let events = vec![AirtableRecord {
            id: "recFaHA48FLGVAMbK".to_string(),
            fields: EventFields {
                name: Some("Sponsored Session".to_string()),
                description: None,
                start_time: Some("2023-09-14T13:30:00.000Z".to_string()),
                speakers: None,
                is_break: None,
            },
        }];

        let result = parser.parse_events(events, &HashMap::new());
        assert!(result.is_err(), "Sponsored sessions should be filtered out");
    }

    #[test]
    fn test_missing_speaker_in_map_skips_talk() {
        let parser = RustConf2023;

        let events = vec![AirtableRecord {
            id: "recGQPbMXWrIJUtdE".to_string(),
            fields: EventFields {
                name: Some("Extending Rust's Effect System".to_string()),
                description: None,
                start_time: Some("2023-09-13T14:00:00.000Z".to_string()),
                // Speaker ID not present in the speaker map
                speakers: Some(vec!["recUNKNOWN".to_string()]),
                is_break: None,
            },
        }];

        let result = parser.parse_events(events, &HashMap::new());
        assert!(
            result.is_err(),
            "Talk whose speakers cannot be resolved should be skipped"
        );
    }

    #[test]
    fn test_default_date_used_when_start_time_absent() {
        let parser = RustConf2023;

        let speaker_id = "spk1".to_string();
        let events = vec![AirtableRecord {
            id: "evtA".to_string(),
            fields: EventFields {
                name: Some("A Talk Without a Timestamp".to_string()),
                description: None,
                start_time: None,
                speakers: Some(vec![speaker_id.clone()]),
                is_break: None,
            },
        }];

        let mut speaker_map = HashMap::new();
        speaker_map.insert(speaker_id, "Some Speaker".to_string());

        let talks = parser.parse_events(events, &speaker_map).unwrap();
        assert_eq!(
            talks[0].talk.date,
            NaiveDate::from_ymd_opt(2023, 9, 13).unwrap(),
            "Should fall back to the default conference date"
        );
    }
}
