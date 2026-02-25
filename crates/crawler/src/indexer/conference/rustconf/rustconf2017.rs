//! RustConf 2017 schedule parser.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use log::{debug, info};
use scraper::{ElementRef, Html};
use std::sync::LazyLock;
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{
    ConferenceMetadata, ParsedTalk, ScheduleParser, base_url, static_url,
};
use crate::tools::css::{css, select_text, text};

/// Parser for RustConf 2017
pub struct RustConf2017;

static RUSTCONF_2017_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://2017.rustconf.com"));
static RUSTCONF_2017_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=COrl851gMTY&list=PL85XCvVPmGQhUSX_QBkxb4g1-o56cCqI9",
    )
});

#[async_trait]
impl ScheduleParser for RustConf2017 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustconf-2017",
            conference: "RustConf",
            year: "2017",
            url: (*RUSTCONF_2017_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTCONF_2017_PLAYLIST_URL).clone()),
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

        // RustConf 2017 conference day: August 19
        let date = NaiveDate::from_ymd_opt(2017, 8, 19).context("Invalid date")?;

        self.parse_schedule(&document, date, &base_url)
    }
}

impl RustConf2017 {
    fn parse_schedule(
        &self,
        document: &Html,
        date: NaiveDate,
        base_url: &Url,
    ) -> Result<Vec<ParsedTalk>> {
        debug!("Parsing RustConf 2017 schedule entries");
        let mut talks = Vec::new();

        let speaker_selector = css("p.speaker")?;
        let byline_selector = css("p.byline")?;

        for speaker_el in document.select(&speaker_selector) {
            let title = text(speaker_el);
            if title.is_empty() {
                continue;
            }

            let byline = speaker_el
                .ancestors()
                .filter_map(ElementRef::wrap)
                .find(|el| el.value().name() == "td")
                .and_then(|td| select_text(td, &byline_selector))
                .unwrap_or_default();

            let speakers = parse_speakers(&byline);
            if speakers.is_empty() {
                continue;
            }

            let summary = format!("Talk by {}", speakers.join(", "));
            let slug = super::slugify(&title);
            let website_url = base_url
                .join(&format!("schedule.html#{}", slug))
                .with_context(|| format!("Invalid URL for talk: {}", title))?;

            let talk = NewTalk {
                title: title.clone(),
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

            let speaker_list = speakers
                .into_iter()
                .map(|name| NewSpeaker { name })
                .collect::<Vec<_>>();

            talks.push(ParsedTalk {
                talk,
                speakers: speaker_list,
            });
        }

        if talks.is_empty() {
            bail!(
                "No talks found in RustConf 2017 schedule page. HTML length: {} chars.",
                document.html().len()
            );
        }

        info!("Parsed {} talks from schedule", talks.len());
        Ok(talks)
    }
}

fn parse_speakers(byline: &str) -> Vec<String> {
    let trimmed = byline.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let normalized = trimmed
        .strip_prefix("by")
        .unwrap_or(trimmed)
        .trim()
        .replace(" and ", ", ");

    normalized
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustconf_2017_metadata() {
        let parser = RustConf2017;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustconf-2017");
        assert_eq!(metadata.conference, "RustConf");
        assert_eq!(metadata.year, "2017");
        assert_eq!(
            metadata.url,
            Url::parse("https://2017.rustconf.com").expect("valid RustConf 2017 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=COrl851gMTY&list=PL85XCvVPmGQhUSX_QBkxb4g1-o56cCqI9"
                )
                .expect("valid RustConf 2017 playlist URL")
            )
        );
    }

    #[test]
    fn test_parse_speakers() {
        assert_eq!(
            parse_speakers("by Alice, Bob and Carol"),
            vec!["Alice", "Bob", "Carol"]
        );
        assert_eq!(parse_speakers("by Ada Lovelace"), vec!["Ada Lovelace"]);
    }
}
