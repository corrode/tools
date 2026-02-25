//! RustNL 2024 schedule parser.
//!
//! Delft, May 7–8 2024. Two-day, single-track conference.
//! Schedule is on a dedicated page at `/schedule/`.
//! No individual talk pages exist; speaker bios are at `/speakers#slug`.
//!
//! HTML structure:
//! ```text
//! <h3>Schedule <strong>day 1 (May 7th)</strong></h3>
//! <ul class="timetable">
//!   <li id="rik">
//!     <div class="time">10:00</div>
//!     <a href="/speakers#rik">
//!       <div class="name">Rik Arends</div>
//!       Visual application design for Rust
//!     </a>
//!   </li>
//!   <!-- non-talk items have a <div> instead of <a>, no .name -->
//!   <li>
//!     <div class="time">11:20</div>
//!     <div>  Break  </div>
//!   </li>
//! </ul>
//!
//! <h3>Schedule <strong>day 2 (May 8th)</strong></h3>
//! <ul class="timetable">
//!   ...
//! </ul>
//! ```
//!
//! Talks are `<li>` elements that contain an `<a>` with a `<div class="name">`
//! child. The talk title is the remaining text content of the `<a>` after
//! stripping the speaker name.

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
use crate::tools::css::{css, parse_speaker_names, select_text, slugify, text};

use super::should_skip;

/// Parser for RustNL 2024
pub struct RustNL2024;

static RUSTNL_2024_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://2024.rustnl.org"));
static RUSTNL_2024_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=NPP2_6KMA60&list=PL8Q1w7Ff68DBZZbJt3ie5MUoJV5v2HeA7",
    )
});

/// Conference dates for each day.
const DAY1_DATE: (i32, u32, u32) = (2024, 5, 7);
const DAY2_DATE: (i32, u32, u32) = (2024, 5, 8);

#[async_trait]
impl ScheduleParser for RustNL2024 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustnl-2024",
            conference: "RustNL",
            year: "2024",
            url: (*RUSTNL_2024_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTNL_2024_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let base_url = base_url(&self.metadata().url)?;
        let schedule_url = base_url
            .join("schedule/")
            .context("Failed to build schedule URL")?;
        info!("Fetching schedule from: {}", schedule_url);

        let response = client
            .get(schedule_url.as_str())
            .send()
            .await
            .context("Failed to fetch RustNL 2024 schedule page")?;

        if !response.status().is_success() {
            bail!(
                "Failed to fetch RustNL 2024 schedule page: HTTP {}",
                response.status()
            );
        }

        let html = response
            .text()
            .await
            .context("Failed to read RustNL 2024 schedule page body")?;

        let document = Html::parse_document(&html);
        self.parse_schedule(&document)
    }
}

impl RustNL2024 {
    fn parse_schedule(&self, document: &Html) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();
        let base_url = base_url(&self.metadata().url)?;

        let timetable_selector = css("ul.timetable")?;
        let li_selector = css("li")?;
        let a_selector = css("a")?;
        let name_selector = css("div.name")?;

        // The page has two <ul class="timetable"> elements: one for day 1, one for day 2.
        let timetables: Vec<_> = document.select(&timetable_selector).collect();

        for (tt_idx, timetable) in timetables.iter().enumerate() {
            let date = if tt_idx == 0 {
                NaiveDate::from_ymd_opt(DAY1_DATE.0, DAY1_DATE.1, DAY1_DATE.2)
            } else {
                NaiveDate::from_ymd_opt(DAY2_DATE.0, DAY2_DATE.1, DAY2_DATE.2)
            }
            .context("Invalid date")?;

            debug!("Parsing RustNL 2024 day {} ({})", tt_idx + 1, date);

            for li in timetable.select(&li_selector) {
                // Only talk items have an <a> with a <div class="name"> inside.
                let link = match li.select(&a_selector).next() {
                    Some(a) => a,
                    None => continue,
                };

                let speaker_name = match select_text(link, &name_selector) {
                    Some(name) => name,
                    None => continue,
                };

                if speaker_name.is_empty() {
                    continue;
                }

                // The talk title is the text content of the <a> minus the speaker
                // name. The <a> contains:
                //   <div class="name">Speaker</div>
                //   Talk Title Text
                let full_text = text(link);
                let title = full_text
                    .strip_prefix(&speaker_name)
                    .or_else(|| full_text.strip_suffix(&speaker_name))
                    .unwrap_or(&full_text)
                    .trim()
                    .to_string();

                if title.is_empty() || should_skip(&title) {
                    continue;
                }

                // Parse multiple speakers if present
                let speakers = parse_speaker_names(&speaker_name);

                if speakers.is_empty() {
                    debug!("Skipping item with no speakers: {}", title);
                    continue;
                }

                // Build URL: use the li id if present, otherwise slugify the title
                let slug = li
                    .value()
                    .id()
                    .map(String::from)
                    .unwrap_or_else(|| slugify(&title));

                let website_url = base_url
                    .join(&format!("schedule/#{}", slug))
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
                "No talks found on RustNL 2024 schedule page. HTML length: {} chars.",
                document.html().len()
            );
        }

        info!("Parsed {} talks from RustNL 2024 schedule", talks.len());
        Ok(talks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustnl_2024_metadata() {
        let parser = RustNL2024;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustnl-2024");
        assert_eq!(metadata.conference, "RustNL");
        assert_eq!(metadata.year, "2024");
        assert_eq!(
            metadata.url,
            Url::parse("https://2024.rustnl.org").expect("valid RustNL 2024 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=NPP2_6KMA60&list=PL8Q1w7Ff68DBZZbJt3ie5MUoJV5v2HeA7"
                )
                .expect("valid RustNL 2024 playlist URL")
            )
        );
    }

    #[test]
    fn test_parse_schedule_from_html() {
        let html = r#"
        <html><body>
        <h3>Schedule <strong>day 1 (May 7th)</strong></h3>
        <ul class="timetable">
            <li>
                <div class="time">08:45</div>
                <div>  Registration Opens </div>
            </li>
            <li id="rik">
                <div class="time">10:00</div>
                <a href="/speakers#rik">
                    <div class="name">Rik Arends</div>
                    Visual application design for Rust
                </a>
            </li>
            <li id="michael">
                <div class="time">10:40</div>
                <a href="/speakers#michael">
                    <div class="name">Michaël Melchiore</div>
                    (Th)Rust for Space: Initial momentum
                </a>
            </li>
            <li>
                <div class="time">11:20</div>
                <div>  Break  </div>
            </li>
        </ul>

        <h3>Schedule <strong>day 2 (May 8th)</strong></h3>
        <ul class="timetable">
            <li id="sophia">
                <div class="time">09:30</div>
                <a href="/speakers#sophia">
                    <div class="name">Sophia Turner</div>
                    Secret!
                </a>
            </li>
            <li id="kevin">
                <div class="time">10:10</div>
                <a href="/speakers#kevin">
                    <div class="name">Kevin Boos</div>
                    Robius: Immersive and Seamless Multi-platform App Development in Rust
                </a>
            </li>
        </ul>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let parser = RustNL2024;
        let talks = parser.parse_schedule(&document).unwrap();

        assert_eq!(talks.len(), 4);

        // Day 1
        assert_eq!(talks[0].talk.title, "Visual application design for Rust");
        assert_eq!(talks[0].speakers[0].name, "Rik Arends");
        assert_eq!(
            talks[0].talk.date,
            NaiveDate::from_ymd_opt(2024, 5, 7).unwrap()
        );

        assert_eq!(talks[1].talk.title, "(Th)Rust for Space: Initial momentum");
        assert_eq!(talks[1].speakers[0].name, "Michaël Melchiore");

        // Day 2
        assert_eq!(talks[2].talk.title, "Secret!");
        assert_eq!(talks[2].speakers[0].name, "Sophia Turner");
        assert_eq!(
            talks[2].talk.date,
            NaiveDate::from_ymd_opt(2024, 5, 8).unwrap()
        );

        assert_eq!(
            talks[3].talk.title,
            "Robius: Immersive and Seamless Multi-platform App Development in Rust"
        );
        assert_eq!(talks[3].speakers[0].name, "Kevin Boos");
    }

    #[test]
    fn test_parse_schedule_skips_non_talks() {
        let html = r#"
        <html><body>
        <ul class="timetable">
            <li>
                <div class="time">08:45</div>
                <div>  Registration Opens </div>
            </li>
            <li>
                <div class="time">11:20</div>
                <div>  Break  </div>
            </li>
            <li>
                <div class="time">17:00</div>
                <div>  Drinks  </div>
            </li>
        </ul>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let parser = RustNL2024;
        let result = parser.parse_schedule(&document);
        // Should bail because no talks were found
        assert!(result.is_err());
    }

    #[test]
    fn test_website_url_uses_li_id() {
        let html = r#"
        <html><body>
        <ul class="timetable">
            <li id="alice">
                <div class="time">11:50</div>
                <a href="/speakers#alice">
                    <div class="name">Alice Ryhl</div>
                    Arc in the Linux Kernel
                </a>
            </li>
        </ul>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let parser = RustNL2024;
        let talks = parser.parse_schedule(&document).unwrap();

        assert_eq!(talks.len(), 1);
        assert!(
            talks[0]
                .talk
                .website_url
                .as_str()
                .ends_with("schedule/#alice"),
            "Expected URL to end with 'schedule/#alice', got: {}",
            talks[0].talk.website_url
        );
    }
}
