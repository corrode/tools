//! RustNL 2023 schedule parser.
//!
//! Amsterdam, May 10 2023. Single-day, single-track conference.
//! Schedule is embedded as an HTML table on the main page.
//! No individual talk pages exist.
//!
//! HTML structure:
//! ```text
//! <table>
//!   <tr>
//!     <td><span>10:00</span></td>
//!     <td>
//!       <span>Makepad: Designing modern UIs with Rust</span>
//!       <br>
//!       <em><span>Rik Arends</span></em>
//!     </td>
//!   </tr>
//!   ...
//! </table>
//! ```
//!
//! Talks have an `<em>` element containing the speaker name inside the second
//! `<td>`. Non-talk items (registration, breaks, lunch, etc.) have no `<em>`.

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
use crate::tools::css::{css, parse_speaker_names, slugify, text};

use super::should_skip;

/// Parser for RustNL 2023
pub struct RustNL2023;

static RUSTNL_2023_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://2023.rustnl.org"));
static RUSTNL_2023_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=rC4FCS-oMpg&list=PL8Q1w7Ff68DCM_fsMM4v9m473sYLvJwHS",
    )
});

/// Conference date.
const CONF_DATE: (i32, u32, u32) = (2023, 5, 10);

#[async_trait]
impl ScheduleParser for RustNL2023 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustnl-2023",
            conference: "RustNL",
            year: "2023",
            url: (*RUSTNL_2023_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTNL_2023_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let url = &*RUSTNL_2023_BASE_URL;
        info!("Fetching schedule from: {}", url);

        let response = client
            .get(url.as_str())
            .send()
            .await
            .context("Failed to fetch RustNL 2023 page")?;

        if !response.status().is_success() {
            bail!(
                "Failed to fetch RustNL 2023 page: HTTP {}",
                response.status()
            );
        }

        let html = response
            .text()
            .await
            .context("Failed to read RustNL 2023 page body")?;

        let document = Html::parse_document(&html);
        self.parse_schedule(&document)
    }
}

impl RustNL2023 {
    fn parse_schedule(&self, document: &Html) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();
        let base_url = base_url(&self.metadata().url)?;

        let date = NaiveDate::from_ymd_opt(CONF_DATE.0, CONF_DATE.1, CONF_DATE.2)
            .context("Invalid date")?;

        let tr_selector = css("tr")?;
        let td_selector = css("td")?;
        let em_selector = css("em")?;
        let span_selector = css("span")?;

        for row in document.select(&tr_selector) {
            let tds: Vec<_> = row.select(&td_selector).collect();
            if tds.len() < 2 {
                continue;
            }

            let content_td = tds[1];

            // Only talk rows have an <em> element containing the speaker name.
            let speaker_em = match content_td.select(&em_selector).next() {
                Some(em) => em,
                None => continue,
            };

            // Extract speaker name from <em><span>Speaker Name</span></em>
            let speaker_name = speaker_em
                .select(&span_selector)
                .next()
                .map(|el| text(el))
                .or_else(|| {
                    let t = text(speaker_em);
                    if t.is_empty() { None } else { Some(t) }
                })
                .unwrap_or_default();

            if speaker_name.is_empty() {
                continue;
            }

            // Extract talk title from the <span> elements in the content td
            // that are NOT inside the <em> (speaker name).
            // The title is in the first <span> child of the td.
            let title = content_td
                .select(&span_selector)
                .next()
                .map(|el| text(el))
                .unwrap_or_default();

            if title.is_empty() || should_skip(&title) {
                continue;
            }

            // Some titles may contain the speaker name appended with a separator;
            // clean that up.
            let title = title
                .strip_suffix(&speaker_name)
                .unwrap_or(&title)
                .trim()
                .trim_end_matches('–')
                .trim_end_matches('-')
                .trim()
                .to_string();

            if title.is_empty() {
                continue;
            }

            // Handle talks with multiple speakers (separated by " & " or " and ")
            let speakers = parse_speaker_names(&speaker_name);

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

        if talks.is_empty() {
            bail!(
                "No talks found on RustNL 2023 page. HTML length: {} chars.",
                document.html().len()
            );
        }

        info!("Parsed {} talks from RustNL 2023 schedule", talks.len());
        Ok(talks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustnl_2023_metadata() {
        let parser = RustNL2023;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustnl-2023");
        assert_eq!(metadata.conference, "RustNL");
        assert_eq!(metadata.year, "2023");
        assert_eq!(
            metadata.url,
            Url::parse("https://2023.rustnl.org").expect("valid RustNL 2023 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=rC4FCS-oMpg&list=PL8Q1w7Ff68DCM_fsMM4v9m473sYLvJwHS"
                )
                .expect("valid RustNL 2023 playlist URL")
            )
        );
    }

    #[test]
    fn test_parse_schedule_from_html() {
        let html = r#"
        <html><body>
        <table>
            <tr>
                <td><span>9:00</span></td>
                <td><span>Registration</span></td>
            </tr>
            <tr>
                <td><span>10:00</span></td>
                <td>
                    <span>Makepad: Designing modern UIs with Rust</span>
                    <br>
                    <em><span>Rik Arends</span></em>
                </td>
            </tr>
            <tr>
                <td><span>10:45</span></td>
                <td>
                    <span>The Mystery of the Pin</span>
                    <br>
                    <em><span>Martin Hoffmann</span></em>
                </td>
            </tr>
            <tr>
                <td><span>11:15</span></td>
                <td><span>Break</span></td>
            </tr>
        </table>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let parser = RustNL2023;
        let talks = parser.parse_schedule(&document).unwrap();

        assert_eq!(talks.len(), 2);
        assert_eq!(
            talks[0].talk.title,
            "Makepad: Designing modern UIs with Rust"
        );
        assert_eq!(talks[0].speakers[0].name, "Rik Arends");
        assert_eq!(talks[1].talk.title, "The Mystery of the Pin");
        assert_eq!(talks[1].speakers[0].name, "Martin Hoffmann");
    }
}
