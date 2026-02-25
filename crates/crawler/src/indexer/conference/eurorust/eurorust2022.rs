//! EuroRust 2022 schedule parser.
//!
//! Berlin, October 13–14 2022. Single-track conference.
//! Schedule is embedded as HTML tables on the main year page.
//! No individual talk pages exist.
//!
//! HTML structure:
//! ```text
//! section.schedule
//!   div.timetable           (one per day)
//!     h3.table-title        "Day 1" / "Day 2"
//!     table.schedule-table
//!       tr
//!         td > div.talk-info > p.time
//!         td.schedule-table--timeslot > div.talk
//!           p.talk-title              — talk title
//!           div.speaker > a > p.name  — speaker name (0..N)
//! ```

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use log::{debug, info};
use scraper::Html;
use std::sync::LazyLock;
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{
    ConferenceMetadata, ParsedTalk, ScheduleParser, base_url, static_url,
};
use crate::tools::css::{css, select_text, slugify, text};

use super::{clean_speaker_name, should_skip};

/// Parser for EuroRust 2022
pub struct EuroRust2022;

static EURORUST_2022_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://eurorust.eu/2022"));
static EURORUST_2022_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=8FKXPUkTQLE&list=PLH6-VpZ3SvUXLJ2xKnT1z12pTAUSgMAu7",
    )
});

/// Conference dates for each day.
const DAY1_DATE: (i32, u32, u32) = (2022, 10, 13);
const DAY2_DATE: (i32, u32, u32) = (2022, 10, 14);

#[async_trait]
impl ScheduleParser for EuroRust2022 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "eurorust-2022",
            conference: "EuroRust",
            year: "2022",
            url: (*EURORUST_2022_BASE_URL).clone(),
            youtube_playlist_url: Some((*EURORUST_2022_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let url = &*EURORUST_2022_BASE_URL;
        info!("Fetching schedule from: {}", url);

        let response = client
            .get(url.as_str())
            .send()
            .await
            .context("Failed to fetch EuroRust 2022 page")?;

        if !response.status().is_success() {
            bail!(
                "Failed to fetch EuroRust 2022 page: HTTP {}",
                response.status()
            );
        }

        let html = response
            .text()
            .await
            .context("Failed to read EuroRust 2022 page body")?;

        let document = Html::parse_document(&html);
        self.parse_schedule(&document)
    }
}

impl EuroRust2022 {
    fn parse_schedule(&self, document: &Html) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();
        let base_url = base_url(&self.metadata().url)?;

        let timetable_selector = css("div.timetable")?;
        let table_title_selector = css("h3.table-title")?;
        let tr_selector = css("tr")?;
        let talk_title_selector = css("p.talk-title")?;
        let speaker_name_selector = css("div.speaker p.name")?;

        for timetable in document.select(&timetable_selector) {
            // Determine which day this timetable is for
            let day_title = select_text(timetable, &table_title_selector).unwrap_or_default();

            let date = if day_title.contains("1") {
                NaiveDate::from_ymd_opt(DAY1_DATE.0, DAY1_DATE.1, DAY1_DATE.2)
            } else {
                NaiveDate::from_ymd_opt(DAY2_DATE.0, DAY2_DATE.1, DAY2_DATE.2)
            }
            .context("Invalid date")?;

            debug!("Parsing EuroRust 2022 {} ({})", day_title, date);

            for row in timetable.select(&tr_selector) {
                // Extract talk title
                let title = match select_text(row, &talk_title_selector) {
                    Some(t) => t,
                    None => continue,
                };

                if title.is_empty() || should_skip(&title) {
                    continue;
                }

                // Extract speaker names
                let speakers: Vec<String> = row
                    .select(&speaker_name_selector)
                    .map(|el| clean_speaker_name(&text(el)))
                    .filter(|name| !name.is_empty())
                    .collect();

                // Skip items with no speakers (not a talk)
                if speakers.is_empty() {
                    debug!("Skipping item with no speakers: {}", title);
                    continue;
                }

                let slug = slugify(&title);
                let website_url = base_url
                    .join(&format!("#{}", slug))
                    .with_context(|| format!("Invalid URL for talk: {title}"))?;

                let talk = NewTalk {
                    title: title.clone(),
                    summary: format!("Talk by {}", speakers.join(", ")),
                    transcript: None,
                    conference: self.metadata().conference.to_string(),
                    date,
                    website_url: website_url.into(),
                    video_url: None,
                    slides_url: None,
                    thumbnail_url: None,
                    duration_seconds: None,
                };

                let speaker_list = speakers
                    .into_iter()
                    .map(|name| NewSpeaker { name })
                    .collect();

                talks.push(ParsedTalk {
                    talk,
                    speakers: speaker_list,
                });
            }
        }

        if talks.is_empty() {
            bail!(
                "No talks found on EuroRust 2022 page. HTML length: {} chars.",
                document.html().len()
            );
        }

        info!("Parsed {} talks from EuroRust 2022 schedule", talks.len());
        Ok(talks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eurorust_2022_metadata() {
        let parser = EuroRust2022;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "eurorust-2022");
        assert_eq!(metadata.conference, "EuroRust");
        assert_eq!(metadata.year, "2022");
        assert_eq!(
            metadata.url,
            Url::parse("https://eurorust.eu/2022").expect("valid EuroRust 2022 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=8FKXPUkTQLE&list=PLH6-VpZ3SvUXLJ2xKnT1z12pTAUSgMAu7"
                )
                .expect("valid EuroRust 2022 playlist URL")
            )
        );
    }
}
