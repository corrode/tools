//! # Search Params
//!
//! This module owns search input parsing, validation, and normalization.
//! It is intentionally independent from storage: storage receives typed,
//! validated inputs and never parses raw user strings.
//!
//! ## Design goals
//! - Strong types (avoid stringly-typed logic).
//! - Centralized validation with clear error reporting.
//! - Maintainable parsing rules (single `site:` filter, no negation).
//! - Easy to unit-test without touching storage or HTTP.
//!
//! ## Types and responsibilities
//! - [`RawParams`]: serde-friendly HTTP query parameters.
//! - [`Params`]: validated/normalized filters (date range, sort, page).
//! - [`FtsQuery`]: normalized, escaped query string for FTS.
//!
//! ## Parsing rules (current, intentionally minimal)
//! - Quotes group phrases: `"async await"` is one term.
//! - Single `site:` filter, with `site:example.com` or `site: example.com`.
//! - Additional `site:` filters are rejected (parse error).
//! - No negation, no OR, no parentheses.
//!
//! ## Example queries
//! - `async await`
//! - `"async await" borrow checker`
//! - `site:github.com rust async`
//! - `rust site:example.com`
//!
//! ## Trade-offs
//! - We do not support advanced syntax (negation, OR) to keep UX simple.
//! - We prefer validation errors over silent coercion; callers can fall back
//!   to defaults if parsing fails.
//!
//! ## Future extensibility
//! - Introduce `SearchExpression` to support boolean logic.
//! - Allow multiple filters (e.g., tags) as new typed fields.
//! - Add stricter site validation (e.g., host parsing).
//!
//! ## Fallback strategy
//! Parsing returns `Result<_, ParamsError>`. Callers can gracefully fall back
//! to default filters if parsing fails.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::ContentType;

/// HTTP query parameters for `/search`.
///
/// This struct should remain serde-friendly and be used at the Axum boundary.
/// It is intentionally permissive; validation happens during normalization.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct RawParams {
    /// Raw query string from the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    /// Optional start year filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_year: Option<i32>,
    /// Optional end year filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_year: Option<i32>,
    /// Optional sort order requested by the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<SortOrder>,
    /// Content type filter: "articles", "video", or "podcast".
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<ContentType>,
    /// Optional page number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
}

impl RawParams {
    /// Builds a `/search` URL for the given page, serializing only set params.
    pub fn build_url(&self, page: u32) -> String {
        let mut url = String::from("/search?");
        let mut serializer = url::form_urlencoded::Serializer::new(&mut url);

        if let Some(q) = &self.q {
            serializer.append_pair("q", q);
        }
        if let Some(start_year) = self.start_year {
            serializer.append_pair("start-year", &start_year.to_string());
        }
        if let Some(end_year) = self.end_year {
            serializer.append_pair("end-year", &end_year.to_string());
        }
        if let Some(sort_by) = self.sort_by {
            serializer.append_pair("sort-by", sort_by.as_str());
        }
        if let Some(content_type) = self.content_type {
            serializer.append_pair("type", &content_type.to_string());
        }
        serializer.append_pair("page", &page.to_string());

        serializer.finish();
        url
    }

    /// Normalizes params into validated filters.
    pub fn normalize(self, defaults: SearchDefaults) -> Result<Params, ParamsError> {
        Params::try_from((self, defaults))
    }

    /// Normalizes params with a graceful fallback on error.
    ///
    /// Returns the filters plus an optional error describing why fallback
    /// was used.
    pub fn normalize_or_fallback(self, defaults: SearchDefaults) -> (Params, Option<ParamsError>) {
        match Params::try_from((self, defaults)) {
            Ok(filters) => (filters, None),
            Err(err) => (Params::fallback(defaults), Some(err)),
        }
    }
}

/// Deprecated alias for `Params`.
#[deprecated(note = "use Params")]
pub type SearchFilters = Params;

/// Sort order for search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SortOrder {
    /// Rank by textual relevance.
    #[default]
    Relevance,
    /// Newest first.
    DateDesc,
    /// Oldest first.
    DateAsc,
}

impl SortOrder {
    /// Returns the query-string representation for this sort order.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::DateDesc => "date-desc",
            Self::DateAsc => "date-asc",
        }
    }
}

impl fmt::Display for SortOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Default values and constraints for normalization.
#[derive(Debug, Clone, Copy)]
pub struct SearchDefaults {
    /// Minimum allowed year.
    pub min_year: i32,
    /// Maximum allowed year.
    pub max_year: i32,
    /// Default start year when absent.
    pub default_start_year: i32,
    /// Default end year when absent.
    pub default_end_year: i32,
    /// Default sort order when absent.
    pub default_sort: SortOrder,
    /// Default page number when absent.
    pub default_page: u32,
}

impl SearchDefaults {
    /// Creates defaults using the provided min/max range.
    pub fn new(min_year: i32, max_year: i32) -> Self {
        Self {
            min_year,
            max_year,
            default_start_year: min_year,
            default_end_year: max_year,
            default_sort: SortOrder::default(),
            default_page: 1,
        }
    }
}

/// Structured filters supported by the search parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchFilter {
    /// Restrict results to a specific site/host.
    Site(SiteFilter),
}

fn parse_query(input: &str) -> Result<(Vec<SearchTerm>, Vec<SearchFilter>), ParamsError> {
    let tokens = tokenize(input);

    let mut terms = Vec::new();
    let mut filters: Vec<SearchFilter> = Vec::new();

    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Word(word) => {
                if let Some(remainder) = word.strip_prefix("site:") {
                    let site_value = if remainder.is_empty() {
                        let next = tokens.get(i + 1).ok_or(ParamsError::EmptySiteFilter)?;
                        let value = token_to_string(next);
                        i += 1;
                        value
                    } else {
                        remainder.to_string()
                    };

                    let parsed_site = SiteFilter::new(site_value)?;
                    if filters
                        .iter()
                        .any(|filter| matches!(filter, SearchFilter::Site(_)))
                    {
                        return Err(ParamsError::MultipleSiteFilters);
                    }
                    filters.push(SearchFilter::Site(parsed_site));
                } else if !word.trim().is_empty() {
                    terms.push(SearchTerm::new(word.clone())?);
                }
            }
            Token::Phrase(phrase) => {
                if !phrase.trim().is_empty() {
                    terms.push(SearchTerm::new(phrase.clone())?);
                }
            }
        }
        i += 1;
    }

    Ok((terms, filters))
}

fn escape_fts_query(terms: &[SearchTerm]) -> Option<FtsQuery> {
    if terms.is_empty() {
        return None;
    }

    let escaped = terms
        .iter()
        .map(|term| format!("\"{}\"", term.as_str().replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ");

    Some(FtsQuery(escaped))
}

/// Normalized/validated filters derived from params + parsed query.
#[derive(Debug, Clone)]
pub struct Params {
    /// Parsed search terms and phrases.
    pub terms: Vec<SearchTerm>,
    /// Structured filters extracted from the query.
    pub filters: Vec<SearchFilter>,
    /// Optional site filter.
    pub site: Option<SiteFilter>,
    /// Inclusive start year.
    pub start_year: i32,
    /// Inclusive end year.
    pub end_year: i32,
    /// Selected sort order.
    pub sort_by: SortOrder,
    /// Optional content type filter.
    pub content_type: Option<ContentType>,
    /// 1-based page number.
    pub page: u32,
}

impl Params {
    /// Returns true when the year range matches the defaults.
    pub fn is_default_range(&self, defaults: SearchDefaults) -> bool {
        self.start_year == defaults.default_start_year && self.end_year == defaults.default_end_year
    }

    /// Returns true when any query terms are present.
    pub fn has_query_terms(&self) -> bool {
        !self.terms.is_empty()
    }

    /// Returns true when any structured filters are present.
    pub fn has_filters(&self) -> bool {
        !self.filters.is_empty()
    }

    /// Returns the site filter if present.
    pub fn site_filter(&self) -> Option<&SiteFilter> {
        self.filters
            .iter()
            .map(|filter| match filter {
                SearchFilter::Site(site) => site,
            })
            .next()
    }

    /// Returns an escaped FTS query string if there are terms.
    pub fn escaped_fts_query(&self) -> Option<FtsQuery> {
        escape_fts_query(&self.terms)
    }

    /// Builds a fallback filters value from defaults.
    pub fn fallback(defaults: SearchDefaults) -> Self {
        Self {
            terms: Vec::new(),
            filters: Vec::new(),
            site: None,
            start_year: defaults.default_start_year,
            end_year: defaults.default_end_year,
            sort_by: defaults.default_sort,
            content_type: None,
            page: defaults.default_page,
        }
    }
}

impl TryFrom<(RawParams, SearchDefaults)> for Params {
    type Error = ParamsError;

    fn try_from(value: (RawParams, SearchDefaults)) -> Result<Self, Self::Error> {
        let (params, defaults) = value;

        let (terms, filters) = match params.q.as_deref() {
            Some(raw) if !raw.trim().is_empty() => parse_query(raw)?,
            _ => (Vec::new(), Vec::new()),
        };

        let site = filters
            .iter()
            .map(|filter| match filter {
                SearchFilter::Site(site) => site.clone(),
            })
            .next();

        let start_year = params.start_year.unwrap_or(defaults.default_start_year);
        let end_year = params.end_year.unwrap_or(defaults.default_end_year);

        if start_year < defaults.min_year || start_year > defaults.max_year {
            return Err(ParamsError::YearOutOfBounds(start_year));
        }
        if end_year < defaults.min_year || end_year > defaults.max_year {
            return Err(ParamsError::YearOutOfBounds(end_year));
        }
        if start_year > end_year {
            return Err(ParamsError::InvalidYearRange {
                start_year,
                end_year,
            });
        }

        let sort_by = params.sort_by.unwrap_or(defaults.default_sort);

        let page = params.page.unwrap_or(defaults.default_page);
        if page == 0 {
            return Err(ParamsError::InvalidPage(page));
        }

        Ok(Self {
            terms,
            filters,
            site,
            start_year,
            end_year,
            sort_by,
            content_type: params.content_type,
            page,
        })
    }
}

/// Typed search term (token or phrase).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchTerm(String);

impl SearchTerm {
    /// Creates a validated search term.
    pub fn new(input: String) -> Result<Self, ParamsError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(ParamsError::EmptyTerm);
        }
        Ok(Self(trimmed.to_string()))
    }

    /// Returns the term as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Typed site filter (e.g., `github.com`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteFilter(String);

impl SiteFilter {
    /// Creates a validated site filter value.
    pub fn new(input: String) -> Result<Self, ParamsError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(ParamsError::EmptySiteFilter);
        }
        if trimmed.split_whitespace().count() > 1 {
            return Err(ParamsError::InvalidSiteFilter(trimmed.to_string()));
        }
        Ok(Self(trimmed.to_string()))
    }

    /// Returns the site filter as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Escaped FTS query string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtsQuery(String);

impl FtsQuery {
    /// Returns the escaped query string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Parsing/validation error type for params.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamsError {
    /// The site filter was missing a value.
    EmptySiteFilter,
    /// The site filter contained invalid characters or whitespace.
    InvalidSiteFilter(String),
    /// More than one site filter was supplied.
    MultipleSiteFilters,
    /// A parsed term was empty after trimming.
    EmptyTerm,
    /// The provided year range was inverted.
    InvalidYearRange {
        /// Inclusive start year.
        start_year: i32,
        /// Inclusive end year.
        end_year: i32,
    },
    /// A year was outside the allowed bounds.
    YearOutOfBounds(i32),
    /// The page number was invalid (must be >= 1).
    InvalidPage(u32),
}

impl fmt::Display for ParamsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySiteFilter => write!(f, "site filter is empty"),
            Self::InvalidSiteFilter(value) => write!(f, "invalid site filter: {value}"),
            Self::MultipleSiteFilters => write!(f, "multiple site filters are not supported"),
            Self::EmptyTerm => write!(f, "search term is empty"),
            Self::InvalidYearRange {
                start_year,
                end_year,
            } => write!(f, "invalid year range: {start_year}..{end_year}"),
            Self::YearOutOfBounds(value) => write!(f, "year out of bounds: {value}"),
            Self::InvalidPage(value) => write!(f, "invalid page: {value}"),
        }
    }
}

impl std::error::Error for ParamsError {}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Phrase(String),
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        if ch == '"' {
            chars.next();
            let mut phrase = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                phrase.push(c);
            }
            tokens.push(Token::Phrase(phrase));
            continue;
        }

        let mut word = String::new();
        while let Some(c) = chars.peek().copied() {
            if c.is_whitespace() || c == '"' {
                break;
            }
            word.push(c);
            chars.next();
        }
        tokens.push(Token::Word(word));
    }

    tokens
}

fn token_to_string(token: &Token) -> String {
    match token {
        Token::Word(value) | Token::Phrase(value) => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_basic_terms() {
        let (terms, filters) = parse_query("rust async await").unwrap();
        assert_eq!(
            terms,
            vec![
                SearchTerm("rust".to_string()),
                SearchTerm("async".to_string()),
                SearchTerm("await".to_string())
            ]
        );
        assert!(filters.is_empty());
    }

    #[test]
    fn parse_query_quoted_phrase() {
        let (terms, filters) = parse_query("\"async await\" borrow checker").unwrap();
        assert_eq!(
            terms,
            vec![
                SearchTerm("async await".to_string()),
                SearchTerm("borrow".to_string()),
                SearchTerm("checker".to_string())
            ]
        );
        assert!(filters.is_empty());
    }

    #[test]
    fn parse_query_site_inline() {
        let (terms, filters) = parse_query("rust site:github.com").unwrap();
        assert_eq!(
            filters,
            vec![SearchFilter::Site(SiteFilter("github.com".to_string()))]
        );
        assert_eq!(terms, vec![SearchTerm("rust".to_string())]);
    }

    #[test]
    fn parse_query_site_separate_token() {
        let (terms, filters) = parse_query("site: github.com rust").unwrap();
        assert_eq!(
            filters,
            vec![SearchFilter::Site(SiteFilter("github.com".to_string()))]
        );
        assert_eq!(terms, vec![SearchTerm("rust".to_string())]);
    }

    #[test]
    fn parse_query_multiple_site_filters_is_error() {
        let err = parse_query("site:example.com site:github.com").unwrap_err();
        assert_eq!(err, ParamsError::MultipleSiteFilters);
    }

    #[test]
    fn parse_query_site_in_phrase_is_term() {
        let (terms, filters) = parse_query("\"site:example.com\" rust").unwrap();
        assert_eq!(
            terms,
            vec![
                SearchTerm("site:example.com".to_string()),
                SearchTerm("rust".to_string())
            ]
        );
        assert!(filters.is_empty());
    }

    #[test]
    fn parse_query_unterminated_quote() {
        let (terms, filters) = parse_query("\"async await").unwrap();
        assert_eq!(terms, vec![SearchTerm("async await".to_string())]);
        assert!(filters.is_empty());
    }

    #[test]
    fn fts_query_escapes_quotes() {
        let terms = vec![SearchTerm("he said \"hi\"".to_string())];
        let fts = escape_fts_query(&terms).unwrap();
        assert_eq!(fts.as_str(), "\"he said \"\"hi\"\"\"");
    }

    #[test]
    fn filters_validation_year_bounds() {
        let defaults = SearchDefaults::new(2013, 2025);
        let params = RawParams {
            q: Some("rust".to_string()),
            start_year: Some(2010),
            end_year: None,
            sort_by: None,
            content_type: None,
            page: None,
        };

        let err = Params::try_from((params, defaults)).unwrap_err();
        assert_eq!(err, ParamsError::YearOutOfBounds(2010));
    }

    #[test]
    fn filters_validation_page() {
        let defaults = SearchDefaults::new(2013, 2025);
        let params = RawParams {
            q: Some("rust".to_string()),
            start_year: None,
            end_year: None,
            sort_by: None,
            content_type: None,
            page: Some(0),
        };

        let err = Params::try_from((params, defaults)).unwrap_err();
        assert_eq!(err, ParamsError::InvalidPage(0));
    }
}
