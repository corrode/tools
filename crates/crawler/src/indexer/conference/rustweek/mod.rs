//! RustWeek / RustNL schedule parsers.
//!
//! This conference series was originally called "RustNL" (2023, 2024) and
//! rebranded to "RustWeek" starting in 2025. All editions are grouped here.
//!
//! - 2023 (Amsterdam): single-day, single-track, HTML table on main page.
//! - 2024 (Delft): two-day, single-track, HTML list on `/schedule/`.
//! - 2025 (Utrecht): two-day, multi-track, CSS-grid schedule on `/schedule/tuesday` and `/schedule/wednesday`,
//!   with individual talk pages at `/talks/{slug}/`.

mod rustnl2023;
mod rustnl2024;
mod rustweek2025;

pub use rustnl2023::RustNL2023;
pub use rustnl2024::RustNL2024;
pub use rustweek2025::RustWeek2025;

use anyhow::{Context, Result};
use log::debug;
use scraper::Html;
use types::{NewSpeaker, NewTalk};

use super::ParsedTalk;
use crate::tools::css::{css, normalize_whitespace, text};

/// Items to skip when parsing the schedule — these are not talks.
const SKIP_TITLES: &[&str] = &[
    "registration",
    "doors open",
    "opening",
    "introduction",
    "break",
    "lunch",
    "drinks",
    "outro",
    "closing",
    "badge",
    "reception",
    "expert tables",
    "hackathon",
    "workshop",
    "book signing",
];

/// Returns `true` if the given title represents a non-talk schedule item that
/// should be skipped.
pub fn should_skip(title: &str) -> bool {
    let lower = title.to_lowercase();
    SKIP_TITLES.iter().any(|&skip| lower.contains(skip))
}

/// Parse a RustWeek 2025 talk detail page.
///
/// Returns `(title, summary, speakers)` extracted from the page.
///
/// HTML structure:
/// ```text
/// <div class="kicker">Talk</div>
/// <h1>Talk Title</h1>
/// <p>by Speaker Name</p>
/// <!-- optional abstract paragraphs between h1 and "Audience:" -->
/// <strong>Audience:</strong> ...
/// <h3>Speaker(s)</h3>
/// <h4>Speaker Name 1</h4>
/// <h4>Speaker Name 2</h4>
/// ```
pub fn parse_talk_detail_page(html: &str) -> Result<(String, String, Vec<String>)> {
    let document = Html::parse_document(html);
    let root = document.root_element();

    // Title from <h1>
    let h1_sel = css("h1")?;
    let title = root
        .select(&h1_sel)
        .next()
        .map(|el| text(el))
        .unwrap_or_default();

    // Speakers from <h4> elements (inside the Speaker(s) section)
    let h4_sel = css("h4")?;
    let speakers: Vec<String> = root
        .select(&h4_sel)
        .map(|el| text(el))
        .filter(|name| !name.is_empty() && *name != title)
        .collect();

    // Abstract: text content between </h1> and "Audience:" or "Speaker"
    // We look for the raw HTML between these markers.
    let summary = extract_abstract_text(html, &title);

    Ok((title, summary, speakers))
}

/// Extract the abstract text from the raw HTML of a talk detail page.
///
/// The abstract is the prose text between the title/byline and the
/// "Audience:" or "Speaker" section. We strip HTML tags and normalize
/// whitespace.
fn extract_abstract_text(html: &str, _title: &str) -> String {
    // Find the end of the <h1> tag (after the title)
    let h1_end = match html.find("</h1>") {
        Some(idx) => idx + 5,
        None => return String::new(),
    };

    // Find the start of either "Audience:" or "Speaker" section
    let section_start = html[h1_end..]
        .find("Audience:")
        .or_else(|| html[h1_end..].find(">Speaker"))
        .map(|idx| h1_end + idx)
        .unwrap_or(html.len());

    let between = &html[h1_end..section_start];

    // Strip HTML tags, decode basic entities, normalize whitespace
    let text = between
        .replace("<br>", " ")
        .replace("<br/>", " ")
        .replace("<br />", " ");

    // Remove all remaining HTML tags
    let mut result = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(ch);
        }
    }

    // Decode common HTML entities
    let result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&middot;", "·")
        .replace("&#160;", " ")
        .replace("&nbsp;", " ");

    let result = normalize_whitespace(&result);

    // Remove the "by Speaker Name" prefix that appears after the title
    let result = if let Some(stripped) = result.strip_prefix("by ") {
        // The byline is typically "by Speaker Name" — remove it if it's short
        // (single line, no periods) or if it doesn't look like abstract text
        if !stripped.contains('.') && stripped.len() < 100 {
            String::new()
        } else {
            // The byline might be followed by the actual abstract
            stripped
                .find('.')
                .map(|_| stripped.to_string())
                .unwrap_or_default()
        }
    } else {
        result
    };

    // Skip if the remaining text is just navigation or noise
    if result.len() < 30 {
        return String::new();
    }

    result
}

/// Fetch and parse a single RustWeek 2025 talk detail page,
/// returning a [`ParsedTalk`].
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
    fn test_should_skip() {
        assert!(should_skip("Registration Opens"));
        assert!(should_skip("Lunch Break"));
        assert!(should_skip("Break"));
        assert!(should_skip("Drinks!"));
        assert!(should_skip("Opening"));
        assert!(should_skip("Outro"));
        assert!(should_skip("Expert Tables Round 1"));
        assert!(!should_skip("10 Years of Rust: Why?"));
        assert!(!should_skip("Faster, easier 2D vector rendering"));
    }

    #[test]
    fn test_parse_talk_detail_page_single_speaker() {
        let html = r#"
        <html><body>
        <div class="kicker">Talk</div>
        <h1>Faster, easier 2D vector rendering</h1>
        <p>by Raph Levien</p>
        <p>This talk presents new work on high performance vector rendering.</p>
        <strong>Audience:</strong> All
        <h3>Speaker</h3>
        <h4>Raph Levien</h4>
        <p>Research engineer at Google</p>
        </body></html>
        "#;

        let (title, summary, speakers) = parse_talk_detail_page(html).unwrap();
        assert_eq!(title, "Faster, easier 2D vector rendering");
        assert_eq!(speakers, vec!["Raph Levien"]);
        assert!(!summary.is_empty());
    }

    #[test]
    fn test_parse_talk_detail_page_multi_speaker() {
        let html = r#"
        <html><body>
        <div class="kicker">Talk</div>
        <h1>Ariel OS</h1>
        <p>by Emmanuel Baccelli &amp; Koen Zandberg</p>
        <strong>Audience:</strong> All
        <h3>Speakers</h3>
        <h4>Emmanuel Baccelli</h4>
        <h4>Koen Zandberg</h4>
        </body></html>
        "#;

        let (title, _, speakers) = parse_talk_detail_page(html).unwrap();
        assert_eq!(title, "Ariel OS");
        assert_eq!(speakers, vec!["Emmanuel Baccelli", "Koen Zandberg"]);
    }
}
