//! FOSDEM 2024 Rust devroom schedule parser.
//!
//! TODO: Add actual conference details and parsing logic.
//! Placeholder implementation that returns empty talks list.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::LazyLock;
use types::Url;

use crate::indexer::conference::{ConferenceMetadata, ParsedTalk, ScheduleParser, static_url};

/// Parser for FOSDEM 2024 Rust devroom
pub struct FOSDEM2024;

static FOSDEM_2024_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://fosdem.org/2024/schedule/track/rust/"));

#[async_trait]
impl ScheduleParser for FOSDEM2024 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "fosdem-2024",
            conference: "FOSDEM",
            year: "2024",
            url: (*FOSDEM_2024_BASE_URL).clone(),
            youtube_playlist_url: None,
        }
    }

    async fn parse(&self, _client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        // TODO: Implement actual parsing logic
        // For now, return empty list
        log::info!("FOSDEM 2024 Rust devroom parser: No talks available yet (placeholder)");
        Ok(Vec::new())
    }
}
