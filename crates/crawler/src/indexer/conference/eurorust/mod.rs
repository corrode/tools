//! EuroRust schedule parsers.
//!
//! Each EuroRust edition has its own parser implementation.
//! - 2022 (Berlin) and 2023 (Brussels): HTML table scraping, no individual talk pages.
//! - 2024 (Vienna) and 2025 (Paris): schedule list scraping + individual talk page fetching.

mod eurorust2022;
mod eurorust2023;
mod eurorust2024;
mod eurorust2025;

pub use eurorust2022::EuroRust2022;
pub use eurorust2023::EuroRust2023;
pub use eurorust2024::EuroRust2024;
pub use eurorust2025::EuroRust2025;

use anyhow::{Context, Result};
use log::debug;
use scraper::Html;
use types::{NewSpeaker, NewTalk};

use super::ParsedTalk;
use crate::tools::css::{css, select_text, text};

/// Slugify a string for use in EuroRust URLs.
///
/// The EuroRust site uses its own slug generation. This is only used as a
/// fallback for editions without individual talk pages (2022, 2023) where we
/// need to fabricate a unique URL fragment.
pub fn slugify(input: &str) -> String {
    input.to_lowercase().replace(' ', "-").replace(
        ['\'', '?', '!', ':', '`', '"', '(', ')', ',', '.', ';', '&'],
        "",
    )
}

/// Strip trailing emoji and other non-ASCII decorations from speaker names.
///
/// EuroRust uses names like "Jon Gjengset 🦀" and "Charlie Marsh 🦀" in the
/// schedule HTML. We strip the trailing non-ASCII noise to get clean names.
pub fn clean_speaker_name(name: &str) -> String {
    let trimmed = name.trim();
    // Strip the leading » character used on 2025 schedule pages
    let trimmed = trimmed.strip_prefix('»').unwrap_or(trimmed).trim();
    // Walk backwards past trailing non-ASCII characters (emoji, variation selectors, ZWJ, etc.)
    let end = trimmed
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_ascii())
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    trimmed[..end].trim().to_string()
}

/// Items to skip when parsing the schedule — these are not talks.
const SKIP_TITLES: &[&str] = &[
    "doors open",
    "breakfast",
    "lunch",
    "snack break",
    "break",
    "closing party",
    "closing dinner",
    "karaoke",
    "group dinner",
    "walking tour",
    "bike tour",
    "wine tasting",
    "cruise tour",
    "meet & eat",
    "rust vienna",
    "rust paris",
    "impl day",
    "unconference",
    "street art tour",
    "meet the rust-programmed",
];

/// Returns `true` if the given title represents a non-talk schedule item that
/// should be skipped.
pub fn should_skip(title: &str) -> bool {
    let lower = title.to_lowercase();
    if lower.starts_with("impl room") {
        return true;
    }
    SKIP_TITLES.iter().any(|&skip| lower.contains(skip))
}

/// Normalize whitespace in a string (collapse runs of whitespace to single spaces).
pub fn normalize_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse the abstract from a EuroRust individual talk page (2024 or 2025).
///
/// Returns `(title, summary, speakers)` extracted from the detail page.
pub fn parse_talk_detail_page(html: &str) -> Result<(String, String, Vec<String>)> {
    let document = Html::parse_document(html);

    // Title: <h1 class="h1 pb-7"> or <h1 class="h1 ...">
    let h1_selector = css("h1")?;
    let title = select_text(document.root_element(), &h1_selector).unwrap_or_default();

    // Abstract: paragraphs inside div.wrapper-content, after the h1.
    // 2024: div.pb-8.mw-70 > p
    // 2025: div (after h1) > p
    // We grab all <p> inside .wrapper-content that are not the title itself.
    let wrapper_selector = css(".wrapper-content")?;
    let p_selector = css("p")?;

    let summary = if let Some(wrapper) = document.select(&wrapper_selector).next() {
        // Collect all <p> text that's inside a <div> sibling of h1 (the abstract container)
        let div_selector = css("div")?;

        let paragraphs: Vec<String> = wrapper
            .select(&div_selector)
            .flat_map(|div| div.select(&p_selector))
            .map(|p| text(p))
            .filter(|t| !t.is_empty() && *t != title)
            .collect();

        paragraphs.join("\n\n")
    } else {
        String::new()
    };

    // Speakers: 2024 uses h2.mb-4 inside .mentor__wrapper-content or .mentor__grid
    //           2025 uses h2.mb-4 inside div.mentor__grid-name
    // Both share the pattern: <h2 class="mb-4">Name</h2> inside the author section.
    let speaker_selector = css("h2.mb-4")?;

    // Filter to h2s that are in the speaker/author section (after <h5>Speaker</h5> or <p class="h5">Speaker</p>)
    let speakers: Vec<String> = document
        .select(&speaker_selector)
        .map(|el| text(el))
        .filter(|name| !name.is_empty() && *name != title)
        .map(|name| clean_speaker_name(&name))
        .filter(|name| !name.is_empty())
        .collect();

    Ok((title, summary, speakers))
}

/// Fetch and parse a single talk detail page, returning a [`ParsedTalk`].
pub async fn fetch_talk_detail(
    client: &reqwest::Client,
    talk_url: &str,
    date: chrono::NaiveDate,
    conference: &str,
) -> Result<Option<ParsedTalk>> {
    debug!("Fetching talk detail page: {}", talk_url);

    let response = client
        .get(talk_url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch talk page: {talk_url}"))?;

    if !response.status().is_success() {
        debug!(
            "Talk page returned HTTP {}: {}",
            response.status(),
            talk_url
        );
        return Ok(None);
    }

    let html = response
        .text()
        .await
        .with_context(|| format!("Failed to read talk page body: {talk_url}"))?;

    let (title, summary, speakers) = parse_talk_detail_page(&html)?;

    if title.is_empty() || speakers.is_empty() {
        debug!("Skipping talk with missing title or speakers: {}", talk_url);
        return Ok(None);
    }

    let summary = if summary.is_empty() {
        format!("Talk by {}", speakers.join(", "))
    } else {
        summary
    };

    let talk = NewTalk {
        title,
        summary,
        transcript: None,
        conference: conference.to_string(),
        date,
        website_url: types::Url::parse(talk_url)
            .with_context(|| format!("Invalid talk URL: {talk_url}"))?,
        video_url: None,
        slides_url: None,
        thumbnail_url: None,
        duration_seconds: None,
    };

    let speaker_list = speakers
        .into_iter()
        .map(|name| NewSpeaker { name })
        .collect();

    Ok(Some(ParsedTalk {
        talk,
        speakers: speaker_list,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_speaker_name_strips_emoji() {
        assert_eq!(clean_speaker_name("Jon Gjengset 🦀"), "Jon Gjengset");
        assert_eq!(clean_speaker_name("Charlie Marsh 🦀"), "Charlie Marsh");
        assert_eq!(
            clean_speaker_name("Alberto Schiabel 🦀"),
            "Alberto Schiabel"
        );
    }

    #[test]
    fn test_clean_speaker_name_strips_leading_marker() {
        assert_eq!(
            clean_speaker_name("»Victoria Brekenfeld"),
            "Victoria Brekenfeld"
        );
        assert_eq!(clean_speaker_name("»Jacob Pratt"), "Jacob Pratt");
    }

    #[test]
    fn test_clean_speaker_name_plain() {
        assert_eq!(clean_speaker_name("Niko Matsakis"), "Niko Matsakis");
        assert_eq!(clean_speaker_name("  Lisa Passing  "), "Lisa Passing");
    }

    #[test]
    fn test_slugify() {
        assert_eq!(
            slugify("Through the Fire and the Flames"),
            "through-the-fire-and-the-flames"
        );
        assert_eq!(
            slugify("I/O in Rust: the whole story"),
            "i/o-in-rust-the-whole-story"
        );
    }

    #[test]
    fn test_should_skip() {
        assert!(should_skip("Doors open & Breakfast"));
        assert!(should_skip("Lunch"));
        assert!(should_skip("Snack Break"));
        assert!(should_skip("Closing Party"));
        assert!(should_skip("impl Room #1"));
        assert!(should_skip("impl Room #2"));
        assert!(!should_skip("Through the Fire and the Flames"));
        assert!(!should_skip(
            "Building an extremely fast Python package manager, in Rust"
        ));
    }

    #[test]
    fn test_normalize_whitespace() {
        assert_eq!(normalize_whitespace("  hello   world  "), "hello world");
        assert_eq!(
            normalize_whitespace("no\nnewlines\there"),
            "no newlines here"
        );
    }
}
