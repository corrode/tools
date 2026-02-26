//! EuroRust 2024 schedule parser.
//!
//! Vienna, October 10–11 2024. Two-track conference (Main Stage + Side Track).
//! Schedule is embedded on the main year page as activity cards.
//! Individual talk pages exist at `/2024/talks/{slug}/` with full abstracts.
//!
//! HTML structure (schedule on main page):
//! ```text
//! section.schedule
//!   div.schedule__main-stage / div.schedule__side-track
//!     ol.schedule__list
//!       li > div.activity-card
//!         a.activity-card__link[href]          — link to talk detail page
//!           span.visually-hidden               — talk title
//!         div.activity-card__content
//!           p.large                            — time slot
//!           p[aria-hidden]                     — talk title (visible)
//!           p > a.speaker                      — speaker name(s)
//! ```
//!
//! Talks also appear in a `<table class="schedule__table">` further down the
//! page. We parse only the `ol.schedule__list` sections and deduplicate by
//! talk slug to avoid double-counting.

use std::collections::HashSet;
use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use scraper::Html;
use tracing::{debug, info};
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{ConferenceMetadata, ParsedTalk, ScheduleParser, static_url};

use crate::tools::css::{css, select_attr, select_text, text};

use super::{clean_speaker_name, fetch_talk_detail, should_skip};

/// Parser for EuroRust 2024
pub struct EuroRust2024;

static EURORUST_2024_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://eurorust.eu/2024"));
static EURORUST_2024_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=8-KLX1PGg8Q&list=PLH6-VpZ3SvUWox7mJDLNCu_E0gl7a-fP3",
    )
});

/// Conference dates for each day.
const DAY1_DATE: (i32, u32, u32) = (2024, 10, 10);
const DAY2_DATE: (i32, u32, u32) = (2024, 10, 11);

#[async_trait]
impl ScheduleParser for EuroRust2024 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "eurorust-2024",
            conference: "EuroRust",
            year: "2024",
            url: (*EURORUST_2024_BASE_URL).clone(),
            youtube_playlist_url: Some((*EURORUST_2024_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let url = &*EURORUST_2024_BASE_URL;
        info!("Fetching schedule from: {}", url);

        let response = client
            .get(url.as_str())
            .send()
            .await
            .context("Failed to fetch EuroRust 2024 page")?;

        if !response.status().is_success() {
            bail!(
                "Failed to fetch EuroRust 2024 page: HTTP {}",
                response.status()
            );
        }

        let html = response
            .text()
            .await
            .context("Failed to read EuroRust 2024 page body")?;

        // Parse the HTML and extract talk entries in a block so the non-Send
        // `Html` document is dropped before the async talk-page fetches below.
        let talk_entries = {
            let document = Html::parse_document(&html);
            let entries = self.collect_talk_entries(&document)?;
            if entries.is_empty() {
                bail!(
                    "No talk entries found on EuroRust 2024 page. HTML length: {} chars.",
                    html.len()
                );
            }
            entries
        };

        info!(
            "Found {} unique talk entries on schedule, fetching detail pages...",
            talk_entries.len()
        );

        let mut talks = Vec::new();
        for entry in &talk_entries {
            match fetch_talk_detail(client, &entry.url, entry.date, "EuroRust").await {
                Ok(Some(parsed)) => {
                    debug!("Parsed talk detail: {}", parsed.talk.title);
                    talks.push(parsed);
                }
                Ok(None) => {
                    debug!("Skipped talk detail page: {}", entry.url);
                    // Fall back to the schedule data we already have
                    if let Some(parsed) = entry.to_fallback_talk() {
                        talks.push(parsed);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch talk detail page {}: {}", entry.url, e);
                    // Fall back to the schedule data
                    if let Some(parsed) = entry.to_fallback_talk() {
                        talks.push(parsed);
                    }
                }
            }
        }

        if talks.is_empty() {
            bail!("No talks could be parsed from EuroRust 2024");
        }

        info!("Parsed {} talks from EuroRust 2024", talks.len());
        Ok(talks)
    }
}

/// A talk entry extracted from the schedule overview, before fetching the
/// detail page.
struct TalkEntry {
    _slug: String,
    url: String,
    title: String,
    speakers: Vec<String>,
    date: NaiveDate,
}

impl TalkEntry {
    /// Build a fallback [`ParsedTalk`] from schedule data when the detail page
    /// cannot be fetched.
    fn to_fallback_talk(&self) -> Option<ParsedTalk> {
        if self.title.is_empty() || self.speakers.is_empty() {
            return None;
        }

        let talk = NewTalk {
            title: self.title.clone(),
            summary: format!("Talk by {}", self.speakers.join(", ")),
            transcript: None,
            conference: "EuroRust".to_string(),
            date: self.date,
            website_url: Url::parse(&self.url).ok()?,
            video_url: None,
            slides_url: None,
            thumbnail_url: None,
            duration_seconds: None,
        };

        let speakers = self
            .speakers
            .iter()
            .map(|name| NewSpeaker { name: name.clone() })
            .collect();

        Some(ParsedTalk { talk, speakers })
    }
}

impl EuroRust2024 {
    /// Parse the schedule overview page and collect unique talk entries.
    ///
    /// We parse only the `ol.schedule__list` elements inside the
    /// `schedule__main-stage` and `schedule__side-track` containers.
    /// Each conference day section appears once in the list view and once
    /// in a table view — we deduplicate by slug.
    fn collect_talk_entries(&self, document: &Html) -> Result<Vec<TalkEntry>> {
        let list_selector = css("ol.schedule__list")?;
        let card_selector = css("div.activity-card")?;
        let link_selector = css("a.activity-card__link")?;
        let hidden_title_selector = css("span.visually-hidden")?;
        let speaker_selector = css("a.speaker")?;

        let mut seen_slugs = HashSet::new();
        let mut entries = Vec::new();

        // The page has two day sections. We assign dates based on the order of
        // schedule__list elements: lists 0-1 are Day 1 (main + side), lists 2-3
        // are Day 2 (main + side).
        let lists: Vec<_> = document.select(&list_selector).collect();

        for (list_idx, list) in lists.iter().enumerate() {
            let date = if list_idx < 2 {
                NaiveDate::from_ymd_opt(DAY1_DATE.0, DAY1_DATE.1, DAY1_DATE.2)
            } else {
                NaiveDate::from_ymd_opt(DAY2_DATE.0, DAY2_DATE.1, DAY2_DATE.2)
            }
            .context("Invalid date")?;

            for card in list.select(&card_selector) {
                // Only process cards that have a link to a talk detail page
                let href = match select_attr(card, &link_selector, "href") {
                    Some(h) => h,
                    None => continue,
                };

                // Extract slug from href like "/2024/talks/through-the-fire-and-the-flames/"
                let slug = href
                    .trim_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_string();

                if slug.is_empty() {
                    continue;
                }

                // Deduplicate
                if !seen_slugs.insert(slug.clone()) {
                    continue;
                }

                // Extract title from the visually-hidden span inside the link
                let link = match card.select(&link_selector).next() {
                    Some(a) => a,
                    None => continue,
                };
                let title = select_text(link, &hidden_title_selector).unwrap_or_default();

                if title.is_empty() || should_skip(&title) {
                    continue;
                }

                // Extract speaker names
                let speakers: Vec<String> = card
                    .select(&speaker_selector)
                    .map(|el| clean_speaker_name(&text(el)))
                    .filter(|name| !name.is_empty())
                    .collect();

                if speakers.is_empty() {
                    debug!("Skipping card with no speakers: {}", title);
                    continue;
                }

                // Build the canonical URL for this talk
                let talk_url = format!("https://eurorust.eu/2024/talks/{slug}/");

                entries.push(TalkEntry {
                    _slug: slug,
                    url: talk_url,
                    title,
                    speakers,
                    date,
                });
            }
        }

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eurorust_2024_metadata() {
        let parser = EuroRust2024;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "eurorust-2024");
        assert_eq!(metadata.conference, "EuroRust");
        assert_eq!(metadata.year, "2024");
        assert_eq!(
            metadata.url,
            Url::parse("https://eurorust.eu/2024").expect("valid EuroRust 2024 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=8-KLX1PGg8Q&list=PLH6-VpZ3SvUWox7mJDLNCu_E0gl7a-fP3"
                )
                .expect("valid EuroRust 2024 playlist URL")
            )
        );
    }
}
