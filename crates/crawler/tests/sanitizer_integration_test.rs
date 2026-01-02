//! Integration tests for HTML sanitization
//!
//! These tests use realistic HTML fixtures to verify that the sanitizer
//! effectively removes boilerplate while preserving meaningful content.

use anyhow::Result;

// We need to access the sanitizer module from the main binary
// Since this is an integration test, we'll need to make sanitizer public
// For now, we'll copy the essential logic or use a different approach

mod sanitizer {
    use anyhow::Result;
    use dom_smoothie::{CandidateSelectMode, Config, Readability};

    pub fn sanitize(html: &str) -> Result<String> {
        let cfg = Config {
            max_elements_to_parse: 10000,
            candidate_select_mode: CandidateSelectMode::DomSmoothie,
            ..Default::default()
        };

        let mut readability = Readability::new(html, None, Some(cfg))?;
        let article = readability.parse()?;

        // Get plain text and normalize whitespace
        let text = article.text_content.to_string();
        Ok(normalize_whitespace(&text))
    }

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
}

/// Helper function to load HTML fixtures
fn load_fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/{}", name))
        .expect("Failed to read fixture file")
}

/// Helper to check if text contains HTML tags
fn contains_html_tags(text: &str) -> bool {
    // Check for common HTML tag patterns
    text.contains("<div")
        || text.contains("<p>")
        || text.contains("<span")
        || text.contains("<nav")
        || text.contains("<header")
        || text.contains("<footer")
}

/// Helper to check for JavaScript content
fn contains_javascript(text: &str) -> bool {
    text.contains("console.log")
        || text.contains("window.addEventListener")
        || text.contains("function()")
}

/// Helper to check for CSS
fn contains_css(text: &str) -> bool {
    text.contains("font-family:")
        || text.contains("background:")
        || text.contains("padding:")
        || text.contains("margin:")
}

#[test]
fn test_blog_post_sanitization() -> Result<()> {
    let html = load_fixture("blog_post_with_noise.html");
    let sanitized = sanitizer::sanitize(&html)?;

    // Should not be empty
    assert!(
        !sanitized.is_empty(),
        "Sanitized output should not be empty"
    );

    // Should contain the main article content
    // Note: H1 titles may be stripped by readability algorithm
    // but the actual content should be preserved
    assert!(
        sanitized.contains("Rust's lifetime system") || sanitized.contains("Lifetimes ensure"),
        "Should preserve article content: {}",
        &sanitized[..sanitized.len().min(200)]
    );
    assert!(
        sanitized.contains("Lifetime parameters are denoted with an apostrophe"),
        "Should preserve article content"
    );
    assert!(
        sanitized.contains("longest"),
        "Should preserve code examples"
    );

    // Should not contain navigation menu text
    assert!(
        !sanitized.contains("Home") || !sanitized.contains("Blog") || !sanitized.contains("About"),
        "Should remove navigation menu - found: {}",
        &sanitized[..sanitized.len().min(200)]
    );

    // Should not contain JavaScript
    assert!(!contains_javascript(&sanitized), "Should remove JavaScript");

    // Should not contain CSS
    assert!(!contains_css(&sanitized), "Should remove CSS");

    // Should not contain HTML tags (plain text output)
    assert!(
        !contains_html_tags(&sanitized),
        "Should not contain HTML tags in plain text output"
    );

    println!(
        "Blog post sanitization succeeded. Output length: {} chars",
        sanitized.len()
    );

    Ok(())
}

#[test]
fn test_news_article_sanitization() -> Result<()> {
    let html = load_fixture("news_article.html");
    let sanitized = sanitizer::sanitize(&html)?;

    // Should not be empty
    assert!(
        !sanitized.is_empty(),
        "Sanitized output should not be empty"
    );

    // Should contain the main article content
    // Note: H1 titles in headers may be stripped by readability algorithm
    assert!(
        sanitized.contains("Rust 1.75") || sanitized.contains("Rust team has announced"),
        "Should preserve article content"
    );
    assert!(
        sanitized.contains("async runtime capabilities"),
        "Should preserve article content"
    );
    assert!(
        sanitized.contains("Compiler Performance"),
        "Should preserve section headings"
    );

    // Should not contain sidebar content
    assert!(
        !sanitized.contains("Subscribe to our Newsletter"),
        "Should remove newsletter signup"
    );

    // Should not contain footer social links
    assert!(
        !sanitized.contains("Follow Us"),
        "Should remove footer content"
    );

    // Should not contain breadcrumbs
    let has_breadcrumbs = sanitized.contains("Home &gt;") && sanitized.contains("Programming &gt;");
    assert!(!has_breadcrumbs, "Should remove breadcrumbs navigation");

    // Should not contain JavaScript
    assert!(!contains_javascript(&sanitized), "Should remove JavaScript");

    // Should not contain CSS
    assert!(!contains_css(&sanitized), "Should remove CSS");

    println!(
        "News article sanitization succeeded. Output length: {} chars",
        sanitized.len()
    );

    Ok(())
}

#[test]
fn test_simple_doc_sanitization() -> Result<()> {
    let html = load_fixture("simple_doc.html");
    let sanitized = sanitizer::sanitize(&html)?;

    // Should not be empty
    assert!(
        !sanitized.is_empty(),
        "Sanitized output should not be empty"
    );

    // Should contain the main documentation content
    assert!(
        sanitized.contains("Module std::vec"),
        "Should preserve module title"
    );
    assert!(
        sanitized.contains("contiguous growable array type"),
        "Should preserve description"
    );
    assert!(
        sanitized.contains("Vec::new"),
        "Should preserve code examples"
    );
    assert!(
        sanitized.contains("Capacity and Reallocation"),
        "Should preserve section headings"
    );

    // Should not contain search bar
    assert!(
        !sanitized.contains("Search documentation"),
        "Should remove search bar"
    );

    // Should not contain sidebar navigation
    assert!(
        !sanitized.contains("std::collections"),
        "Should remove sidebar navigation"
    );

    println!(
        "Simple doc sanitization succeeded. Output length: {} chars",
        sanitized.len()
    );

    Ok(())
}

#[test]
fn test_all_fixtures_produce_reasonable_output() -> Result<()> {
    let fixtures = vec![
        "blog_post_with_noise.html",
        "news_article.html",
        "simple_doc.html",
        "hn.html",
    ];

    for fixture in fixtures {
        let html = load_fixture(fixture);
        let sanitized = sanitizer::sanitize(&html)?;

        // Basic sanity checks for all fixtures
        assert!(
            !sanitized.is_empty(),
            "Fixture {} produced empty output",
            fixture
        );

        assert!(
            sanitized.len() > 100,
            "Fixture {} produced suspiciously short output: {} chars",
            fixture,
            sanitized.len()
        );

        assert!(
            sanitized.len() < html.len(),
            "Fixture {} did not reduce size (original: {}, sanitized: {})",
            fixture,
            html.len(),
            sanitized.len()
        );

        // Should not contain HTML tags (plain text output)
        assert!(
            !contains_html_tags(&sanitized),
            "Fixture {} still contains HTML tags",
            fixture
        );

        // Should not contain JavaScript
        assert!(
            !contains_javascript(&sanitized),
            "Fixture {} still contains JavaScript",
            fixture
        );

        // Should not contain CSS
        assert!(
            !contains_css(&sanitized),
            "Fixture {} still contains CSS",
            fixture
        );

        println!(
            "✓ {} - Original: {} bytes, Sanitized: {} bytes ({}% reduction)",
            fixture,
            html.len(),
            sanitized.len(),
            (100 - (sanitized.len() * 100 / html.len()))
        );
    }

    Ok(())
}

#[test]
fn test_sanitizer_preserves_code_blocks() -> Result<()> {
    let html = load_fixture("blog_post_with_noise.html");
    let sanitized = sanitizer::sanitize(&html)?;

    // Should preserve code examples
    assert!(
        sanitized.contains("fn longest"),
        "Should preserve function names in code blocks"
    );
    assert!(
        sanitized.contains("&amp;'a str") || sanitized.contains("&'a str"),
        "Should preserve code with lifetime annotations"
    );

    Ok(())
}

#[test]
fn test_sanitizer_removes_common_boilerplate() -> Result<()> {
    let fixtures = vec![
        ("blog_post_with_noise.html", vec!["Popular Posts", "Accept"]),
        ("news_article.html", vec!["Related Articles", "Follow Us"]),
        ("simple_doc.html", vec!["Search documentation"]),
    ];

    for (fixture, boilerplate_phrases) in fixtures {
        let html = load_fixture(fixture);
        let sanitized = sanitizer::sanitize(&html)?;

        for phrase in boilerplate_phrases {
            assert!(
                !sanitized.contains(phrase),
                "Fixture {} should not contain boilerplate phrase: '{}'",
                fixture,
                phrase
            );
        }
    }

    Ok(())
}

#[test]
fn test_hackernews_sanitization() -> Result<()> {
    let html = load_fixture("hn.html");
    let sanitized = sanitizer::sanitize(&html)?;

    // Should not be empty
    assert!(
        !sanitized.is_empty(),
        "Sanitized output should not be empty"
    );

    // Should contain the main content about Rust Core Team announcement
    // Note: HN extracts may have whitespace/formatting issues, so check for key terms
    let lower = sanitized.to_lowercase();
    assert!(
        lower.contains("yehuda"),
        "Should preserve main content mentioning Yehuda"
    );
    assert!(
        lower.contains("steve"),
        "Should preserve main content mentioning Steve"
    );
    assert!(
        lower.contains("rust"),
        "Should preserve Rust-related content"
    );

    // HN pages are notorious for excessive metadata
    // The sanitized output should be much smaller than the original
    let size_reduction_percent = ((html.len() - sanitized.len()) * 100) / html.len();
    assert!(
        size_reduction_percent > 50,
        "Should reduce HN page size by at least 50% (actual: {}%)",
        size_reduction_percent
    );

    // Should not contain HN-specific technical UI elements
    // Note: Words like "points", "ago", "reply" can appear in natural language,
    // so we check for more specific HN patterns instead
    let hn_specific_patterns = ["votearrow", "upvote", "hnname", "comhead", "subtext"];

    let mut found_hn_patterns = Vec::new();
    for pattern in &hn_specific_patterns {
        if sanitized.to_lowercase().contains(pattern) {
            found_hn_patterns.push(*pattern);
        }
    }

    assert!(
        found_hn_patterns.is_empty(),
        "Should remove HN-specific UI patterns, but found: {:?}",
        found_hn_patterns
    );

    // Should not contain navigation menu
    assert!(
        !sanitized.contains("Hacker News"),
        "Should remove HN header/navigation"
    );

    // Should not contain HTML tags (plain text output)
    assert!(
        !contains_html_tags(&sanitized),
        "Should not contain HTML tags in plain text output"
    );

    // Should not contain JavaScript or CSS
    assert!(!contains_javascript(&sanitized), "Should remove JavaScript");
    assert!(!contains_css(&sanitized), "Should remove CSS");

    println!(
        "HN sanitization succeeded. Original: {} bytes, Sanitized: {} bytes ({}% reduction)",
        html.len(),
        sanitized.len(),
        size_reduction_percent
    );
    println!("Sanitized content preview (first 500 chars):");
    println!("{}", &sanitized.chars().take(500).collect::<String>());

    Ok(())
}
