//! CSS selector and HTML scraping helpers.
//!
//! Thin convenience wrappers around [`scraper`] that eliminate the repetitive
//! boilerplate every HTML-based conference parser needs.
//!
//! # Quick start
//!
//! ```ignore
//! use crate::tools::css::{css, text, select_text, select_all_text};
//!
//! let sel       = css("div.timetable")?;
//! let title_sel = css("h3.table-title")?;
//!
//! for timetable in document.select(&sel) {
//!     let title = select_text(timetable, &title_sel).unwrap_or_default();
//!     // ...
//! }
//! ```

use anyhow::Result;
use scraper::{ElementRef, Selector};

/// Parse a CSS selector string, returning a descriptive [`anyhow::Error`] on
/// failure.
///
/// `scraper::Selector::parse` returns a `cssparser::ParseError` that borrows
/// the input, so it cannot be used directly with [`anyhow::Context`]. This
/// helper does the `map_err` conversion for you.
///
/// # Examples
///
/// ```ignore
/// let timetable = css("div.timetable")?;
/// let speaker   = css("div.speaker p.name")?;
/// ```
pub fn css(selector: &str) -> Result<Selector> {
    Selector::parse(selector)
        .map_err(|e| anyhow::anyhow!("Failed to parse CSS selector {selector:?}: {e:?}"))
}

/// Extract the full text content of an element with whitespace normalized.
///
/// All runs of whitespace (spaces, newlines, tabs, etc.) are collapsed into
/// single spaces and leading/trailing whitespace is trimmed.
///
/// This replaces the common pattern:
///
/// ```ignore
/// normalize_whitespace(&el.text().collect::<String>())
/// ```
pub fn text(el: ElementRef<'_>) -> String {
    let raw: String = el.text().collect();
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Select the first element matching `selector` inside `parent` and return its
/// normalized text content.
///
/// Returns `None` if no element matches or if the matched element contains only
/// whitespace.
///
/// This replaces the common pattern:
///
/// ```ignore
/// parent
///     .select(&selector)
///     .next()
///     .map(|el| normalize_whitespace(&el.text().collect::<String>()))
///     .filter(|s| !s.is_empty())
/// ```
pub fn select_text(parent: ElementRef<'_>, selector: &Selector) -> Option<String> {
    let result = text(parent.select(selector).next()?);
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Collect normalized text from **all** elements matching `selector` inside
/// `parent`, skipping empty results.
///
/// This replaces the common pattern:
///
/// ```ignore
/// parent
///     .select(&selector)
///     .map(|el| normalize_whitespace(&el.text().collect::<String>()))
///     .filter(|s| !s.is_empty())
///     .collect()
/// ```
pub fn select_all_text(parent: ElementRef<'_>, selector: &Selector) -> Vec<String> {
    parent
        .select(selector)
        .map(|el| text(el))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Select the first element matching `selector` inside `parent` and return the
/// value of the given HTML attribute.
///
/// Returns `None` if no element matches or if the attribute is absent.
///
/// This replaces the common pattern:
///
/// ```ignore
/// parent
///     .select(&link_selector)
///     .next()
///     .and_then(|el| el.value().attr("href"))
/// ```
pub fn select_attr<'a>(parent: ElementRef<'a>, selector: &Selector, attr: &str) -> Option<&'a str> {
    parent.select(selector).next()?.value().attr(attr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Html;

    #[test]
    fn css_valid_selector() {
        assert!(css("div.foo").is_ok());
        assert!(css("table#schedule tbody tr").is_ok());
        assert!(css("div.speaker p.name").is_ok());
    }

    #[test]
    fn css_invalid_selector() {
        let err = css("[[[invalid").unwrap_err();
        assert!(
            err.to_string().contains("Failed to parse CSS selector"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn text_normalizes_whitespace() {
        let html = Html::parse_fragment("<p>  hello \n\t world  </p>");
        let sel = css("p").unwrap();
        let el = html.select(&sel).next().unwrap();
        assert_eq!(text(el), "hello world");
    }

    #[test]
    fn text_empty_element() {
        let html = Html::parse_fragment("<p>   </p>");
        let sel = css("p").unwrap();
        let el = html.select(&sel).next().unwrap();
        assert_eq!(text(el), "");
    }

    #[test]
    fn select_text_found() {
        let html = Html::parse_fragment(r#"<div><h3 class="title">  Hello World  </h3></div>"#);
        let parent_sel = css("div").unwrap();
        let child_sel = css("h3.title").unwrap();
        let parent = html.select(&parent_sel).next().unwrap();
        assert_eq!(select_text(parent, &child_sel), Some("Hello World".into()));
    }

    #[test]
    fn select_text_not_found() {
        let html = Html::parse_fragment("<div><p>hello</p></div>");
        let parent_sel = css("div").unwrap();
        let child_sel = css("h3.missing").unwrap();
        let parent = html.select(&parent_sel).next().unwrap();
        assert_eq!(select_text(parent, &child_sel), None);
    }

    #[test]
    fn select_text_empty_content() {
        let html = Html::parse_fragment("<div><p>   </p></div>");
        let parent_sel = css("div").unwrap();
        let child_sel = css("p").unwrap();
        let parent = html.select(&parent_sel).next().unwrap();
        assert_eq!(select_text(parent, &child_sel), None);
    }

    #[test]
    fn select_all_text_collects() {
        let html =
            Html::parse_fragment("<ul><li>Alice</li><li>  </li><li>Bob</li><li>Carol</li></ul>");
        let parent_sel = css("ul").unwrap();
        let child_sel = css("li").unwrap();
        let parent = html.select(&parent_sel).next().unwrap();
        assert_eq!(
            select_all_text(parent, &child_sel),
            vec!["Alice", "Bob", "Carol"],
        );
    }

    #[test]
    fn select_attr_found() {
        let html =
            Html::parse_fragment(r#"<div><a class="link" href="/talks/foo/">click</a></div>"#);
        let parent_sel = css("div").unwrap();
        let child_sel = css("a.link").unwrap();
        let parent = html.select(&parent_sel).next().unwrap();
        assert_eq!(select_attr(parent, &child_sel, "href"), Some("/talks/foo/"));
    }

    #[test]
    fn select_attr_missing_element() {
        let html = Html::parse_fragment("<div></div>");
        let parent_sel = css("div").unwrap();
        let child_sel = css("a.link").unwrap();
        let parent = html.select(&parent_sel).next().unwrap();
        assert_eq!(select_attr(parent, &child_sel, "href"), None);
    }

    #[test]
    fn select_attr_missing_attr() {
        let html = Html::parse_fragment(r#"<div><a class="link">no href</a></div>"#);
        let parent_sel = css("div").unwrap();
        let child_sel = css("a.link").unwrap();
        let parent = html.select(&parent_sel).next().unwrap();
        assert_eq!(select_attr(parent, &child_sel, "href"), None);
    }
}
