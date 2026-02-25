//! EuroRust 2025 schedule parser.
//!
//! Paris, October 9–10 2025. Multi-track conference (Main Stage + Side Track + impl room).
//! Schedule is on a dedicated page at `/2025/schedule`.
//! Individual talk pages exist at `/2025/talks/{slug}/` with full abstracts.
//!
//! HTML structure (schedule page):
//! ```text
//! section#conference-day-1 / section#conference-day-2
//!   div.schedule__main-stage / div.schedule__side-track
//!     ol.schedule__list
//!       li > div.activity-card
//!         a.activity-card__link[href]              — link to talk detail page
//!         div.activity-card__content
//!           p.activity-card__time                  — time slot
//!           p.activity-card__title[aria-hidden]     — talk title
//!           p.activity-card__speaker               — speaker name (with » prefix)
//! ```
//!
//! Talks also appear in a `<table>` further down the page for each day.
//! We parse only the `ol.schedule__list` sections and deduplicate by talk slug.
//! Workshop-day and post-conference-day sections are skipped.

use std::collections::HashSet;
use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use log::{debug, info};
use scraper::Html;
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{ConferenceMetadata, ParsedTalk, ScheduleParser, static_url};
use crate::tools::css::{css, select_attr, select_text, text};

use super::{clean_speaker_name, fetch_talk_detail, should_skip};

/// Parser for EuroRust 2025
pub struct EuroRust2025;

static EURORUST_2025_SCHEDULE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://eurorust.eu/2025/schedule"));
static EURORUST_2025_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://eurorust.eu/2025"));
static EURORUST_2025_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=zYuXY4xiRFY&list=PLH6-VpZ3SvUUO_lfxniFmyKxUMngBOWFg",
    )
});

/// Conference dates for each day.
const DAY1_DATE: (i32, u32, u32) = (2025, 10, 9);
const DAY2_DATE: (i32, u32, u32) = (2025, 10, 10);

#[async_trait]
impl ScheduleParser for EuroRust2025 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "eurorust-2025",
            conference: "EuroRust",
            year: "2025",
            url: (*EURORUST_2025_BASE_URL).clone(),
            youtube_playlist_url: Some((*EURORUST_2025_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let url = &*EURORUST_2025_SCHEDULE_URL;
        info!("Fetching schedule from: {}", url);

        let response = client
            .get(url.as_str())
            .send()
            .await
            .context("Failed to fetch EuroRust 2025 schedule page")?;

        if !response.status().is_success() {
            bail!(
                "Failed to fetch EuroRust 2025 schedule page: HTTP {}",
                response.status()
            );
        }

        let html = response
            .text()
            .await
            .context("Failed to read EuroRust 2025 schedule page body")?;

        // Parse the HTML and extract talk entries in a block so the non-Send
        // `Html` document is dropped before the async talk-page fetches below.
        let talk_entries = {
            let document = Html::parse_document(&html);
            let entries = self.collect_talk_entries(&document)?;
            if entries.is_empty() {
                bail!(
                    "No talk entries found on EuroRust 2025 schedule page. HTML length: {} chars.",
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
                    log::warn!("Failed to fetch talk detail page {}: {}", entry.url, e);
                    // Fall back to the schedule data
                    if let Some(parsed) = entry.to_fallback_talk() {
                        talks.push(parsed);
                    }
                }
            }
        }

        if talks.is_empty() {
            bail!("No talks could be parsed from EuroRust 2025");
        }

        info!("Parsed {} talks from EuroRust 2025", talks.len());
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

impl EuroRust2025 {
    /// Parse the schedule page and collect unique talk entries.
    ///
    /// We parse `ol.schedule__list` elements inside `schedule__main-stage` and
    /// `schedule__side-track` containers for the two conference days only (skipping
    /// workshop day and post-conference day). Talks are deduplicated by slug.
    fn collect_talk_entries(&self, document: &Html) -> Result<Vec<TalkEntry>> {
        let main_stage_selector = css("div.schedule__main-stage")?;
        let side_track_selector = css("div.schedule__side-track")?;
        let list_selector = css("ol.schedule__list")?;
        let card_selector = css("div.activity-card")?;
        let link_selector = css("a.activity-card__link")?;
        let title_selector = css("p.activity-card__title")?;
        let speaker_selector = css("p.activity-card__speaker")?;

        let mut seen_slugs = HashSet::new();
        let mut entries = Vec::new();

        // Collect main-stage and side-track containers.
        // The 2025 schedule page has:
        //   - Workshop day: one schedule__main-stage (index 0) — skip
        //   - Conference Day 1: main-stage (index 1) + side-track (index 0)
        //   - Conference Day 2: main-stage (index 2) + side-track (index 1)
        //   (indices may shift depending on the page structure)
        //
        // Rather than relying on fragile indices, we look for
        // schedule__main-stage / schedule__side-track elements that contain
        // links to `/2025/talks/` — the workshop day links to `/2025/workshops/`.
        let track_containers: Vec<_> = document
            .select(&main_stage_selector)
            .chain(document.select(&side_track_selector))
            .collect();

        // We assign dates by tracking which talk slugs we see. The schedule
        // page renders Day 1 containers before Day 2 containers in the list
        // view. We detect the day boundary by finding the first main-stage
        // container that has talk links (Day 1), then the second (Day 2).
        //
        // Simpler approach: collect all lists from track containers that have
        // `/talks/` links, and split them into Day 1 vs Day 2 based on their
        // document order. The page renders Day 1 list views first, then Day 2
        // list views, then Day 1 table views, then Day 2 table views. We only
        // parse lists to avoid the table duplication.

        // Gather all schedule lists from track containers
        let mut talk_lists: Vec<(scraper::ElementRef<'_>, bool)> = Vec::new();

        for container in &track_containers {
            for list in container.select(&list_selector) {
                // Check if this list contains talk links (vs workshop links)
                let has_talk_links = list.select(&link_selector).any(|a| {
                    a.value()
                        .attr("href")
                        .map(|h| h.contains("/talks/"))
                        .unwrap_or(false)
                });
                if has_talk_links {
                    talk_lists.push((list, has_talk_links));
                }
            }
        }

        // The first half of talk_lists are Day 1 (list view for main + side),
        // the second half are Day 2. With 2 tracks per day we expect 4 lists
        // total from the list views, but there may be more from table views.
        // We just process all and rely on slug deduplication.
        let mid = talk_lists.len() / 2;

        for (list_idx, (list, _)) in talk_lists.iter().enumerate() {
            let date = if list_idx < mid {
                NaiveDate::from_ymd_opt(DAY1_DATE.0, DAY1_DATE.1, DAY1_DATE.2)
            } else {
                NaiveDate::from_ymd_opt(DAY2_DATE.0, DAY2_DATE.1, DAY2_DATE.2)
            }
            .context("Invalid date")?;

            for card in list.select(&card_selector) {
                // Skip workshop cards
                if card
                    .value()
                    .attr("class")
                    .map(|c| c.contains("workshop"))
                    .unwrap_or(false)
                {
                    continue;
                }

                // Only process cards that have a link to a talk detail page
                let href = match select_attr(card, &link_selector, "href") {
                    Some(h) => h,
                    None => continue,
                };

                // Only process /talks/ links (skip /workshops/)
                if !href.contains("/talks/") {
                    continue;
                }

                // Extract slug from href like "/2025/talks/how-rust-compiles/"
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

                // Extract title from p.activity-card__title
                let title = select_text(card, &title_selector).unwrap_or_default();

                if title.is_empty() || should_skip(&title) {
                    continue;
                }

                // Extract speaker names from p.activity-card__speaker
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
                let talk_url = format!("https://eurorust.eu/2025/talks/{slug}/");

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
    fn test_eurorust_2025_metadata() {
        let parser = EuroRust2025;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "eurorust-2025");
        assert_eq!(metadata.conference, "EuroRust");
        assert_eq!(metadata.year, "2025");
        assert_eq!(
            metadata.url,
            Url::parse("https://eurorust.eu/2025").expect("valid EuroRust 2025 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=zYuXY4xiRFY&list=PLH6-VpZ3SvUUO_lfxniFmyKxUMngBOWFg"
                )
                .expect("valid EuroRust 2025 playlist URL")
            )
        );
    }
}
