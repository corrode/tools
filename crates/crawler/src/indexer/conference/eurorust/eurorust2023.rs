//! EuroRust 2023 schedule parser.
//!
//! Brussels, October 12–13 2023. Single-track conference.
//! Schedule is embedded as HTML tables on the main year page.
//! No individual talk pages exist.
//!
//! HTML structure:
//! ```text
//! section.schedule
//!   div.day                  (one per day)
//!     div.content
//!       h3                   "Day 1" / "Day 2"
//!       table
//!         tr
//!           td.time          — time slot
//!           td.title         — talk title text + a.speaker links
//!             "Talk Title"
//!             – (separator)
//!             a.speaker      — speaker name (0..N)
//! ```

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use scraper::Html;
use std::sync::LazyLock;
use tracing::{debug, info};
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{
    ConferenceMetadata, ParsedTalk, ScheduleParser, base_url, static_url,
};
use crate::tools::css::{css, select_text, slugify, text};

use super::{clean_speaker_name, should_skip};

/// Parser for EuroRust 2023
pub struct EuroRust2023;

static EURORUST_2023_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://eurorust.eu/2023"));
static EURORUST_2023_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=pM_c4HNiEB0&list=PLH6-VpZ3SvUUKFSEPEWiHQi4JqebBj9Tq",
    )
});

/// Conference dates for each day.
const DAY1_DATE: (i32, u32, u32) = (2023, 10, 12);
const DAY2_DATE: (i32, u32, u32) = (2023, 10, 13);

#[async_trait]
impl ScheduleParser for EuroRust2023 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "eurorust-2023",
            conference: "EuroRust",
            year: "2023",
            url: (*EURORUST_2023_BASE_URL).clone(),
            youtube_playlist_url: Some((*EURORUST_2023_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let url = &*EURORUST_2023_BASE_URL;
        info!("Fetching schedule from: {}", url);

        let response = client
            .get(url.as_str())
            .send()
            .await
            .context("Failed to fetch EuroRust 2023 page")?;

        if !response.status().is_success() {
            bail!(
                "Failed to fetch EuroRust 2023 page: HTTP {}",
                response.status()
            );
        }

        let html = response
            .text()
            .await
            .context("Failed to read EuroRust 2023 page body")?;

        let document = Html::parse_document(&html);
        self.parse_schedule(&document)
    }
}

impl EuroRust2023 {
    fn parse_schedule(&self, document: &Html) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();
        let base_url = base_url(&self.metadata().url)?;

        let day_selector = css("div.day")?;
        let h3_selector = css("h3")?;
        let tr_selector = css("tr")?;
        let title_td_selector = css("td.title")?;
        let speaker_selector = css("a.speaker")?;

        for day in document.select(&day_selector) {
            // Determine which day this is
            let day_title = select_text(day, &h3_selector).unwrap_or_default();

            let date = if day_title.contains('1') {
                NaiveDate::from_ymd_opt(DAY1_DATE.0, DAY1_DATE.1, DAY1_DATE.2)
            } else {
                NaiveDate::from_ymd_opt(DAY2_DATE.0, DAY2_DATE.1, DAY2_DATE.2)
            }
            .context("Invalid date")?;

            debug!("Parsing EuroRust 2023 {} ({})", day_title, date);

            for row in day.select(&tr_selector) {
                let title_td = match row.select(&title_td_selector).next() {
                    Some(td) => td,
                    None => continue,
                };

                // Extract speaker names from <a class="speaker"> links
                let speakers: Vec<String> = title_td
                    .select(&speaker_selector)
                    .map(|el| clean_speaker_name(&text(el)))
                    .filter(|name| !name.is_empty())
                    .collect();

                // Extract the talk title.
                // The td.title contains: "Talk Title – Speaker Name(s)"
                // We need just the part before the speaker links.
                // Strategy: get the full text, remove speaker names, and trim the
                // separator character.
                let full_text = text(title_td);

                // The title is the text before the first "–" separator, or
                // the full text minus speaker names if no separator is present.
                let title = if let Some(sep_idx) = full_text.find('–') {
                    full_text[..sep_idx].trim().to_string()
                } else {
                    // Fallback: strip speaker names from full text
                    let mut title = full_text.clone();
                    for speaker in &speakers {
                        title = title.replace(speaker, "");
                    }
                    title.trim().trim_end_matches('–').trim().to_string()
                };

                if title.is_empty() || should_skip(&title) {
                    continue;
                }

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
                "No talks found on EuroRust 2023 page. HTML length: {} chars.",
                document.html().len()
            );
        }

        info!("Parsed {} talks from EuroRust 2023 schedule", talks.len());
        Ok(talks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eurorust_2023_metadata() {
        let parser = EuroRust2023;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "eurorust-2023");
        assert_eq!(metadata.conference, "EuroRust");
        assert_eq!(metadata.year, "2023");
        assert_eq!(
            metadata.url,
            Url::parse("https://eurorust.eu/2023").expect("valid EuroRust 2023 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=pM_c4HNiEB0&list=PLH6-VpZ3SvUUKFSEPEWiHQi4JqebBj9Tq"
                )
                .expect("valid EuroRust 2023 playlist URL")
            )
        );
    }
}
