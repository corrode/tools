//! View-layer result types for search results.
//!
//! These types are optimized for template rendering, converting from the
//! database-layer `SearchResult` type.

use crate::{ArxivCategory, SearchEntry, SearchResult};

/// Video search result for display in templates.
/// Also used for conference talks, which share the same template.
#[derive(Debug, Clone)]
pub struct Video {
    /// Video title
    pub title: String,
    /// Title with search terms highlighted via `<mark>` tags
    pub highlighted_title: Option<String>,
    /// Primary URL (video URL for plain videos, website URL for talks)
    pub url: String,
    /// Thumbnail URL (if available)
    pub thumbnail_url: Option<String>,
    /// Formatted duration (e.g., "12:34")
    pub duration: Option<String>,
    /// Search snippet with highlights
    pub snippet: Option<String>,
    /// Formatted date
    pub date: String,
    /// Domain name (e.g., "youtube.com") — None for talks
    pub domain: Option<String>,
    /// Conference name — Some for talks, None for plain videos
    pub conference: Option<String>,
    /// Talk summary — used as snippet fallback when no FTS snippet is available
    pub summary: Option<String>,
    /// Direct video URL — Some for talks that have a recording, always the URL for plain videos
    pub video_url: Option<String>,
    /// Slides URL — Some for talks that have slides
    pub slides_url: Option<String>,
}

/// Article search result for display in templates.
#[derive(Debug, Clone)]
pub struct Article {
    /// Article title
    pub title: String,
    /// Title with search terms highlighted via `<mark>` tags
    pub highlighted_title: Option<String>,
    /// Article URL
    pub url: String,
    /// Search snippet with highlights
    pub snippet: Option<String>,
    /// Formatted date
    pub date: String,
    /// Domain name
    pub domain: String,
    /// Category (e.g., "RFC", "Blog")
    pub category: String,
    /// Reference identifier (e.g., "RFC #123", "TWiR #456")
    pub reference: Option<String>,
    /// Formatted reading time (e.g., "~5 min read")
    pub reading_time: String,
}

/// Podcast search result for display in templates.
#[derive(Debug, Clone)]
pub struct Podcast {
    /// Podcast title
    pub title: String,
    /// Title with search terms highlighted via `<mark>` tags
    pub highlighted_title: Option<String>,
    /// Podcast URL
    pub url: String,
    /// Podcast/show name
    pub podcast_name: String,
    /// Episode name
    pub episode_name: String,
    /// Thumbnail URL (if available)
    pub thumbnail_url: Option<String>,
    /// Formatted duration (e.g., "42:17")
    pub duration: Option<String>,
    /// Search snippet with highlights
    pub snippet: Option<String>,
    /// Formatted date
    pub date: String,
    /// Domain name
    pub domain: String,
    /// Episode summary (optional)
    pub summary: Option<String>,
}

/// Maximum number of authors to display before truncating with "et al."
const MAX_AUTHORS_SHOWN: usize = 3;

/// Research paper search result for display in templates.
#[derive(Debug, Clone)]
pub struct Research {
    /// Paper title
    pub title: String,
    /// Title with search terms highlighted via `<mark>` tags
    pub highlighted_title: Option<String>,
    /// Paper URL
    pub url: String,
    /// Authors (truncated to a few names + "et al." if needed)
    pub authors: String,
    /// Abstract/summary (shortened by SQL — FTS snippet or substr)
    pub abstract_text: String,
    /// Search snippet with highlights
    pub snippet: Option<String>,
    /// Formatted date
    pub date: String,
    /// Domain name
    pub domain: String,
    /// Parsed arXiv category with human-readable display name
    pub category: ArxivCategory,
    /// Paper ID (e.g., "arXiv:2301.00000" or DOI)
    pub paper_id: Option<String>,
    /// Publication venue
    pub publication: Option<String>,
}

/// Truncate an authors string to at most `max` names, appending "et al." if
/// there are more.
fn truncate_authors(authors: &str, max: usize) -> String {
    let parts: Vec<&str> = authors.split(", ").collect();
    if parts.len() <= max {
        return authors.to_string();
    }
    let mut result = parts[..max].join(", ");
    result.push_str(" et al.");
    result
}

/// Talk search result for display in templates.
#[derive(Debug, Clone)]
pub struct Talk {
    /// Talk title
    pub title: String,
    /// Title with search terms highlighted via `<mark>` tags
    pub highlighted_title: Option<String>,
    /// Talk URL
    pub url: String,
    /// Conference name
    pub conference: String,
    /// Formatted date
    pub date: String,
    /// Search snippet with highlights
    pub snippet: Option<String>,
    /// Talk summary
    pub summary: String,
    /// Video URL (if available)
    pub video_url: Option<String>,
    /// Slides URL (if available)
    pub slides_url: Option<String>,
    /// Thumbnail URL (if available)
    pub thumbnail_url: Option<String>,
    /// Duration string (if available)
    pub duration: Option<String>,
}

/// Sanitise an FTS5 snippet/highlight for safe rendering with `|safe`.
///
/// FTS5 wraps matched terms in `<mark>...</mark>` but the surrounding text
/// is the raw document body, which can contain characters the browser
/// interprets as HTML (e.g. Rust generics like `<S: Marker>`, which the
/// parser happily treats as an `<s>` strikethrough start tag and bleeds
/// styling into subsequent results). We HTML-escape `<`, `>`, and `&`
/// everywhere except for the `<mark>` / `</mark>` markers themselves.
pub(crate) fn sanitize_snippet(snippet: Option<String>) -> Option<String> {
    let value = snippet?;
    if !value.contains('<') && !value.contains('>') && !value.contains('&') {
        return Some(value);
    }

    let bytes = value.as_bytes();
    let mut output = String::with_capacity(value.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'<' {
            // Check for `<mark>` / `</mark>` (case-insensitive) and pass
            // them through verbatim; everything else gets escaped so it
            // renders as literal text instead of an HTML tag.
            if matches_ci(&bytes[i..], b"<mark>") {
                output.push_str("<mark>");
                i += 6;
                continue;
            }
            if matches_ci(&bytes[i..], b"</mark>") {
                output.push_str("</mark>");
                i += 7;
                continue;
            }
            output.push_str("&lt;");
            i += 1;
        } else if b == b'>' {
            output.push_str("&gt;");
            i += 1;
        } else if b == b'&' {
            output.push_str("&amp;");
            i += 1;
        } else {
            // Copy one UTF-8 char. `b` is a leading byte; figure out how
            // many continuation bytes follow.
            let len = utf8_char_len(b);
            let end = (i + len).min(bytes.len());
            // Safety net: if `len` is wrong somehow, fall back to 1 byte.
            match std::str::from_utf8(&bytes[i..end]) {
                Ok(s) => output.push_str(s),
                Err(_) => output.push(b as char),
            }
            i = end;
        }
    }

    Some(output)
}

fn matches_ci(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len()
        && haystack[..needle.len()]
            .iter()
            .zip(needle.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xC0 {
        1 // continuation byte mid-stream; treat defensively
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

impl TryFrom<SearchResult> for Video {
    type Error = &'static str;

    fn try_from(result: SearchResult) -> Result<Self, Self::Error> {
        let duration = result.duration().map(|d| d.to_string());
        let highlighted_title = sanitize_snippet(result.highlighted_title.clone());
        let SearchResult { entry, snippet, .. } = result;
        let snippet = sanitize_snippet(snippet);

        match entry {
            SearchEntry::Video(video) => Ok(Self {
                title: video.title().to_string(),
                highlighted_title,
                url: video.url().to_string(),
                thumbnail_url: video.thumbnail_url().map(|s| s.to_string()),
                duration,
                date: video.date().to_string(),
                domain: Some(video.url().host_str().unwrap_or("youtube.com").to_string()),
                conference: None,
                summary: None,
                video_url: None,
                slides_url: None,
                snippet,
            }),
            SearchEntry::Talk(talk) => Ok(Self {
                title: talk.title().to_string(),
                highlighted_title,
                url: talk.website_url().to_string(),
                thumbnail_url: talk.thumbnail_url().map(|s| s.to_string()),
                duration,
                date: talk.date().to_string(),
                domain: None,
                conference: Some(talk.conference().to_string()),
                summary: Some(talk.summary().to_string()),
                video_url: talk.video_url().map(|s| s.to_string()),
                slides_url: talk.slides_url().map(|s| s.to_string()),
                snippet,
            }),
            _ => Err("expected video or talk result for Video view"),
        }
    }
}

impl TryFrom<SearchResult> for Podcast {
    type Error = &'static str;

    fn try_from(result: SearchResult) -> Result<Self, Self::Error> {
        let duration = result.duration().map(|d| d.to_string());
        let domain = result.host_str().unwrap_or("unknown").to_string();
        let highlighted_title = sanitize_snippet(result.highlighted_title.clone());
        let SearchResult { entry, snippet, .. } = result;
        let summary = entry.summary().map(|s| s.to_string());
        let SearchEntry::Podcast(podcast) = entry else {
            return Err("expected podcast result for Podcast view");
        };
        let podcast_name = podcast.podcast_name().to_string();
        let episode_name = podcast.episode_name().to_string();

        Ok(Self {
            title: podcast.title().to_string(),
            highlighted_title,
            url: podcast.url().to_string(),
            podcast_name,
            episode_name,
            thumbnail_url: podcast.thumbnail_url().map(|s| s.to_string()),
            duration,
            snippet: sanitize_snippet(snippet),
            date: podcast.date().to_string(),
            domain,
            summary,
        })
    }
}

impl TryFrom<SearchResult> for Talk {
    type Error = &'static str;

    fn try_from(result: SearchResult) -> Result<Self, Self::Error> {
        let duration = result.duration().map(|d| d.to_string());
        let highlighted_title = sanitize_snippet(result.highlighted_title.clone());
        let SearchResult { entry, snippet, .. } = result;
        let snippet = sanitize_snippet(snippet);
        let SearchEntry::Talk(talk) = entry else {
            return Err("expected talk result for Talk view");
        };

        Ok(Self {
            title: talk.title().to_string(),
            highlighted_title,
            url: talk.website_url().to_string(),
            conference: talk.conference().to_string(),
            date: talk.date().to_string(),
            snippet,
            summary: talk.summary().to_string(),
            video_url: talk.video_url().map(|s| s.to_string()),
            slides_url: talk.slides_url().map(|s| s.to_string()),
            thumbnail_url: talk.thumbnail_url().map(|s| s.to_string()),
            duration,
        })
    }
}

impl TryFrom<SearchResult> for Article {
    type Error = &'static str;

    fn try_from(result: SearchResult) -> Result<Self, Self::Error> {
        let reading_time = result
            .duration()
            .map(|d| d.to_string())
            .unwrap_or_else(|| "~1 min read".to_string());
        let domain = result.host_str().unwrap_or("unknown").to_string();
        let highlighted_title = sanitize_snippet(result.highlighted_title.clone());
        let SearchResult { entry, snippet, .. } = result;
        let snippet = sanitize_snippet(snippet);
        let SearchEntry::Article(article) = entry else {
            return Err("expected article result for Article view");
        };

        Ok(Self {
            title: article.title().to_string(),
            highlighted_title,
            url: article.url().to_string(),
            date: article.date().to_string(),
            domain,
            category: article.category().to_string(),
            reference: article.reference().map(|s| s.to_string()),
            reading_time,
            snippet,
        })
    }
}

impl TryFrom<SearchResult> for Research {
    type Error = &'static str;

    fn try_from(result: SearchResult) -> Result<Self, Self::Error> {
        let domain = result.host_str().unwrap_or("unknown").to_string();
        let highlighted_title = sanitize_snippet(result.highlighted_title.clone());
        let SearchResult { entry, snippet, .. } = result;
        let snippet = sanitize_snippet(snippet);
        let SearchEntry::Research(paper) = entry else {
            return Err("expected research result for Research view");
        };

        Ok(Self {
            title: paper.title().to_string(),
            highlighted_title,
            url: paper.url().to_string(),
            authors: truncate_authors(paper.authors(), MAX_AUTHORS_SHOWN),
            abstract_text: paper.abstract_text().to_string(),
            snippet,
            date: paper.date().to_string(),
            domain,
            category: ArxivCategory::from_code(paper.category()),
            paper_id: paper.paper_id().map(|s| s.to_string()),
            publication: paper.publication().map(|s| s.to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_snippet;

    #[test]
    fn passes_through_plain_text() {
        assert_eq!(
            sanitize_snippet(Some("hello world".into())),
            Some("hello world".into())
        );
    }

    #[test]
    fn preserves_mark_tags() {
        assert_eq!(
            sanitize_snippet(Some("find <mark>rust</mark> here".into())),
            Some("find <mark>rust</mark> here".into())
        );
    }

    #[test]
    fn escapes_rust_generics_that_look_like_s_tag() {
        // Regression: `<S: Marker>` in an article body was parsed by the
        // browser as an `<s>` strikethrough start tag and bled into all
        // following results.
        assert_eq!(
            sanitize_snippet(Some("impl<S: Marker> for HttpResponse".into())),
            Some("impl&lt;S: Marker&gt; for HttpResponse".into())
        );
    }

    #[test]
    fn escapes_ampersands_and_stray_brackets() {
        assert_eq!(
            sanitize_snippet(Some("a & b > c < d".into())),
            Some("a &amp; b &gt; c &lt; d".into())
        );
    }

    #[test]
    fn mixes_marks_with_escaped_html() {
        assert_eq!(
            sanitize_snippet(Some("<mark>impl</mark><S: Marker>".into())),
            Some("<mark>impl</mark>&lt;S: Marker&gt;".into())
        );
    }

    #[test]
    fn preserves_utf8() {
        assert_eq!(
            sanitize_snippet(Some("café — <mark>rüst</mark>".into())),
            Some("café — <mark>rüst</mark>".into())
        );
    }
}
