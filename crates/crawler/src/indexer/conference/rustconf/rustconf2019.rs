//! RustConf 2019 schedule parser.
//!
//! The schedule is a static HTML table at `/schedule/`.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::NaiveDate;
use log::{debug, info};
use scraper::Html;
use std::sync::LazyLock;
use types::{NewSpeaker, NewTalk, Url};
use url::Url as UrlLib;

use crate::indexer::conference::{
    ConferenceMetadata, ParsedTalk, ScheduleParser, base_url, static_url,
};
use crate::tools::css::{css, select_attr, select_text, text};

/// Parser for RustConf 2019
pub struct RustConf2019;

static RUSTCONF_2019_BASE_URL: LazyLock<Url> =
    LazyLock::new(|| static_url("https://2019.rustconf.com"));
static RUSTCONF_2019_PLAYLIST_URL: LazyLock<Url> = LazyLock::new(|| {
    static_url(
        "https://www.youtube.com/watch?v=FSrQX4uYuOM&list=PL85XCvVPmGQhDOUIZBe6u388GydeACbTt",
    )
});

#[async_trait]
impl ScheduleParser for RustConf2019 {
    fn metadata(&self) -> ConferenceMetadata {
        ConferenceMetadata {
            id: "rustconf-2019",
            conference: "RustConf",
            year: "2019",
            url: (*RUSTCONF_2019_BASE_URL).clone(),
            youtube_playlist_url: Some((*RUSTCONF_2019_PLAYLIST_URL).clone()),
        }
    }

    async fn parse(&self, client: &reqwest::Client) -> Result<Vec<ParsedTalk>> {
        let base_url = base_url(&self.metadata().url)?;
        let schedule_url = base_url
            .join("schedule/")
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

        // RustConf 2019: conference day is Aug 23
        let date = NaiveDate::from_ymd_opt(2019, 8, 23).context("Invalid date")?;

        self.parse_schedule(&document, date, &base_url)
    }
}

impl RustConf2019 {
    fn parse_schedule(
        &self,
        document: &Html,
        date: NaiveDate,
        base_url: &Url,
    ) -> Result<Vec<ParsedTalk>> {
        let mut talks = Vec::new();

        debug!("Parsing RustConf 2019 schedule table");

        let row_selector = css("table.card tbody tr")?;
        let session_selector = css("td.session")?;
        let title_selector = css("p")?;
        let speaker_selector = css("ul.inline-list li a")?;
        let link_selector = css("a")?;

        let schedule_base: Url = base_url
            .join("schedule/")
            .context("Failed to build schedule base URL")?
            .into();

        for row in document.select(&row_selector) {
            for session in row.select(&session_selector) {
                let title = match select_text(session, &title_selector) {
                    Some(t) => t,
                    None => continue,
                };

                if should_skip_title(&title) {
                    continue;
                }

                let speakers: Vec<String> = session
                    .select(&speaker_selector)
                    .map(|el| text(el))
                    .filter(|s| !s.is_empty())
                    .collect();

                if speakers.is_empty() {
                    debug!("Skipping session without speakers: {}", title);
                    continue;
                }

                let website_url = if let Some(href) = select_attr(session, &link_selector, "href") {
                    Self::resolve_url(base_url, &schedule_base, href)
                        .with_context(|| format!("Invalid URL for talk: {}", title))?
                } else {
                    base_url
                        .join(&format!("schedule/#{}", super::slugify(&title)))
                        .with_context(|| format!("Invalid URL for talk: {}", title))?
                };

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
                    .collect::<Vec<_>>();

                talks.push(ParsedTalk {
                    talk,
                    speakers: speaker_list,
                });
            }
        }

        if talks.is_empty() {
            bail!(
                "No talks found in RustConf 2019 schedule page. HTML length: {} chars.",
                document.html().len()
            );
        }

        info!("Parsed {} talks from schedule", talks.len());
        Ok(talks)
    }

    fn resolve_url(base_url: &Url, schedule_base: &Url, href: &str) -> Result<UrlLib> {
        if href.starts_with("http://") || href.starts_with("https://") {
            UrlLib::parse(href).with_context(|| format!("Invalid URL: {}", href))
        } else if href.starts_with("//") {
            UrlLib::parse(&format!("{}:{}", base_url.scheme(), href))
                .with_context(|| format!("Invalid scheme-relative URL: {}", href))
        } else if href.starts_with('#') {
            schedule_base
                .join(href)
                .with_context(|| format!("Invalid URL fragment: {}", href))
        } else if href.starts_with('/') {
            base_url
                .join(href)
                .with_context(|| format!("Invalid URL path: {}", href))
        } else {
            schedule_base
                .join(href)
                .with_context(|| format!("Invalid URL path: {}", href))
        }
    }
}

fn should_skip_title(title: &str) -> bool {
    let lower = title.to_lowercase();
    lower.contains("registration")
        || lower.contains("break")
        || lower.contains("lunch")
        || lower.contains("reception")
        || lower.contains("closing")
        || lower.contains("opening")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rustconf_2019_metadata() {
        let parser = RustConf2019;
        let metadata = parser.metadata();
        assert_eq!(metadata.id, "rustconf-2019");
        assert_eq!(metadata.conference, "RustConf");
        assert_eq!(metadata.year, "2019");
        assert_eq!(
            metadata.url,
            Url::parse("https://2019.rustconf.com").expect("valid RustConf 2019 base URL")
        );
        assert_eq!(
            metadata.youtube_playlist_url,
            Some(
                Url::parse(
                    "https://www.youtube.com/watch?v=FSrQX4uYuOM&list=PL85XCvVPmGQhDOUIZBe6u388GydeACbTt"
                )
                .expect("valid RustConf 2019 playlist URL")
            )
        );
    }
}
