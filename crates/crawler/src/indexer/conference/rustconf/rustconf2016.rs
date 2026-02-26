//! RustConf 2016 schedule parser.
//!
//! The 2016 site exposes an HTML schedule table at `/schedule.html`.

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
use crate::tools::css::{css, select_text, text};

/// Parser for RustConf 2016
pub struct RustConf2016;

static RUSTCONF_2016_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("http://2016.rustconf.com"));
static RUSTCONF_2016_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=pTQxHIzGqFI&list=PLE7tQUdRKcybLShxegjn0xyTTDJeYwEkI",
    )
});

#[async_trait]
impl ScheduleParser for RustConf2016 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustconf-2016",
            conference: "RustConf",
            year: "2016",
            url: (*RUSTCONF_2016_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTCONF_2016_PLAYLIST_URL).clone()),
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

        // RustConf 2016 main conference day
        let date = NaiveDate::from_ymd_opt(2016, 9, 10).context("Invalid date")?;

        self.parse_schedule(&document, date, &base_url)
    }
}

impl RustConf2016 {
    fn parse_schedule(
        &self,
        document: &Html,
        date: NaiveDate,
        base_url: &Url,
    ) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        debug!("Parsing RustConf 2016 schedule table");

        let row_selector = css("table#schedule tbody tr")?;
        let cell_selector = css("td")?;
        let speaker_selector = css("p.speaker")?;
        let byline_selector = css("p.byline")?;

        for row in document.select(&row_selector) {
            let cells: Vec<_> = row.select(&cell_selector).collect();
            if cells.len() < 2 {
                continue;
            }

            // First cell is time; remaining are sessions (possibly 1 or 2 columns)
            for cell in cells.iter().skip(1) {
                let title = match select_text(*cell, &speaker_selector) {
                    Some(t) => t,
                    None => continue,
                };

                if self.is_break_item(&title) {
                    continue;
                }

                let speakers = cell
                    .select(&byline_selector)
                    .next()
                    .map(|el| self.parse_speakers_from_byline(&text(el)))
                    .unwrap_or_default();

                if speakers.is_empty() {
                    debug!("Skipping item with no speakers: {}", title);
                    continue;
                }

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

                let speaker_list = speakers
                    .into_iter()
                    .map(|name| NewSpeaker { name })
                    .collect::<Vec<_>>();

                talks.push(ParsedTalk {
                    talk,
                    speakers: speaker_list,
                });
            }
        }

        if talks.is_empty() {
            bail!(
                "No talks found in RustConf 2016 schedule page. HTML length: {} chars.",
                document.html().len()
            );
        }

        info!("Parsed {} talks from schedule", talks.len());
        Ok(talks)
    }

    fn parse_speakers_from_byline(&self, byline: &str) -> Vec<String> {
        let cleaned = byline.replace("by", "").replace(['\n', '\t'], " ");

        cleaned
            .split(',')
            .flat_map(|part| part.split(" and "))
            .flat_map(|part| part.split('&'))
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>()
    }

    fn is_break_item(&self, title: &str) -> bool {
        let lower = title.to_lowercase();
        lower.contains("registration")
            || lower.contains("break")
            || lower.contains("lunch")
            || lower.contains("snack")
            || lower.contains("coffee")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustconf_2016_metadata() {
        let parser = RustConf2016;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustconf-2016");
        assert_eq!(metadata.conference, "RustConf");
        assert_eq!(metadata.year, "2016");
        assert_eq!(
            metadata.url,
            Url::parse("http://2016.rustconf.com").expect("valid RustConf 2016 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=pTQxHIzGqFI&list=PLE7tQUdRKcybLShxegjn0xyTTDJeYwEkI"
                )
                .expect("valid RustConf 2016 playlist URL")
            )
        );
    }
}
