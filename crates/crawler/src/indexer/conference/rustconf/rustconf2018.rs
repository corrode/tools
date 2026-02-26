//! RustConf 2018 schedule parser.
//!
//! The schedule is a static HTML table at `/schedule.html`.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use scraper::{ElementRef, Html, Selector};
use std::sync::LazyLock;
use tracing::{debug, info};
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{
    ConferenceMetadata, ParsedTalk, ScheduleParser, base_url, static_url,
};
use crate::tools::css::{css, select_text, text};

/// Parser for RustConf 2018.
pub struct RustConf2018;

static RUSTCONF_2018_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://2018.rustconf.com"));
static RUSTCONF_2018_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=J9OFQm8Qf1I&list=PL85XCvVPmGQi3tivxDDF1hrT9qr5hdMBZ",
    )
});

#[async_trait]
impl ScheduleParser for RustConf2018 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustconf-2018",
            conference: "RustConf",
            year: "2018",
            url: (*RUSTCONF_2018_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTCONF_2018_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let base_url = base_url(&self.metadata().url)?;
        let schedule_url = base_url
            .join("schedule.html")
            .context("Failed to build schedule URL")?;
        info!("Fetching schedule from: {}", schedule_url);

        let response = client
            .get(schedule_url)
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

        // RustConf 2018 conference day.
        let date = NaiveDate::from_ymd_opt(2018, 8, 17).context("Invalid date")?;

        self.parse_schedule(&document, date, &base_url)
    }
}

impl RustConf2018 {
    fn parse_schedule(
        &self,
        document: &Html,
        date: NaiveDate,
        base_url: &Url,
    ) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        debug!("Parsing RustConf 2018 schedule table");

        let row_selector = css("table#schedule tr")?;
        let cell_selector = css("td")?;
        let speaker_selector = css("p.speaker")?;
        let byline_selector = css("p.byline")?;

        for row in document.select(&row_selector) {
            let cells: Vec<ElementRef> = row.select(&cell_selector).collect();
            if cells.len() < 2 {
                continue;
            }

            for cell in cells.into_iter().skip(1) {
                let Some((title, speakers)) =
                    Self::parse_cell(&cell, &speaker_selector, &byline_selector)
                else {
                    continue;
                };

                let website_url = base_url
                    .join(&format!("schedule.html#{}", super::slugify(&title)))
                    .with_context(|| format!("Invalid URL for talk: {}", title))?;

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

                let speaker_list: Vec<NewSpeaker> = speakers
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
                "No talks found in RustConf 2018 schedule page. HTML length: {} chars.",
                document.html().len()
            );
        }

        info!("Parsed {} talks from schedule", talks.len());
        Ok(talks)
    }

    fn parse_cell(
        cell: &ElementRef,
        speaker_selector: &Selector,
        byline_selector: &Selector,
    ) -> Option<(String, Vec<String>)> {
        let title = select_text(*cell, speaker_selector)?;

        if should_skip_title(&title) {
            return None;
        }

        let byline = cell
            .select(byline_selector)
            .next()
            .map(|el| text(el))
            .unwrap_or_default();

        let speakers = parse_speakers(&byline);
        if speakers.is_empty() {
            return None;
        }

        Some((title, speakers))
    }
}

fn parse_speakers(byline: &str) -> Vec<String> {
    let cleaned = byline
        .replace("by", "")
        .replace(" and ", ",")
        .replace(" & ", ",")
        .replace(";", ",");

    cleaned
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn should_skip_title(title: &str) -> bool {
    let lower = title.to_lowercase();
    lower.contains("registration")
        || lower.contains("break")
        || lower.contains("lunch")
        || lower.contains("closing")
        || lower.contains("opening")
        || lower.contains("welcome")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustconf_2018_metadata() {
        let parser = RustConf2018;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustconf-2018");
        assert_eq!(metadata.conference, "RustConf");
        assert_eq!(metadata.year, "2018");
        assert_eq!(
            metadata.url,
            Url::parse("https://2018.rustconf.com").expect("valid RustConf 2018 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=J9OFQm8Qf1I&list=PL85XCvVPmGQi3tivxDDF1hrT9qr5hdMBZ"
                )
                .expect("valid RustConf 2018 playlist URL")
            )
        );
    }

    #[test]
    fn test_parse_speakers() {
        let speakers = parse_speakers("by Alice, Bob and Carol");
        assert_eq!(speakers, vec!["Alice", "Bob", "Carol"]);
    }
}
