//! Oxidize 2025 schedule parser.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use scraper::Html;
use std::sync::LazyLock;
use tracing::{debug, info};
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{ConferenceMetadata, ParsedTalk, ScheduleParser, static_url};
use crate::tools::css::{css, text};

/// Parser for Oxidize 2025
pub struct Oxidize2025;

static OXIDIZE_2025_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://oxidizeconf.com/"));
static OXIDIZE_2025_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url("https://www.youtube.com/playlist?list=PLilpJp3WAOvcn5_VDv3VIkQzniMWl_BfO")
});

#[async_trait]
impl ScheduleParser for Oxidize2025 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "oxidize-2025",
            conference: "Oxidize",
            year: "2025",
            url: (*OXIDIZE_2025_BASE_URL).clone(),
            youtube_playlist_url: Some((*OXIDIZE_2025_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        info!("Fetching schedule from: {}", self.metadata().url);

        let response = client
            .get(self.metadata().url.to_string())
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

        // Oxidize 2025 main conference day assumption
        let date = NaiveDate::from_ymd_opt(2025, 9, 17).context("Invalid date")?;

        self.parse_schedule(&document, date)
    }
}

impl Oxidize2025 {
    fn parse_schedule(&self, document: &Html, date: NaiveDate) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        let session_selector = css("a.session")?;
        let summary_selector = css(".session_summary p")?;
        let person_selector = css(".session_person")?;
        let person_no_pic_selector = css(".session_person-no-pic div:first-child")?;

        let base_url = &*OXIDIZE_2025_BASE_URL;

        for session in document.select(&session_selector) {
            let href = session.value().attr("href").unwrap_or("");
            if href.is_empty() {
                continue;
            }

            let website_url = base_url
                .join(href)
                .with_context(|| format!("Invalid URL for talk: {}", href))?;

            // Extract talk summary/title
            let title = session
                .select(&summary_selector)
                .next()
                .map(|el| text(el))
                .unwrap_or_default()
                .replace('\n', " ")
                .trim()
                .to_string();

            if title.is_empty() {
                debug!("Skipping session with empty title at {}", href);
                continue;
            }

            let mut speakers = Vec::new();

            // Extract speakers with pictures
            for el in session.select(&person_selector) {
                let name = text(el).trim().to_string();
                if !name.is_empty() {
                    speakers.push(NewSpeaker { name });
                }
            }

            // Extract speakers without pictures
            for el in session.select(&person_no_pic_selector) {
                let name = text(el).trim().to_string();
                if !name.is_empty() {
                    speakers.push(NewSpeaker { name });
                }
            }

            if speakers.is_empty() {
                debug!("Skipping session with no speakers: {}", title);
                continue;
            }

            debug!(
                "Parsed Oxidize 2025 talk candidate: {} ({} speakers)",
                title,
                speakers.len()
            );

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

        if talks.is_empty() {
            bail!(
                "No talks found in schedule page. HTML length: {} chars. \
                 The page structure may have changed.",
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
    fn test_oxidize_2025_metadata() {
        let parser = Oxidize2025;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "oxidize-2025");
        assert_eq!(metadata.conference, "Oxidize");
        assert_eq!(metadata.year, "2025");
        assert_eq!(
            metadata.url,
            Url::parse("https://oxidizeconf.com/").expect("valid Oxidize 2025 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/playlist?list=PLilpJp3WAOvcn5_VDv3VIkQzniMWl_BfO"
                )
                .expect("valid Oxidize 2025 playlist URL")
            )
        );
    }
}
