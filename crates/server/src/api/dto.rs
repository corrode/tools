//! Public DTOs returned by the JSON API.
//!
//! These types are intentionally separate from the template view types in
//! `types::search_result`. The view types carry UI concerns (inline SVG
//! icons, `<mark>`-wrapped highlights, human-formatted strings like
//! `~5 min read`) that we deliberately do not bake into a public, versioned
//! API contract.
//!
//! Field naming follows the JSON convention for this API: `snake_case`,
//! ISO-8601 dates serialized as `YYYY-MM-DD` strings via chrono, raw
//! durations in seconds, and URLs as plain strings.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_precision_loss
)]

use chrono::NaiveDate;
use serde::Serialize;
use types::{
    ArxivCategory, ContentType, PodcastEpisode, SearchEntry, SearchResult, params::SortOrder,
};
use utoipa::ToSchema;

/// Top-level response from `GET /api/v1/search`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct SearchResponse {
    /// Echo of the user's trimmed query, if any.
    #[schema(example = "async runtime")]
    pub query: Option<String>,
    /// Total number of matching results across all pages.
    #[schema(example = 142)]
    pub total: i64,
    /// Number of results in the current page (`<= per_page`).
    #[schema(example = 20)]
    pub returned: usize,
    /// 1-based current page number, clamped to the last valid page.
    #[schema(example = 1)]
    pub page: u32,
    /// Fixed page size. Mirrors `Repository::RESULTS_PER_PAGE`.
    #[schema(example = 20)]
    pub per_page: u32,
    /// Total number of pages available for this query.
    #[schema(example = 8)]
    pub total_pages: u32,
    /// Applied content type filter, if any.
    pub content_type: Option<ContentType>,
    /// Applied sort order. Echoed back; defaults to `relevance`.
    pub sort_by: SortOrder,
    /// Server-side wall-clock duration for this request, in milliseconds.
    #[schema(example = 17)]
    pub took_ms: u64,
    /// Result hits on the current page, in ranked order.
    pub results: Vec<SearchHit>,
}

/// One search result. The `kind` field discriminates the variant.
///
/// In OpenAPI this renders as a `oneOf` with a `kind` discriminator, so
/// client codegen produces a proper tagged union.
#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SearchHit {
    /// A blog post, RFC, or other written article.
    Article(ArticleHit),
    /// A video (YouTube and similar).
    Video(VideoHit),
    /// A conference or meetup talk.
    Talk(TalkHit),
    /// A podcast episode.
    Podcast(PodcastHit),
    /// An academic research paper.
    Research(ResearchHit),
}

/// Article hit.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ArticleHit {
    /// Article title.
    pub title: String,
    /// Canonical article URL.
    pub url: String,
    /// Host name extracted from `url`, e.g. `blog.rust-lang.org`.
    pub domain: String,
    /// Publication date in ISO-8601 (`YYYY-MM-DD`).
    pub date: NaiveDate,
    /// Editorial category, e.g. `Blog`, `RFC`.
    pub category: String,
    /// Reference identifier (e.g. RFC number, TWiR issue), if any.
    pub reference: Option<String>,
    /// Estimated reading time in minutes (200 wpm, rounded up).
    pub reading_minutes: Option<u32>,
    /// Word count, if known.
    pub word_count: Option<i64>,
    /// FTS snippet with `<mark>...</mark>` highlights, if available.
    pub snippet: Option<String>,
    /// FTS relevance rank. Lower is better. Comparable only within one
    /// response.
    pub rank: f64,
}

/// Video hit.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct VideoHit {
    /// Video title.
    pub title: String,
    /// Canonical video URL.
    pub url: String,
    /// Host name extracted from `url`, if parseable.
    pub domain: Option<String>,
    /// Publication date in ISO-8601 (`YYYY-MM-DD`).
    pub date: NaiveDate,
    /// Thumbnail URL, if available.
    pub thumbnail_url: Option<String>,
    /// Duration in seconds, if known.
    pub duration_seconds: Option<u32>,
    /// FTS snippet with `<mark>...</mark>` highlights, if available.
    pub snippet: Option<String>,
    /// FTS relevance rank. Lower is better.
    pub rank: f64,
}

/// Conference talk hit.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TalkHit {
    /// Talk title.
    pub title: String,
    /// Canonical talk URL (usually the conference site).
    pub url: String,
    /// Talk date in ISO-8601 (`YYYY-MM-DD`).
    pub date: NaiveDate,
    /// Conference name (e.g. `RustConf`).
    pub conference: String,
    /// Short editorial summary of the talk.
    pub summary: String,
    /// Recording URL, if a video was published.
    pub video_url: Option<String>,
    /// Slides URL, if slides were published.
    pub slides_url: Option<String>,
    /// Thumbnail URL, if available.
    pub thumbnail_url: Option<String>,
    /// Duration in seconds, if known.
    pub duration_seconds: Option<u32>,
    /// FTS snippet with `<mark>...</mark>` highlights, if available.
    pub snippet: Option<String>,
    /// FTS relevance rank. Lower is better.
    pub rank: f64,
}

/// Podcast episode hit.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct PodcastHit {
    /// Numeric episode identifier. Use with `GET /api/v1/podcasts/{id}` to
    /// fetch the full transcript.
    pub id: i64,
    /// Episode title.
    pub title: String,
    /// Canonical episode URL on the publisher's site.
    pub url: String,
    /// Host name extracted from `url`.
    pub domain: String,
    /// Publication date in ISO-8601 (`YYYY-MM-DD`).
    pub date: NaiveDate,
    /// Name of the show this episode belongs to.
    pub podcast_name: String,
    /// Episode-specific title (often the same as `title`).
    pub episode_name: String,
    /// Optional editorial summary.
    pub summary: Option<String>,
    /// Thumbnail URL, if available.
    pub thumbnail_url: Option<String>,
    /// Duration in seconds, if known.
    pub duration_seconds: Option<u32>,
    /// FTS snippet with `<mark>...</mark>` highlights, if available.
    pub snippet: Option<String>,
    /// FTS relevance rank. Lower is better.
    pub rank: f64,
}

/// Research paper hit.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResearchHit {
    /// Paper title.
    pub title: String,
    /// Canonical paper URL.
    pub url: String,
    /// Host name extracted from `url` (typically `arxiv.org`).
    pub domain: String,
    /// Publication date in ISO-8601 (`YYYY-MM-DD`).
    pub date: NaiveDate,
    /// Comma-separated author list as stored in the index.
    pub authors: String,
    /// Paper abstract.
    pub abstract_text: String,
    /// arXiv subject category (display label).
    #[schema(example = "cs.PL — Programming Languages")]
    pub category: String,
    /// arXiv subject category code (e.g. `cs.PL`).
    #[schema(example = "cs.PL")]
    pub category_code: String,
    /// Paper identifier (e.g. `arXiv:2301.00000`, DOI), if available.
    pub paper_id: Option<String>,
    /// Publication venue, if available.
    pub publication: Option<String>,
    /// FTS snippet with `<mark>...</mark>` highlights, if available.
    pub snippet: Option<String>,
    /// FTS relevance rank. Lower is better.
    pub rank: f64,
}

/// Response body for `GET /api/v1/suggestions`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct SuggestionsResponse {
    /// The trimmed prefix the suggestions were generated for. Empty if `q`
    /// was missing or whitespace.
    #[schema(example = "asyn")]
    pub query: String,
    /// Suggested completion phrases, ordered by descending relevance.
    pub suggestions: Vec<String>,
}

/// Response body for `GET /api/v1/podcasts/{id}`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct PodcastDetail {
    /// Numeric episode identifier.
    pub id: i64,
    /// Episode title (often the same as `episode_name`).
    pub title: String,
    /// Show / podcast name.
    pub podcast_name: String,
    /// Episode name as listed in the feed.
    pub episode_name: String,
    /// Canonical episode URL on the publisher's site.
    pub url: String,
    /// Publication date in ISO-8601 (`YYYY-MM-DD`).
    pub date: NaiveDate,
    /// Episode summary or description.
    pub summary: String,
    /// Thumbnail URL, if available.
    pub thumbnail_url: Option<String>,
    /// Duration in seconds, if known.
    pub duration_seconds: Option<u32>,
    /// Guest names, in feed order.
    pub guests: Vec<String>,
    /// Full raw transcript text. May contain WebVTT-style `<v Speaker>` cues
    /// when speaker labels are available; clients can render as-is or
    /// post-process.
    pub transcript: String,
}

impl PodcastDetail {
    pub(crate) fn from_episode(episode: PodcastEpisode, guests: Vec<String>) -> Self {
        let duration_seconds = episode
            .duration_seconds()
            .and_then(|s| u32::try_from(s.max(0)).ok());
        Self {
            id: episode.id,
            title: episode.title().to_string(),
            podcast_name: episode.podcast_name().to_string(),
            episode_name: episode.episode_name().to_string(),
            url: episode.url().to_string(),
            date: episode.date(),
            summary: episode.summary().to_string(),
            thumbnail_url: episode.thumbnail_url().map(str::to_owned),
            duration_seconds,
            guests,
            transcript: episode.data.transcript,
        }
    }
}

impl SearchHit {
    /// Build a `SearchHit` from a storage-layer `SearchResult`.
    pub(crate) fn from_result(result: SearchResult) -> Self {
        let SearchResult {
            entry,
            rank,
            snippet,
            ..
        } = result;

        match entry {
            SearchEntry::Article(article) => {
                let domain = article
                    .url()
                    .host_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "unknown".to_string());
                let word_count = article.word_count();
                let reading_minutes = if word_count > 0 {
                    let minutes = word_count.div_ceil(200).max(1);
                    Some(u32::try_from(minutes).unwrap_or(u32::MAX))
                } else {
                    None
                };
                Self::Article(ArticleHit {
                    title: article.title().to_string(),
                    url: article.url().to_string(),
                    domain,
                    date: article.date(),
                    category: article.category().to_string(),
                    reference: article.reference().map(str::to_owned),
                    reading_minutes,
                    word_count: i64::try_from(word_count).ok(),
                    snippet,
                    rank,
                })
            }
            SearchEntry::Video(video) => {
                let domain = video.url().host_str().map(str::to_owned);
                let duration_seconds = video
                    .duration_seconds()
                    .and_then(|s| u32::try_from(s.max(0)).ok());
                Self::Video(VideoHit {
                    title: video.title().to_string(),
                    url: video.url().to_string(),
                    domain,
                    date: video.date(),
                    thumbnail_url: video.thumbnail_url().map(str::to_owned),
                    duration_seconds,
                    snippet,
                    rank,
                })
            }
            SearchEntry::Talk(talk) => {
                let duration_seconds = talk
                    .duration_seconds()
                    .and_then(|s| u32::try_from(s.max(0)).ok());
                Self::Talk(TalkHit {
                    title: talk.title().to_string(),
                    url: talk.website_url().to_string(),
                    date: talk.date(),
                    conference: talk.conference().to_string(),
                    summary: talk.summary().to_string(),
                    video_url: talk.video_url().map(str::to_owned),
                    slides_url: talk.slides_url().map(str::to_owned),
                    thumbnail_url: talk.thumbnail_url().map(str::to_owned),
                    duration_seconds,
                    snippet,
                    rank,
                })
            }
            SearchEntry::Podcast(podcast) => {
                let domain = podcast
                    .url()
                    .host_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "unknown".to_string());
                let duration_seconds = podcast
                    .duration_seconds()
                    .and_then(|s| u32::try_from(s.max(0)).ok());
                Self::Podcast(PodcastHit {
                    id: podcast.id,
                    title: podcast.title().to_string(),
                    url: podcast.url().to_string(),
                    domain,
                    date: podcast.date(),
                    podcast_name: podcast.podcast_name().to_string(),
                    episode_name: podcast.episode_name().to_string(),
                    summary: Some(podcast.summary().to_string()).filter(|s| !s.is_empty()),
                    thumbnail_url: podcast.thumbnail_url().map(str::to_owned),
                    duration_seconds,
                    snippet,
                    rank,
                })
            }
            SearchEntry::Research(paper) => {
                let domain = paper
                    .url()
                    .host_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "unknown".to_string());
                let category = ArxivCategory::from_code(paper.category());
                Self::Research(ResearchHit {
                    title: paper.title().to_string(),
                    url: paper.url().to_string(),
                    domain,
                    date: paper.date(),
                    authors: paper.authors().to_string(),
                    abstract_text: paper.abstract_text().to_string(),
                    category: category.to_string(),
                    category_code: paper.category().to_string(),
                    paper_id: paper.paper_id().map(str::to_owned),
                    publication: paper.publication().map(str::to_owned),
                    snippet,
                    rank,
                })
            }
        }
    }
}
