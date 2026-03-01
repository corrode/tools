use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use log::{debug, info};
use scraper::Html;
use types::{NewSpeaker, NewTalk, Url};

use crate::indexer::conference::{ConferenceMetadata, ParsedTalk, ScheduleParser};
use crate::tools::css::{css, text};

/// Parser for historical RustFest EU events (Zurich, Paris, Rome, Barcelona, etc)
pub struct RustFestEuParser {
    year: u32,
    city: &'static str,
    date: NaiveDate,
}

impl RustFestEuParser {
    pub fn new(year: u32, city: &'static str, date: NaiveDate) -> Self {
        Self { year, city, date }
    }
}

#[async_trait]
impl ScheduleParser for RustFestEuParser {
    fn metadata(&self) -> ConferenceMetadata {
        let city_lower = self.city.to_lowercase();
        ConferenceMetadata {
            // Need to use leaked strings here to satisfy the 'static lifetime requirements
            // of the ConferenceMetadata struct. This is fine since it's instantiated once on startup.
            id: Box::leak(format!("rustfest-{}-{}", city_lower, self.year).into_boxed_str()),
            conference: "RustFest",
            year: Box::leak(self.year.to_string().into_boxed_str()),
            url: Url::parse(&format!("https://{}.rustfest.eu/talks/", city_lower)).unwrap(),
            youtube_playlist_url: None,
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let metadata = self.metadata();

        info!("Fetching schedule from: {}", metadata.url);

        let response = client
            .get(metadata.url.to_string())
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
        let base_url =
            Url::parse(&format!("https://{}.rustfest.eu", self.city.to_lowercase())).unwrap();

        self.parse_schedule(&document, &base_url)
    }
}

impl RustFestEuParser {
    fn parse_schedule(&self, document: &Html, base_url: &Url) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        let title_selector = css("h2.title")?;
        let a_selector = css("a")?;

        for title_el in document.select(&title_selector) {
            let a_el = title_el.select(&a_selector).next();
            if let Some(a) = a_el {
                let full_title = text(a).trim().to_string();
                if full_title.is_empty() {
                    continue;
                }

                let href = a.value().attr("href").unwrap_or("");
                let website_url = base_url
                    .join(href)
                    .map(|u| u.into())
                    .unwrap_or_else(|_| base_url.clone());

                let mut speaker_name = String::new();
                let mut talk_title = full_title.clone();

                // Common format: "Speaker Name – Talk Title" or "Speaker Name - Talk Title"
                if let Some((speaker, title)) = full_title.split_once(" – ") {
                    speaker_name = speaker.trim().to_string();
                    talk_title = title.trim().to_string();
                } else if let Some((speaker, title)) = full_title.split_once(" - ") {
                    speaker_name = speaker.trim().to_string();
                    talk_title = title.trim().to_string();
                }

                // Skip Master of Ceremony and other non-talk events
                if talk_title.to_lowercase().contains("master of ceremony") {
                    continue;
                }

                let mut speakers = Vec::new();
                if !speaker_name.is_empty() {
                    speakers.push(NewSpeaker {
                        name: speaker_name.clone(),
                    });
                }

                let summary = if !speaker_name.is_empty() {
                    format!("Talk by {}", speaker_name)
                } else {
                    talk_title.clone()
                };

                let talk = NewTalk {
                    title: talk_title.clone(),
                    summary,
                    transcript: None,
                    conference: self.metadata().conference.to_string(),
                    date: self.date,
                    website_url,
                    video_url: None,
                    slides_url: None,
                    thumbnail_url: None,
                    duration_seconds: None,
                };

                debug!(
                    "Parsed RustFest {} talk: {} ({} speakers)",
                    self.year,
                    talk_title,
                    speakers.len()
                );
                talks.push(ParsedTalk { talk, speakers });
            }
        }

        if talks.is_empty() {
            bail!("No talks found in RustFest {} schedule page.", self.year);
        }

        info!("Parsed {} talks from schedule", talks.len());
        Ok(talks)
    }
}
