//! Generic FOSDEM Rust devroom schedule parser.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use log::{debug, info};
use scraper::Html;
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{ConferenceMetadata, ParsedTalk, ScheduleParser, static_url};
use crate::tools::css::{css, text};

/// Parser for FOSDEM Rust devroom (supports multiple years)
pub struct FosdemParser {
    year: u32,
    date: NaiveDate,
}

impl FosdemParser {
    pub fn new(year: u32, date: NaiveDate) -> Self {
        Self { year, date }
    }
}

#[async_trait]
impl ScheduleParser for FosdemParser {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            // Need to use leaked strings here to satisfy the 'static lifetime requirements
            // of the ConferenceMetadata struct.
            id: Box::leak(format!("fosdem-{}", self.year).into_boxed_str()),
            conference: "FOSDEM",
            year: Box::leak(self.year.to_string().into_boxed_str()),
            // Older years use archive.fosdem.org
            url: if self.year < 2024 {
                static_url(Box::leak(
                    format!("https://archive.fosdem.org/{}/", self.year).into_boxed_str(),
                ))
            } else {
                static_url("https://fosdem.org/")
            },
            youtube_playlist_url: None,
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let base_url = self.metadata().url;
        let schedule_url = base_url
            .join(&format!("{}/schedule/track/rust/", self.year))
            .context("Failed to build schedule URL")?;

        info!("Fetching schedule from: {}", schedule_url);

        let response = client
            .get(schedule_url.to_string())
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

        self.parse_schedule(&document, &base_url)
    }
}

impl FosdemParser {
    fn parse_schedule(&self, document: &Html, base_url: &Url) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        let row_selector = css("table.table-striped tbody tr")?;
        let td_selector = css("td")?;
        let a_selector = css("a")?;

        let date = self.date;

        for row in document.select(&row_selector) {
            let tds: Vec<_> = row.select(&td_selector).collect();
            // Expected table structure:
            // [0]=Room/Empty, [1]=Event Title/Link, [2]=Speakers, [3]=Start, [4]=End
            if tds.len() >= 4 {
                let title_td = tds[1];
                let speakers_td = tds[2];

                let title = text(title_td).trim().to_string();
                if title.is_empty() {
                    continue;
                }

                let href = title_td
                    .select(&a_selector)
                    .next()
                    .and_then(|a| a.value().attr("href"))
                    .unwrap_or("");

                let speakers_str = text(speakers_td).trim().to_string();
                let mut speakers = Vec::new();

                for speaker_name in speakers_str.split(',') {
                    let s = speaker_name.trim();
                    if !s.is_empty() {
                        speakers.push(NewSpeaker {
                            name: s.to_string(),
                        });
                    }
                }

                let website_url = if href.is_empty() {
                    base_url
                        .join(&format!("{}/schedule/track/rust/", self.year))
                        .context("Failed to build fallback URL")?
                } else {
                    base_url
                        .join(href)
                        .with_context(|| format!("Invalid URL for talk: {}", href))?
                };

                let speaker_names = speakers
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");

                let summary = if speaker_names.is_empty() {
                    title.clone()
                } else {
                    format!("Talk by {}", speaker_names)
                };

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

                debug!(
                    "Parsed FOSDEM {} talk: {} ({} speakers)",
                    self.year,
                    title,
                    speakers.len()
                );
                talks.push(ParsedTalk { talk, speakers });
            }
        }

        if talks.is_empty() {
            bail!("No talks found in FOSDEM {} schedule page.", self.year);
        }

        info!("Parsed {} talks from schedule", talks.len());
        Ok(talks)
    }
}
