use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use scraper::Html;
use tracing::{debug, info};
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

impl RustFestEuParser {
    fn subdomain(&self) -> String {
        match (self.year, self.city.to_lowercase().as_str()) {
            (2016, "berlin") => "2016".to_string(),
            (2017, "kyiv") => "2017".to_string(),
            _ => self.city.to_lowercase(),
        }
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
            url: Url::parse(&format!("https://{}.rustfest.eu/talks/", self.subdomain())).unwrap(),
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
        let base_url = Url::parse(&format!("https://{}.rustfest.eu", self.subdomain())).unwrap();

        self.parse_schedule(&document, &base_url)
    }
}

impl RustFestEuParser {
    fn parse_schedule(&self, document: &Html, base_url: &Url) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        let li_selector = css("ul.talks > li")?;
        let title_selector = css("h2.title")?;
        let a_selector = css("a")?;
        let name_selector = css(".name")?;

        for li in document.select(&li_selector) {
            let title_el = match li.select(&title_selector).next() {
                Some(el) => el,
                None => continue,
            };

            let a_el = title_el.select(&a_selector).next();

            let mut speakers_list = Vec::new();
            let talk_title;
            let mut href = String::new();

            if let Some(a) = a_el {
                let full_title = text(a).trim().to_string();
                if full_title.is_empty() {
                    continue;
                }

                href = a.value().attr("href").unwrap_or("").to_string();

                let mut speaker_str = String::new();
                if let Some((speaker, title)) = full_title.split_once(" – ") {
                    speaker_str = speaker.trim().to_string();
                    talk_title = title.trim().to_string();
                } else if let Some((speaker, title)) = full_title.split_once(" - ") {
                    speaker_str = speaker.trim().to_string();
                    talk_title = title.trim().to_string();
                } else {
                    talk_title = full_title;
                }
                if !speaker_str.is_empty() {
                    speakers_list.push(speaker_str);
                }
            } else {
                let mut title_text = text(title_el).trim().to_string();
                if title_text.to_lowercase().starts_with("keynote ") {
                    title_text = title_text[8..].trim().to_string();
                } else if title_text.to_lowercase().starts_with("special guest ") {
                    title_text = title_text[14..].trim().to_string();
                }

                if title_text.is_empty() {
                    continue;
                }
                talk_title = title_text;

                for name_el in li.select(&name_selector) {
                    let n = text(name_el).trim().to_string();
                    if !n.is_empty() {
                        speakers_list.push(n);
                    }
                }

                if let Some(id) = title_el.value().attr("id") {
                    href = format!("#{}", id);
                }
            }

            if talk_title.to_lowercase().contains("master of ceremony") {
                continue;
            }

            let website_url = if href.is_empty() {
                base_url.clone()
            } else if href.starts_with('#') {
                base_url
                    .join(&format!("/talks/{}", href))
                    .map(|u| u.into())
                    .unwrap_or_else(|_| base_url.clone())
            } else {
                base_url
                    .join(&href)
                    .map(|u| u.into())
                    .unwrap_or_else(|_| base_url.clone())
            };

            let mut speakers = Vec::new();
            for s in &speakers_list {
                speakers.push(NewSpeaker { name: s.clone() });
            }

            let summary = if !speakers_list.is_empty() {
                format!("Talk by {}", speakers_list.join(", "))
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

        if talks.is_empty() {
            bail!("No talks found in RustFest {} schedule page.", self.year);
        }

        info!("Parsed {} talks from schedule", talks.len());
        Ok(talks)
    }
}
