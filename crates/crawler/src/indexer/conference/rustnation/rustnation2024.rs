//! RustNation 2024 schedule parser.
//!
//! TODO: Add actual conference details and parsing logic.
//! Placeholder implementation that returns empty talks list.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::LazyLock;
use types::Url;

use crate::indexer::conference::{ConferenceMetadata, ParsedTalk, ScheduleParser, static_url};

/// Parser for RustNation 2024
pub struct RustNation2024;

static RUSTNATION_2024_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://www.rustnationuk.com/"));

#[async_trait]
impl ScheduleParser for RustNation2024 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustnation-2024",
            conference: "RustNation",
            year: "2024",
            url: (*RUSTNATION_2024_BASE_URL).clone(),
            youtube_playlist_url: None,
        }
    }

    async fn parse(&self, _client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        // TODO: Implement actual parsing logic
        // For now, return empty list
        log::info!("RustNation 2024 parser: No talks available yet (placeholder)");
        Ok(Vec::new())
    }
}
