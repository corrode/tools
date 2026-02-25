//! RustWeek 2025 schedule parser.
//!
//! Utrecht, May 13–14 2025. Two-day, multi-track conference.
//! Schedule is split across two pages:
//!   - `/schedule/tuesday`  (Main, Ecosystem, Industry tracks)
//!   - `/schedule/wednesday` (Main, Deep Dive, Rust Project tracks)
//!
//! Individual talk pages exist at `/talks/{slug}/` with abstracts and full
//! speaker lists (important for multi-speaker talks).
//!
//! HTML structure (schedule pages):
//! ```text
//! <div class="schedule">
//!   <!-- Single-talk item -->
//!   <div class="item yellow" id="alex" data-start="9:40" data-end="10:15">
//!     <div class="content">
//!       <div class="meta">...</div>
//!       <div class="block">
//!         <a class="title" href="/talks/alex">10 Years of Rust: Why?</a>
//!         <a class="speaker" href="/talks/alex">
//!           <img class="speaker-img" ...>
//!           <p>Alex Crichton</p>
//!         </a>
//!       </div>
//!     </div>
//!   </div>
//!
//!   <!-- Multi-talk item (GOSIM Spotlight / shared slot) -->
//!   <div class="item blue" id="gu" ...>
//!     <div class="content">
//!       <div class="meta">...</div>
//!       <div class="talks">
//!         <div class="subitem blue">
//!           <a class="title" href="/talks/gu">Put Rust in your keyboard</a>
//!           <a class="speaker" href="/talks/gu">
//!             <img ...><p>Gu Haobo</p>
//!           </a>
//!         </div>
//!         <div class="subitem blue">
//!           <a class="title" href="/talks/denis">Tango: ...</a>
//!           <a class="speaker" href="/talks/denis">
//!             <img ...><p>Denis Bazhenov</p>
//!           </a>
//!         </div>
//!       </div>
//!     </div>
//!   </div>
//!
//!   <!-- Break -->
//!   <div class="wide">
//!     <div class="time">11:00 - 11:40</div> Break
//!   </div>
//!
//!   <!-- Track indicator (not a talk) -->
//!   <div class="track-indicator item yellow">Main track</div>
//! </div>
//! ```
//!
//! Items with class `striped` are industry-ticket-required but still valid talks.
//! Items with class `track-indicator` or `wide` are not talks.

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

use super::{fetch_talk_detail, should_skip};

/// Parser for RustWeek 2025
pub struct RustWeek2025;

static RUSTWEEK_2025_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://2025.rustweek.org"));
static RUSTWEEK_2025_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=E56STygm8i4&list=PL8Q1w7Ff68DCEXiGidlM0DMn8ztjlUlez",
    )
});

/// Conference dates for each day.
const DAY1_DATE: (i32, u32, u32) = (2025, 5, 13); // Tuesday
const DAY2_DATE: (i32, u32, u32) = (2025, 5, 14); // Wednesday

/// Schedule page paths (relative to the base URL).
const SCHEDULE_PAGES: &[(&str, (i32, u32, u32))] = &[
    ("schedule/tuesday", DAY1_DATE),
    ("schedule/wednesday", DAY2_DATE),
];

#[async_trait]
impl ScheduleParser for RustWeek2025 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustweek-2025",
            conference: "RustWeek",
            year: "2025",
            url: (*RUSTWEEK_2025_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTWEEK_2025_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let base = &*RUSTWEEK_2025_BASE_URL;

        // Collect talk entries from both schedule pages. We parse HTML in a
        // block so the non-Send `Html` document is dropped before the async
        // talk-page fetches below.
        let mut all_entries: Vec<TalkEntry> = Vec::new();
        let mut seen_slugs: HashSet<String> = HashSet::new();

        for &(path, ymd) in SCHEDULE_PAGES {
            let url = format!("{}/{}", base.as_str().trim_end_matches('/'), path);
            info!("Fetching schedule from: {}", url);

            let response = client
                .get(&url)
                .send()
                .await
                .with_context(|| format!("Failed to fetch {url}"))?;

            if !response.status().is_success() {
                bail!("Failed to fetch {url}: HTTP {}", response.status());
            }

            let html = response
                .text()
                .await
                .with_context(|| format!("Failed to read body of {url}"))?;

            let date =
                NaiveDate::from_ymd_opt(ymd.0, ymd.1, ymd.2).context("Invalid date constant")?;

            let entries = {
                let document = Html::parse_document(&html);
                self.collect_talk_entries(&document, date, &mut seen_slugs)?
            };

            info!("Found {} talk entries on {}", entries.len(), path);
            all_entries.extend(entries);
        }

        if all_entries.is_empty() {
            bail!("No talk entries found on any RustWeek 2025 schedule page");
        }

        info!(
            "Found {} unique talk entries total, fetching detail pages...",
            all_entries.len()
        );

        let mut talks = Vec::new();
        for entry in &all_entries {
            match fetch_talk_detail(client, &entry.url, entry.date, "RustWeek").await {
                Ok(Some(parsed)) => {
                    debug!("Parsed talk detail: {}", parsed.talk.title);
                    talks.push(parsed);
                }
                Ok(None) => {
                    debug!("Skipped talk detail page: {}", entry.url);
                    if let Some(parsed) = entry.to_fallback_talk() {
                        talks.push(parsed);
                    }
                }
                Err(e) => {
                    log::warn!("Failed to fetch talk detail page {}: {}", entry.url, e);
                    if let Some(parsed) = entry.to_fallback_talk() {
                        talks.push(parsed);
                    }
                }
            }
        }

        if talks.is_empty() {
            bail!("No talks could be parsed from RustWeek 2025");
        }

        info!("Parsed {} talks from RustWeek 2025", talks.len());
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
            conference: "RustWeek".to_string(),
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

impl RustWeek2025 {
    /// Parse a single schedule page and collect unique talk entries.
    ///
    /// There are two kinds of talk containers on the page:
    ///
    /// 1. **Single-talk items**: `<div class="item ...">` containing a
    ///    `<div class="block">` with one `a.title` and one `a.speaker`.
    ///
    /// 2. **Multi-talk items**: `<div class="item ...">` containing a
    ///    `<div class="talks">` with multiple `<div class="subitem ...">`,
    ///    each having its own `a.title` and `a.speaker`.
    ///
    /// We skip `<div class="wide">` (breaks) and `<div class="track-indicator ...">`.
    fn collect_talk_entries(
        &self,
        document: &Html,
        date: NaiveDate,
        seen_slugs: &mut HashSet<String>,
    ) -> Result<Vec<TalkEntry>> {
        // Select all item containers (both single and multi-talk).
        // We use a selector that matches <div> elements whose class contains "item"
        // but NOT "track-indicator".
        let item_selector = css("div.item")?;
        let block_selector = css("div.block")?;
        let talks_selector = css("div.talks")?;
        let subitem_selector = css("div.subitem")?; // used inside talks container
        let title_selector = css("a.title")?;
        let speaker_selector = css("a.speaker")?;

        let mut entries = Vec::new();

        for item in document.select(&item_selector) {
            let class = item.value().attr("class").unwrap_or("");

            // Skip track indicators (they also have "item" in their class)
            if class.contains("track-indicator") {
                continue;
            }

            // Case 1: Multi-talk block (GOSIM Spotlight / shared time slot)
            if let Some(talks_div) = item.select(&talks_selector).next() {
                for subitem in talks_div.select(&subitem_selector) {
                    if let Some(entry) =
                        self.extract_entry(subitem, &title_selector, &speaker_selector, date)?
                        && seen_slugs.insert(entry._slug.clone())
                    {
                        entries.push(entry);
                    }
                }
                continue;
            }

            // Case 2: Single-talk block
            if let Some(block) = item.select(&block_selector).next()
                && let Some(entry) =
                    self.extract_entry(block, &title_selector, &speaker_selector, date)?
                && seen_slugs.insert(entry._slug.clone())
            {
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    /// Extract a [`TalkEntry`] from a block or subitem element.
    fn extract_entry(
        &self,
        container: scraper::ElementRef<'_>,
        title_selector: &scraper::Selector,
        speaker_selector: &scraper::Selector,
        date: NaiveDate,
    ) -> Result<Option<TalkEntry>> {
        // Get the title link
        let href = match select_attr(container, title_selector, "href") {
            Some(h) => h.to_string(),
            None => return Ok(None),
        };

        // Only process /talks/ links
        if !href.contains("/talks/") {
            return Ok(None);
        }

        // Extract slug from href like "/talks/alex"
        let slug = href
            .trim_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string();

        if slug.is_empty() {
            return Ok(None);
        }

        // Extract title
        let title = match select_text(container, title_selector) {
            Some(t) => t,
            None => return Ok(None),
        };

        if title.is_empty() || should_skip(&title) {
            return Ok(None);
        }

        // Extract speaker name from <a class="speaker"><p>Name</p></a>
        // The speaker <a> contains an <img> and a <p> with the name.
        // Using text() on the <a> will pick up the <p> text and the alt text
        // from <img>. We prefer the <p> text inside the speaker link.
        let speakers: Vec<String> = container
            .select(speaker_selector)
            .filter_map(|a| {
                // Try to get text from a <p> child first (cleaner)
                let p_sel = css("p").ok()?;
                let name = select_text(a, &p_sel).or_else(|| {
                    let t = text(a);
                    if t.is_empty() { None } else { Some(t) }
                })?;
                let name = name.trim().to_string();
                if name.is_empty() { None } else { Some(name) }
            })
            .collect();

        if speakers.is_empty() {
            debug!("Skipping talk with no speakers: {}", title);
            return Ok(None);
        }

        // Build the canonical URL for this talk
        let talk_url = format!(
            "{}/talks/{}/",
            RUSTWEEK_2025_BASE_URL.as_str().trim_end_matches('/'),
            slug
        );

        Ok(Some(TalkEntry {
            _slug: slug,
            url: talk_url,
            title,
            speakers,
            date,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_rustweek_2025_metadata() {
        let parser = RustWeek2025;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustweek-2025");
        assert_eq!(metadata.conference, "RustWeek");
        assert_eq!(metadata.year, "2025");
        assert_eq!(
            metadata.url,
            Url::parse("https://2025.rustweek.org").expect("valid RustWeek 2025 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=E56STygm8i4&list=PL8Q1w7Ff68DCEXiGidlM0DMn8ztjlUlez"
                )
                .expect("valid RustWeek 2025 playlist URL")
            )
        );
    }

    #[test]
    fn test_collect_single_talk() {
        let html = r#"
        <html><body>
        <div class="schedule">
            <div class="item yellow" id="alex" data-start="9:40" data-end="10:15">
                <div class="content">
                    <div class="meta">
                        <div>9:40 <span class="duration">(35 min)</span></div>
                        <div>Cinema 12</div>
                    </div>
                    <div class="block">
                        <a class="title" href="/talks/alex">10 Years of Rust: Why?</a>
                        <a class="speaker" href="/talks/alex">
                            <img class="speaker-img" src="/images/alex.jpg" alt="Picture of Alex Crichton">
                            <p>Alex Crichton</p>
                        </a>
                    </div>
                </div>
            </div>
        </div>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let parser = RustWeek2025;
        let date = NaiveDate::from_ymd_opt(2025, 5, 13).unwrap();
        let mut seen = HashSet::new();
        let entries = parser
            .collect_talk_entries(&document, date, &mut seen)
            .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "10 Years of Rust: Why?");
        assert_eq!(entries[0].speakers, vec!["Alex Crichton"]);
        assert_eq!(entries[0]._slug, "alex");
        assert_eq!(entries[0].url, "https://2025.rustweek.org/talks/alex/");
    }

    #[test]
    fn test_collect_multi_talk_block() {
        let html = r#"
        <html><body>
        <div class="schedule">
            <div class="item blue" id="gu" data-start="10:25" data-end="11:00">
                <div class="content">
                    <div class="meta">
                        <div>10:25 <span class="duration">(35 min)</span></div>
                        <div>Cinema 11</div>
                    </div>
                    <div class="talks">
                        <div class="subitem blue">
                            <a class="title" href="/talks/gu">Put Rust in your keyboard</a>
                            <a class="speaker" href="/talks/gu">
                                <img class="speaker-img" src="/images/gu.jpg" alt="Picture of Gu Haobo">
                                <p>Gu Haobo</p>
                            </a>
                        </div>
                        <div class="subitem blue">
                            <a class="title" href="/talks/denis">Tango: Precise Performance Measurement</a>
                            <a class="speaker" href="/talks/denis">
                                <img class="speaker-img" src="/images/denis.jpg" alt="Picture of Denis Bazhenov">
                                <p>Denis Bazhenov</p>
                            </a>
                        </div>
                    </div>
                </div>
            </div>
        </div>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let parser = RustWeek2025;
        let date = NaiveDate::from_ymd_opt(2025, 5, 13).unwrap();
        let mut seen = HashSet::new();
        let entries = parser
            .collect_talk_entries(&document, date, &mut seen)
            .unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Put Rust in your keyboard");
        assert_eq!(entries[0].speakers, vec!["Gu Haobo"]);
        assert_eq!(entries[1].title, "Tango: Precise Performance Measurement");
        assert_eq!(entries[1].speakers, vec!["Denis Bazhenov"]);
    }

    #[test]
    fn test_skips_breaks_and_track_indicators() {
        let html = r#"
        <html><body>
        <div class="schedule">
            <!-- Break -->
            <div class="wide">
                <div class="time">11:00 - 11:40</div> Break
            </div>

            <!-- Track indicator -->
            <div class="track-indicator item yellow">Main track</div>

            <!-- Actual talk -->
            <div class="item yellow" id="raph">
                <div class="content">
                    <div class="block">
                        <a class="title" href="/talks/raph">Faster, easier 2D vector rendering</a>
                        <a class="speaker" href="/talks/raph">
                            <p>Raph Levien</p>
                        </a>
                    </div>
                </div>
            </div>
        </div>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let parser = RustWeek2025;
        let date = NaiveDate::from_ymd_opt(2025, 5, 13).unwrap();
        let mut seen = HashSet::new();
        let entries = parser
            .collect_talk_entries(&document, date, &mut seen)
            .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Faster, easier 2D vector rendering");
    }

    #[test]
    fn test_deduplication_across_pages() {
        let html = r#"
        <html><body>
        <div class="schedule">
            <div class="item yellow" id="alex">
                <div class="content">
                    <div class="block">
                        <a class="title" href="/talks/alex">10 Years of Rust: Why?</a>
                        <a class="speaker" href="/talks/alex"><p>Alex Crichton</p></a>
                    </div>
                </div>
            </div>
        </div>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let parser = RustWeek2025;
        let date = NaiveDate::from_ymd_opt(2025, 5, 13).unwrap();

        let mut seen = HashSet::new();

        // First parse — should find one entry
        let entries1 = parser
            .collect_talk_entries(&document, date, &mut seen)
            .unwrap();
        assert_eq!(entries1.len(), 1);

        // Second parse with same seen set — should find zero (deduplicated)
        let entries2 = parser
            .collect_talk_entries(&document, date, &mut seen)
            .unwrap();
        assert_eq!(entries2.len(), 0);
    }

    #[test]
    fn test_fallback_talk() {
        let entry = TalkEntry {
            _slug: "alex".to_string(),
            url: "https://2025.rustweek.org/talks/alex/".to_string(),
            title: "10 Years of Rust: Why?".to_string(),
            speakers: vec!["Alex Crichton".to_string()],
            date: NaiveDate::from_ymd_opt(2025, 5, 13).unwrap(),
        };

        let parsed = entry.to_fallback_talk().unwrap();
        assert_eq!(parsed.talk.title, "10 Years of Rust: Why?");
        assert_eq!(parsed.talk.conference, "RustWeek");
        assert_eq!(parsed.speakers.len(), 1);
        assert_eq!(parsed.speakers[0].name, "Alex Crichton");
    }

    #[test]
    fn test_skips_non_talk_links() {
        let html = r#"
        <html><body>
        <div class="schedule">
            <!-- Workshop link (not /talks/) -->
            <div class="item yellow" id="ws1">
                <div class="content">
                    <div class="block">
                        <a class="title" href="/workshops/embedded">Embedded Workshop</a>
                        <a class="speaker" href="/workshops/embedded"><p>James Munns</p></a>
                    </div>
                </div>
            </div>

            <!-- Proper talk -->
            <div class="item yellow" id="alex">
                <div class="content">
                    <div class="block">
                        <a class="title" href="/talks/alex">10 Years of Rust</a>
                        <a class="speaker" href="/talks/alex"><p>Alex Crichton</p></a>
                    </div>
                </div>
            </div>
        </div>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let parser = RustWeek2025;
        let date = NaiveDate::from_ymd_opt(2025, 5, 13).unwrap();
        let mut seen = HashSet::new();
        let entries = parser
            .collect_talk_entries(&document, date, &mut seen)
            .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "10 Years of Rust");
    }

    #[test]
    fn test_industry_track_items_included() {
        let html = r#"
        <html><body>
        <div class="schedule">
            <div class="item red striped" id="niko">
                <div class="content">
                    <div class="block">
                        <a class="title" href="/talks/niko">Our Vision for Rust</a>
                        <a class="speaker" href="/talks/niko"><p>Niko Matsakis</p></a>
                    </div>
                    <div class="ticket">Industry ticket required</div>
                </div>
            </div>
        </div>
        </body></html>
        "#;

        let document = Html::parse_document(html);
        let parser = RustWeek2025;
        let date = NaiveDate::from_ymd_opt(2025, 5, 13).unwrap();
        let mut seen = HashSet::new();
        let entries = parser
            .collect_talk_entries(&document, date, &mut seen)
            .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Our Vision for Rust");
        assert_eq!(entries[0].speakers, vec!["Niko Matsakis"]);
    }
}
