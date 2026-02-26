//! HTML sanitization and content extraction

use anyhow::Result;
use dom_smoothie::{CandidateSelectMode, Config, Readability};
use scraper::{Html, Selector};

/// Sanitizes HTML by extracting main content and removing boilerplate
///
/// This module provides multiple strategies for cleaning HTML:
/// 1. dom_smoothie - Automated content extraction using readability algorithm (preferred)
/// 2. Manual selector-based cleaning - Fallback for edge cases
///
/// Use --save-raw-html flag to save original HTML for testing different approaches.
pub struct Sanitizer;

impl Sanitizer {
    /// Sanitizes HTML using dom_smoothie for content extraction
    ///
    /// This is the primary method that extracts the main content
    /// while removing navigation, ads, and other boilerplate.
    /// Based on Mozilla's readability algorithm with DomSmoothie's
    /// alternative candidate selection for better content capture.
    ///
    /// Returns plain text content with normalized whitespace.
    pub fn sanitize(html: &str) -> Result<String> {
        // Configure readability with DomSmoothie mode for better content capture
        // This mode may be less "clean" but captures more meaningful content
        let cfg = Config {
            max_elements_to_parse: 10000,
            candidate_select_mode: CandidateSelectMode::DomSmoothie,
            ..Default::default()
        };

        // Parse with readability algorithm
        let mut readability = Readability::new(html, None, Some(cfg))?;
        let article = readability.parse()?;

        // Get plain text and normalize whitespace
        let text = article.text_content.to_string();
        Ok(Self::normalize_whitespace(&text))
    }

    /// Normalizes whitespace in text content
    ///
    /// - Trims leading/trailing whitespace
    /// - Collapses multiple spaces into single spaces
    /// - Collapses more than 2 consecutive newlines into 2 (preserves paragraph breaks)
    fn normalize_whitespace(text: &str) -> String {
        // First, collapse multiple spaces into single spaces on each line
        let mut result = String::with_capacity(text.len());
        let mut prev_was_space = false;

        for c in text.chars() {
            if c == ' ' || c == '\t' {
                if !prev_was_space {
                    result.push(' ');
                    prev_was_space = true;
                }
            } else {
                result.push(c);
                prev_was_space = false;
            }
        }

        // Collapse multiple newlines into at most 2
        let lines: Vec<&str> = result.lines().collect();
        let mut normalized_lines = Vec::new();
        let mut blank_count = 0;

        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                blank_count += 1;
                // Keep at most one blank line (two newlines total)
                if blank_count == 1 {
                    normalized_lines.push("");
                }
            } else {
                blank_count = 0;
                normalized_lines.push(trimmed);
            }
        }

        // Join lines and trim the final result
        normalized_lines.join("\n").trim().to_string()
    }

    /// Alternative sanitization using manual selector-based approach
    ///
    /// This is a fallback method that manually removes noise elements
    /// and extracts content areas. Kept for comparison and edge cases.
    #[allow(dead_code)]
    pub fn sanitize_manual(html: &str) -> String {
        let document = Html::parse_document(html);

        // List of noise selectors to remove
        let noise_selectors = [
            "nav",
            "header",
            "footer",
            "aside",
            "script",
            "style",
            ".navigation",
            ".nav",
            ".menu",
            ".sidebar",
            ".footer",
            "#navigation",
            "#nav",
            "#menu",
            "#sidebar",
            "#footer",
            ".cookie",
            ".advertisement",
            ".ad",
            ".social",
            ".share",
        ];

        // Try to find main content area
        let content_selectors = ["main", "article", "[role='main']", ".content", ".main"];

        let mut cleaned_html = String::new();

        // First, try to extract from main content areas
        for selector_str in &content_selectors {
            if let Ok(selector) = Selector::parse(selector_str) {
                for element in document.select(&selector) {
                    cleaned_html.push_str(&element.html());
                    if !cleaned_html.is_empty() {
                        tracing::debug!("Extracted content from: {}", selector_str);
                        return cleaned_html;
                    }
                }
            }
        }

        // Fallback: use body but remove noise
        if let Ok(body_selector) = Selector::parse("body")
            && let Some(body) = document.select(&body_selector).next()
        {
            let mut body_html = body.html();

            // Remove noise elements by replacing them with empty strings
            for noise in &noise_selectors {
                if let Ok(selector) = Selector::parse(noise) {
                    let doc = Html::parse_document(&body_html);
                    let mut cleaned = body_html.clone();
                    for elem in doc.select(&selector) {
                        cleaned = cleaned.replace(&elem.html(), "");
                    }
                    body_html = cleaned;
                }
            }

            return body_html;
        }

        // Ultimate fallback: return original
        html.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_removes_navigation() {
        let html = r#"
            <!DOCTYPE html>
            <html>
                <head><title>Test</title></head>
                <body>
                    <nav>Navigation menu</nav>
                    <main>
                        <article>
                            <h1>Main Content</h1>
                            <p>This is the important content.</p>
                        </article>
                    </main>
                    <footer>Footer content</footer>
                </body>
            </html>
        "#;

        let result = Sanitizer::sanitize(html).unwrap();

        // The result should contain main content
        assert!(result.contains("Main Content"));
        assert!(result.contains("important content"));
    }

    #[test]
    fn test_sanitize_manual_removes_boilerplate() {
        let html = r#"
            <!DOCTYPE html>
            <html>
                <body>
                    <nav class="menu">Menu</nav>
                    <main>
                        <h1>Title</h1>
                        <p>Content</p>
                    </main>
                    <aside>Sidebar</aside>
                </body>
            </html>
        "#;

        let result = Sanitizer::sanitize_manual(html);

        // Should extract the main content
        assert!(result.contains("Title"));
        assert!(result.contains("Content"));
    }
}
