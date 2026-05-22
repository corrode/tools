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
use serde::{Deserialize, Serialize};
use types::{
    Article, ArxivCategory, ContentType, DocumentKind, DocumentRef, PodcastEpisode, ResearchPaper,
    SearchEntry, SearchResult, Talk, Video, params::SortOrder,
};
use utoipa::ToSchema;

use crate::api::passages::{Passage, estimate_tokens};

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
    /// Fixed page size. Mirrors `Repository::RESULTS_PER_PAGE` by default;
    /// reflects the per-request `per_page` override when set.
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
    /// Stable, kind-prefixed identifier (e.g. `article:42`). Use with
    /// `/api/v1/documents/{doc_id}` to fetch the full text.
    pub doc_id: DocumentRef,
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
    /// Short plain-text excerpt of the matching passage, if available. No markup.
    pub snippet: Option<String>,
    /// FTS relevance rank. Lower is better. Comparable only within one
    /// response.
    pub rank: f64,
    /// Additional ranked passages from the article body, populated when the
    /// request was made with `snippets=N`. Empty otherwise.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub passages: Vec<PassageHit>,
}

/// Video hit.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct VideoHit {
    /// Stable, kind-prefixed identifier (e.g. `video:7`).
    pub doc_id: DocumentRef,
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
    /// Short plain-text excerpt of the matching passage, if available. No markup.
    pub snippet: Option<String>,
    /// FTS relevance rank. Lower is better.
    pub rank: f64,
    /// Additional ranked passages from the video transcript, populated when
    /// the request was made with `snippets=N`. Empty otherwise.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub passages: Vec<PassageHit>,
}

/// Conference talk hit.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TalkHit {
    /// Stable, kind-prefixed identifier (e.g. `talk:5`).
    pub doc_id: DocumentRef,
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
    /// Short plain-text excerpt of the matching passage, if available. No markup.
    pub snippet: Option<String>,
    /// FTS relevance rank. Lower is better.
    pub rank: f64,
    /// Additional ranked passages from the talk transcript, populated when
    /// the request was made with `snippets=N`. Empty otherwise.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub passages: Vec<PassageHit>,
}

/// Podcast episode hit.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct PodcastHit {
    /// Stable, kind-prefixed identifier (e.g. `podcast:42`).
    pub doc_id: DocumentRef,
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
    /// Short plain-text excerpt of the matching passage, if available. No markup.
    pub snippet: Option<String>,
    /// FTS relevance rank. Lower is better.
    pub rank: f64,
    /// Additional ranked passages from the transcript, populated when the
    /// request was made with `snippets=N`. Empty otherwise.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub passages: Vec<PassageHit>,
}

/// Research paper hit.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ResearchHit {
    /// Stable, kind-prefixed identifier (e.g. `research:9`).
    pub doc_id: DocumentRef,
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
    /// Short plain-text excerpt of the matching passage, if available. No markup.
    pub snippet: Option<String>,
    /// FTS relevance rank. Lower is better.
    pub rank: f64,
    /// Additional ranked passages from the paper body, populated when the
    /// request was made with `snippets=N`. Empty otherwise.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub passages: Vec<PassageHit>,
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
    /// Builds a `SearchHit` from a storage-layer `SearchResult`.
    pub(crate) fn from_result(result: SearchResult) -> Self {
        Self::from_result_with_passages(result, &[], 0)
    }

    /// Like [`Self::from_result`] but also extracts up to `n_snippets`
    /// additional passages from the underlying document body, using `terms`
    /// as the highlight set. Pass `n_snippets = 0` to skip extraction.
    pub(crate) fn from_result_with_passages(
        result: SearchResult,
        terms: &[&str],
        n_snippets: usize,
    ) -> Self {
        let SearchResult {
            entry,
            rank,
            snippet,
            ..
        } = result;

        // The storage layer wraps FTS matches in `<mark>...</mark>` for the
        // HTML UI. Strip them at the API boundary so JSON consumers (LLMs in
        // particular) get plain text — the query terms are echoed back in
        // the response and the `passages` array still carries char offsets
        // for clients that need precise highlighting.
        let snippet = snippet.map(|s| strip_mark_tags(&s));

        // Pull passages off the entry's text *before* consuming the entry.
        let passages = if n_snippets == 0 || terms.is_empty() {
            Vec::new()
        } else {
            crate::api::passages::extract(entry.text(), terms, n_snippets, 400)
                .into_iter()
                .map(PassageHit::from)
                .collect()
        };

        match entry {
            SearchEntry::Article(article) => {
                let doc_id = DocumentRef::new(DocumentKind::Article, article.id);
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
                    doc_id,
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
                    passages,
                })
            }
            SearchEntry::Video(video) => {
                let doc_id = DocumentRef::new(DocumentKind::Video, video.id);
                let domain = video.url().host_str().map(str::to_owned);
                let duration_seconds = video
                    .duration_seconds()
                    .and_then(|s| u32::try_from(s.max(0)).ok());
                Self::Video(VideoHit {
                    doc_id,
                    title: video.title().to_string(),
                    url: video.url().to_string(),
                    domain,
                    date: video.date(),
                    thumbnail_url: video.thumbnail_url().map(str::to_owned),
                    duration_seconds,
                    snippet,
                    rank,
                    passages,
                })
            }
            SearchEntry::Talk(talk) => {
                let doc_id = DocumentRef::new(DocumentKind::Talk, talk.id);
                let duration_seconds = talk
                    .duration_seconds()
                    .and_then(|s| u32::try_from(s.max(0)).ok());
                Self::Talk(TalkHit {
                    doc_id,
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
                    passages,
                })
            }
            SearchEntry::Podcast(podcast) => {
                let doc_id = DocumentRef::new(DocumentKind::Podcast, podcast.id);
                let domain = podcast
                    .url()
                    .host_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "unknown".to_string());
                let duration_seconds = podcast
                    .duration_seconds()
                    .and_then(|s| u32::try_from(s.max(0)).ok());
                Self::Podcast(PodcastHit {
                    doc_id,
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
                    passages,
                })
            }
            SearchEntry::Research(paper) => {
                let doc_id = DocumentRef::new(DocumentKind::Research, paper.id);
                let domain = paper
                    .url()
                    .host_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "unknown".to_string());
                let category = ArxivCategory::from_code(paper.category());
                Self::Research(ResearchHit {
                    doc_id,
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
                    passages,
                })
            }
        }
    }
}

// =====================================================================
// Document detail (full content) DTOs — used by
// `GET /api/v1/documents/{doc_id}` and `POST /api/v1/documents:batch`.
// =====================================================================

/// Format of a document's textual body, as exposed to clients.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // `Markdown` is reserved for a future crawler change.
pub(crate) enum ContentFormat {
    /// Plain text body, no markup.
    Plain,
    /// Body extracted as Markdown (currently never emitted; reserved).
    Markdown,
    /// WebVTT-style transcript, may include `<v Speaker>` cues.
    Vtt,
}

/// Body of a document, plus cheap LLM-friendly size metadata.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct DocumentContent {
    /// On-the-wire format of `text`. Clients can branch on this when
    /// rendering.
    pub format: ContentFormat,
    /// Number of Unicode characters in `text`.
    pub char_count: u32,
    /// Heuristic token estimate (~chars/4). Useful for budgeting LLM
    /// context windows; not a substitute for a real tokenizer.
    pub token_estimate: u32,
    /// Best-effort language tag, currently always `"en"`. Reserved for
    /// future per-document language detection.
    pub language: Option<String>,
    /// The full body. May be very large for transcripts and papers.
    pub text: String,
}

impl DocumentContent {
    fn new(text: String, format: ContentFormat) -> Self {
        let char_count = u32::try_from(text.chars().count()).unwrap_or(u32::MAX);
        let token_estimate = estimate_tokens(&text);
        Self {
            format,
            char_count,
            token_estimate,
            language: Some("en".to_string()),
            text,
        }
    }
}

/// Full document, returned by [`GET /api/v1/documents/{doc_id}`].
///
/// Tagged union on `kind`, matching [`SearchHit`]. Each variant carries the
/// common identifying fields plus its kind-specific metadata, with the body
/// itself in [`DocumentContent`].
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DocumentDetail {
    /// Full article.
    Article(ArticleDocument),
    /// Full video metadata + transcript/description.
    Video(VideoDocument),
    /// Full conference talk with transcript.
    Talk(TalkDocument),
    /// Full podcast episode with transcript and guest list.
    Podcast(PodcastDocument),
    /// Full research paper.
    Research(ResearchDocument),
}

/// Full article document.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ArticleDocument {
    /// Stable kind-prefixed identifier.
    pub doc_id: DocumentRef,
    /// Article title.
    pub title: String,
    /// Canonical URL.
    pub url: String,
    /// Host name extracted from `url`.
    pub domain: String,
    /// Publication date (`YYYY-MM-DD`).
    pub date: NaiveDate,
    /// Editorial category (e.g. `Blog`, `RFC`).
    pub category: String,
    /// Reference identifier (e.g. RFC number), if any.
    pub reference: Option<String>,
    /// Word count from the crawler, if known.
    pub word_count: Option<i64>,
    /// Estimated reading time in minutes (200 wpm), if word count is known.
    pub reading_minutes: Option<u32>,
    /// Full article body.
    pub content: DocumentContent,
}

/// Full video document.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct VideoDocument {
    /// Stable kind-prefixed identifier.
    pub doc_id: DocumentRef,
    /// Video title.
    pub title: String,
    /// Canonical video URL.
    pub url: String,
    /// Host name, if parseable.
    pub domain: Option<String>,
    /// Publication date.
    pub date: NaiveDate,
    /// Editorial category.
    pub category: String,
    /// Thumbnail URL.
    pub thumbnail_url: Option<String>,
    /// Duration in seconds.
    pub duration_seconds: Option<u32>,
    /// Full transcript or description.
    pub content: DocumentContent,
}

/// Full conference talk document.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct TalkDocument {
    /// Stable kind-prefixed identifier.
    pub doc_id: DocumentRef,
    /// Talk title.
    pub title: String,
    /// Canonical (website) URL.
    pub url: String,
    /// Publication / talk date.
    pub date: NaiveDate,
    /// Conference name.
    pub conference: String,
    /// Editorial summary.
    pub summary: String,
    /// Recording URL, if a video was published.
    pub video_url: Option<String>,
    /// Slides URL, if slides were published.
    pub slides_url: Option<String>,
    /// Thumbnail URL, if available.
    pub thumbnail_url: Option<String>,
    /// Duration in seconds, if known.
    pub duration_seconds: Option<u32>,
    /// Full transcript. `format` is [`ContentFormat::Vtt`] when the
    /// transcript contains `<v Speaker>` cues, otherwise [`ContentFormat::Plain`].
    pub content: DocumentContent,
}

/// Full podcast episode document.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct PodcastDocument {
    /// Stable kind-prefixed identifier.
    pub doc_id: DocumentRef,
    /// Episode title (usually the same as `episode_name`).
    pub title: String,
    /// Show / podcast name.
    pub podcast_name: String,
    /// Episode name as listed in the feed.
    pub episode_name: String,
    /// Canonical episode URL on the publisher's site.
    pub url: String,
    /// Host name extracted from `url`.
    pub domain: String,
    /// Publication date.
    pub date: NaiveDate,
    /// Editorial summary.
    pub summary: String,
    /// Thumbnail URL.
    pub thumbnail_url: Option<String>,
    /// Duration in seconds.
    pub duration_seconds: Option<u32>,
    /// Guest names, in feed order.
    pub guests: Vec<String>,
    /// Full transcript.
    pub content: DocumentContent,
}

/// Full research paper document.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ResearchDocument {
    /// Stable kind-prefixed identifier.
    pub doc_id: DocumentRef,
    /// Paper title.
    pub title: String,
    /// Canonical URL (typically arXiv).
    pub url: String,
    /// Host name.
    pub domain: String,
    /// Publication date.
    pub date: NaiveDate,
    /// Comma-separated author list.
    pub authors: String,
    /// Paper abstract.
    pub abstract_text: String,
    /// arXiv subject category display label.
    pub category: String,
    /// arXiv subject category code (e.g. `cs.PL`).
    pub category_code: String,
    /// Paper identifier (e.g. `arXiv:2301.00000`, DOI), if available.
    pub paper_id: Option<String>,
    /// Publication venue, if available.
    pub publication: Option<String>,
    /// Full paper text.
    pub content: DocumentContent,
}

impl DocumentDetail {
    pub(crate) fn from_article(article: Article) -> Self {
        let doc_id = DocumentRef::new(DocumentKind::Article, article.id);
        let domain = article
            .url()
            .host_str()
            .map(str::to_owned)
            .unwrap_or_else(|| "unknown".to_string());
        let word_count = article.data.word_count;
        let reading_minutes = if word_count > 0 {
            let wc = u64::try_from(word_count).unwrap_or(0);
            let minutes = u32::try_from(wc.div_ceil(200).max(1)).unwrap_or(u32::MAX);
            Some(minutes)
        } else {
            None
        };
        Self::Article(ArticleDocument {
            doc_id,
            title: article.data.metadata.title,
            url: article.data.metadata.url.to_string(),
            domain,
            date: article.data.metadata.date,
            category: article.data.metadata.category,
            reference: article.data.reference,
            word_count: Some(word_count),
            reading_minutes,
            content: DocumentContent::new(article.data.text, ContentFormat::Plain),
        })
    }

    pub(crate) fn from_video(video: Video) -> Self {
        let doc_id = DocumentRef::new(DocumentKind::Video, video.id);
        let domain = video.data.metadata.url.host_str().map(str::to_owned);
        let duration_seconds = video
            .data
            .duration_seconds
            .and_then(|s| u32::try_from(s.max(0)).ok());
        Self::Video(VideoDocument {
            doc_id,
            title: video.data.metadata.title,
            url: video.data.metadata.url.to_string(),
            domain,
            date: video.data.metadata.date,
            category: video.data.metadata.category,
            thumbnail_url: video.data.thumbnail_url,
            duration_seconds,
            content: DocumentContent::new(video.data.text, ContentFormat::Plain),
        })
    }

    pub(crate) fn from_talk(talk: Talk) -> Self {
        let doc_id = DocumentRef::new(DocumentKind::Talk, talk.id);
        let duration_seconds = talk
            .data
            .duration_seconds
            .and_then(|s| u32::try_from(s.max(0)).ok());
        let transcript = talk.data.transcript.unwrap_or_default();
        let format = if transcript.contains("<v ") {
            ContentFormat::Vtt
        } else {
            ContentFormat::Plain
        };
        Self::Talk(TalkDocument {
            doc_id,
            title: talk.data.title,
            url: talk.data.website_url.to_string(),
            date: talk.data.date,
            conference: talk.data.conference,
            summary: talk.data.summary,
            video_url: talk.data.video_url.map(|u| u.to_string()),
            slides_url: talk.data.slides_url.map(|u| u.to_string()),
            thumbnail_url: talk.data.thumbnail_url,
            duration_seconds,
            content: DocumentContent::new(transcript, format),
        })
    }

    pub(crate) fn from_podcast(episode: PodcastEpisode, guests: Vec<String>) -> Self {
        let doc_id = DocumentRef::new(DocumentKind::Podcast, episode.id);
        let domain = episode
            .url()
            .host_str()
            .map(str::to_owned)
            .unwrap_or_else(|| "unknown".to_string());
        let duration_seconds = episode
            .duration_seconds()
            .and_then(|s| u32::try_from(s.max(0)).ok());
        let transcript = episode.data.transcript;
        let format = if transcript.contains("<v ") {
            ContentFormat::Vtt
        } else {
            ContentFormat::Plain
        };
        Self::Podcast(PodcastDocument {
            doc_id,
            title: episode.data.metadata.title.clone(),
            podcast_name: episode.data.podcast_name,
            episode_name: episode.data.episode_name,
            url: episode.data.metadata.url.to_string(),
            domain,
            date: episode.data.metadata.date,
            summary: episode.data.summary,
            thumbnail_url: episode.data.thumbnail_url,
            duration_seconds,
            guests,
            content: DocumentContent::new(transcript, format),
        })
    }

    pub(crate) fn from_research(paper: ResearchPaper) -> Self {
        let doc_id = DocumentRef::new(DocumentKind::Research, paper.id);
        let domain = paper
            .url()
            .host_str()
            .map(str::to_owned)
            .unwrap_or_else(|| "unknown".to_string());
        let category = ArxivCategory::from_code(&paper.data.metadata.category);
        Self::Research(ResearchDocument {
            doc_id,
            title: paper.data.metadata.title,
            url: paper.data.metadata.url.to_string(),
            domain,
            date: paper.data.metadata.date,
            authors: paper.data.authors,
            abstract_text: paper.data.abstract_text,
            category: category.to_string(),
            category_code: paper.data.metadata.category,
            paper_id: paper.data.paper_id,
            publication: paper.data.publication,
            content: DocumentContent::new(paper.data.text, ContentFormat::Plain),
        })
    }

    /// Returns this document's stable identifier.
    pub(crate) fn doc_id(&self) -> DocumentRef {
        match self {
            Self::Article(a) => a.doc_id,
            Self::Video(v) => v.doc_id,
            Self::Talk(t) => t.doc_id,
            Self::Podcast(p) => p.doc_id,
            Self::Research(r) => r.doc_id,
        }
    }

    /// Borrows the document's text body.
    pub(crate) fn body(&self) -> &str {
        match self {
            Self::Article(a) => &a.content.text,
            Self::Video(v) => &v.content.text,
            Self::Talk(t) => &t.content.text,
            Self::Podcast(p) => &p.content.text,
            Self::Research(r) => &r.content.text,
        }
    }
}

// =====================================================================
// Passage / search-within-document DTOs.
// =====================================================================

/// Strip `<mark>` / `</mark>` tags from an FTS snippet so API consumers
/// receive plain text. The HTML UI relies on the same tags for styling,
/// so we only remove them at the API boundary (in [`SearchHit::from_result_with_passages`]).
fn strip_mark_tags(s: &str) -> String {
    // Replace is allocation-light here: snippets are <= ~300 chars and
    // typically contain at most a handful of `<mark>` pairs.
    s.replace("<mark>", "").replace("</mark>", "")
}

/// One excerpt from a document's body, with stable char offsets so clients
/// can highlight or link to the same region.
///
/// We deliberately do *not* include a pre-highlighted (`<mark>`-wrapped)
/// copy of the passage: it would roughly double the payload size for LLM
/// consumers without adding any information, since the query terms are
/// already known to the caller and the `text` + offsets are enough to
/// re-mark client-side.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct PassageHit {
    /// Character offset of the passage start in the document body
    /// (0-based, inclusive). Counted in Unicode `char`s, not bytes.
    pub char_start: u32,
    /// Character offset of the passage end (exclusive).
    pub char_end: u32,
    /// Raw passage text (no markup).
    pub text: String,
    /// Number of (possibly overlapping) query-term hits inside the passage.
    /// Higher is more relevant.
    pub match_count: u32,
}

impl From<Passage> for PassageHit {
    fn from(p: Passage) -> Self {
        Self {
            char_start: u32::try_from(p.char_start).unwrap_or(u32::MAX),
            char_end: u32::try_from(p.char_end).unwrap_or(u32::MAX),
            text: p.text,
            match_count: u32::try_from(p.match_count).unwrap_or(u32::MAX),
        }
    }
}

/// Top-level response from `GET /api/v1/documents/{doc_id}/search`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct DocumentSearchResponse {
    /// Echo of the document identifier.
    pub doc_id: DocumentRef,
    /// Echo of the trimmed query.
    pub query: String,
    /// Total length of the searched body in characters.
    pub char_count: u32,
    /// Heuristic token estimate of the body.
    pub token_estimate: u32,
    /// Number of passages returned (`<= max_snippets`).
    pub returned: usize,
    /// Ranked passages, best match first.
    pub passages: Vec<PassageHit>,
}

// =====================================================================
// Batch fetch DTOs.
// =====================================================================

/// Body of [`POST /api/v1/documents:batch`].
#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct BatchDocumentsRequest {
    /// Document identifiers to fetch. Order is preserved in the response's
    /// `documents` array. Up to 25 identifiers per request.
    #[schema(example = json!(["article:1", "podcast:42", "research:7"]))]
    pub ids: Vec<DocumentRef>,
}

/// Response from [`POST /api/v1/documents:batch`].
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct BatchDocumentsResponse {
    /// Documents that were found, in the same order as the request `ids`
    /// list (with missing entries skipped — use `missing` to reconcile).
    pub documents: Vec<DocumentDetail>,
    /// Identifiers that resolved to no document.
    pub missing: Vec<DocumentRef>,
}

// The legacy `PodcastDetail` returned by `GET /api/v1/podcasts/{id}` stays as-is
// for backwards compatibility. It mirrors the [`PodcastDocument`] variant of
// the new [`DocumentDetail`] enum without the `kind` discriminator.
