//! RustConf 2024 schedule parser.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use log::{debug, info};
use scraper::{Html, Selector};
use std::sync::LazyLock;
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{
    ConferenceMetadata, ParsedTalk, ScheduleParser, base_url, static_url,
};

/// Parser for RustConf 2024
pub struct RustConf2024;

static RUSTCONF_2024_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://2024.rustconf.com"));
static RUSTCONF_2024_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=wTV0WCLERGg&list=PL2b0df3jKKiTWZeF7cip6ZUsaVXxWioRi",
    )
});

#[async_trait]
impl ScheduleParser for RustConf2024 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustconf-2024",
            conference: "RustConf",
            year: "2024",
            url: (*RUSTCONF_2024_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTCONF_2024_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let base_url = base_url(&self.metadata().url)?;
        let schedule_url = base_url
            .join("schedule")
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

        // RustConf 2024: September 10-13, main conference Sep 11-12
        let date = NaiveDate::from_ymd_opt(2024, 9, 11).context("Invalid date")?;

        self.parse_schedule(&document, date, &base_url)
    }
}

impl RustConf2024 {
    fn parse_schedule(
        &self,
        document: &Html,
        date: NaiveDate,
        base_url: &Url,
    ) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        // The 2024 site uses <li> elements for schedule items
        // Each talk has a structure like:
        // - Time range
        // - Title (in heading)
        // - Type badge (Session, Keynote, Workshop, etc.)
        // - Description paragraph
        // - Speaker name(s) in <h5>

        // Try to find schedule list items
        let li_selector = Selector::parse("li")
            .map_err(|e| anyhow::anyhow!("Failed to parse li selector: {:?}", e))?;

        let h5_selector = Selector::parse("h5")
            .map_err(|e| anyhow::anyhow!("Failed to parse h5 selector: {:?}", e))?;

        let p_selector = Selector::parse("p")
            .map_err(|e| anyhow::anyhow!("Failed to parse p selector: {:?}", e))?;

        for li in document.select(&li_selector) {
            let text = li.text().collect::<String>();

            // Skip non-talk items
            if !text.contains("Session") && !text.contains("Keynote") {
                continue;
            }

            // Skip meals, breaks, registration, etc.
            let lower = text.to_lowercase();
            if lower.contains("lunch")
                || lower.contains("break")
                || lower.contains("snack")
                || lower.contains("registration")
                || lower.contains("reception")
                || lower.contains("meal")
                || lower.contains("badge")
            {
                continue;
            }

            // Extract speaker names from h5 tags
            let speakers: Vec<String> = li
                .select(&h5_selector)
                .map(|el| el.text().collect::<String>().trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            // Skip if no speakers (probably not a talk)
            if speakers.is_empty() {
                continue;
            }

            // Extract description from p tags
            let description: String = li
                .select(&p_selector)
                .map(|el| el.text().collect::<String>().trim().to_string())
                .filter(|s| !s.is_empty() && s.len() > 50) // Filter out short non-description text
                .collect::<Vec<_>>()
                .join(" ");

            // Try to extract title - it's typically in the text before "Session" or "Keynote"
            // and after the time range
            let title = self.extract_title(&text)?;

            if title.is_empty() {
                debug!("Skipping item with empty title");
                continue;
            }

            debug!(
                "Parsed RustConf 2024 talk candidate: {} ({} speakers)",
                title,
                speakers.len()
            );

            let slug = super::slugify(&title);
            let website_url = base_url
                .join(&format!("schedule#{}", slug))
                .with_context(|| format!("Invalid URL for talk: {}", title))?;

            let talk = NewTalk {
                title: title.clone(),
                summary: if description.is_empty() {
                    // Use a placeholder if no description found
                    format!("Talk by {}", speakers.join(", "))
                } else {
                    description
                },
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

        if talks.is_empty() {
            bail!(
                "No talks found in schedule page. HTML length: {} chars. \
                 The page structure may have changed.",
                document.html().len()
            );
        }

        info!("Parsed {} talks from schedule", talks.len());
        Ok(talks)
    }

    fn extract_title(&self, text: &str) -> Result<String> {
        // The title appears after the time range and before the type badge
        // Example: "10:05 am To10:25 am Making Open Source Secure by Design Keynote"
        // Or: "10:05 amTo10:25 amMaking Open Source..."

        // Split by common type badges to find title
        let type_markers = [
            "Session",
            "Keynote",
            "Workshop",
            "Emcee/Remarks",
            "Sponsored",
        ];

        for marker in type_markers {
            if let Some(idx) = text.find(marker) {
                // Get text before the marker
                let before = &text[..idx];

                // Find the time pattern: we need to find the SECOND occurrence of am/pm
                // Pattern is like "10:05 am To 10:25 am" or "10:05am To10:25am"
                // The title starts after the second am/pm

                let mut last_time_end = 0;
                let lower = before.to_lowercase();

                // Find all occurrences of "am" and "pm" that follow a digit (time pattern)
                for (i, _) in lower.match_indices("am") {
                    // Check if preceded by a digit (part of time)
                    if i > 0
                        && before[..i]
                            .chars()
                            .last()
                            .map(|c| c.is_ascii_digit() || c.is_whitespace())
                            .unwrap_or(false)
                    {
                        last_time_end = i + 2;
                    }
                }
                for (i, _) in lower.match_indices("pm") {
                    if i > 0
                        && before[..i]
                            .chars()
                            .last()
                            .map(|c| c.is_ascii_digit() || c.is_whitespace())
                            .unwrap_or(false)
                        && i + 2 > last_time_end
                    {
                        last_time_end = i + 2;
                    }
                }

                let title = before[last_time_end..].trim();

                // Clean up any remaining whitespace/newlines
                let title = title
                    .lines()
                    .map(|l| l.trim())
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");

                if !title.is_empty() {
                    return Ok(title);
                }
            }
        }

        Ok(String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustconf_2024_metadata() {
        let parser = RustConf2024;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustconf-2024");
        assert_eq!(metadata.conference, "RustConf");
        assert_eq!(metadata.year, "2024");
        assert_eq!(
            metadata.url,
            Url::parse("https://2024.rustconf.com").expect("valid RustConf 2024 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=wTV0WCLERGg&list=PL2b0df3jKKiTWZeF7cip6ZUsaVXxWioRi"
                )
                .expect("valid RustConf 2024 playlist URL")
            )
        );
    }

    #[test]
    fn test_extract_title_2024() {
        let parser = RustConf2024;

        let text =
            "10:05 am To10:25 am Making Open Source Secure by Design Keynote Track: Main(AM)";
        let title = parser.extract_title(text).unwrap();
        assert_eq!(title, "Making Open Source Secure by Design");

        let text2 = "10:55 am To11:20 am Eternal Sunshine of the Rustfmt'ed Mind Session Track: 1";
        let title2 = parser.extract_title(text2).unwrap();
        assert_eq!(title2, "Eternal Sunshine of the Rustfmt'ed Mind");
    }
}
