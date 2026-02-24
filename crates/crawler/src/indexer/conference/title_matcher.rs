//! Title matching utilities for conference talk matching.
//!
//! This module consolidates all title cleaning and matching logic used
//! to match conference schedule titles with YouTube video titles.

use std::collections::HashSet;

use crate::tools::youtube::ParsedPlaylistItem;

/// Configuration for title matching behavior.
#[derive(Debug, Clone)]
pub struct TitleMatcherConfig {
    /// The conference name (lowercase) for filtering.
    pub conference: String,
    /// The year for filtering.
    pub year: String,
    /// Minimum similarity threshold for accepting a match (0.0 - 1.0).
    pub threshold: f64,
}

impl Default for TitleMatcherConfig {
    fn default() -> Self {
        Self {
            conference: String::new(),
            year: String::new(),
            threshold: 0.70,
        }
    }
}

/// Result of a title matching operation.
#[derive(Debug, Clone)]
pub struct MatchResult<'a> {
    /// The matched playlist item.
    pub item: &'a ParsedPlaylistItem,
}

/// Title matcher for matching conference schedule titles to YouTube video titles.
#[derive(Debug)]
pub struct TitleMatcher {
    config: TitleMatcherConfig,
    noise_words: HashSet<&'static str>,
}

impl TitleMatcher {
    /// Creates a new title matcher with the given configuration.
    pub fn new(config: TitleMatcherConfig) -> Self {
        let noise_words: HashSet<&'static str> = [
            "rustconf",
            "conf",
            "global",
            "session",
            "workshop",
            "talk",
            "remarks",
            "panel",
            "welcome",
            "intro",
            "introduction",
            "lightning",
            "fireside",
            "discussion",
            "update",
            "keynote",
            "am",
            "pm",
            "the",
            "a",
            "an",
        ]
        .into_iter()
        .collect();

        Self {
            config,
            noise_words,
        }
    }

    /// Clean a title by removing noise and normalizing for comparison.
    ///
    /// This is the main normalization function that handles:
    /// - Extracting quoted content if present
    /// - Converting to lowercase
    /// - Normalizing punctuation to spaces
    /// - Removing conference/year mentions
    /// - Removing common noise words
    pub fn clean_title(&self, title: &str) -> String {
        // First, try to extract quoted title (common in YouTube titles like `Speaker: "Title"`)
        let extracted = Self::extract_quoted_title(title).unwrap_or(title);

        // Convert to lowercase and normalize punctuation to spaces
        let cleaned: String = extracted
            .chars()
            .map(|ch| {
                if ch.is_alphanumeric() || ch.is_whitespace() {
                    ch.to_ascii_lowercase()
                } else {
                    ' '
                }
            })
            .collect();

        // Split into tokens and filter out noise
        let tokens: Vec<&str> = cleaned
            .split_whitespace()
            .filter(|token| {
                !token.is_empty()
                    && token.len() > 1
                    && *token != self.config.conference
                    && *token != self.config.year
                    && !self.noise_words.contains(token)
            })
            .collect();

        tokens.join(" ")
    }

    /// Alternative minimal cleaning for fallback matching.
    ///
    /// This is more aggressive and keeps more words, useful when the
    /// main cleaning removes too much (e.g., "Rust for Linux").
    pub fn clean_title_minimal(title: &str) -> String {
        title
            .chars()
            .map(|ch| {
                if ch.is_alphanumeric() || ch.is_whitespace() {
                    ch.to_ascii_lowercase()
                } else {
                    ' '
                }
            })
            .collect::<String>()
            .split_whitespace()
            .filter(|t| t.len() > 1)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Extract quoted content from a title.
    ///
    /// Handles both straight quotes (`"`) and curly quotes (`""`).
    /// Returns `None` if no valid quoted content is found.
    pub fn extract_quoted_title(title: &str) -> Option<&str> {
        // Try straight quotes first
        if let Some(open) = title.find('"')
            && let Some(close) = title[open + 1..].find('"')
        {
            let start = open + 1;
            let end = open + 1 + close;
            let extracted = title[start..end].trim();
            if !extracted.is_empty() {
                return Some(extracted);
            }
        }

        // Try curly quotes: opening " (U+201C) and closing " (U+201D)
        let open_curly = '\u{201C}';
        let close_curly = '\u{201D}';
        if let Some(open) = title.find(open_curly)
            && let Some(close) = title[open + open_curly.len_utf8()..].find(close_curly)
        {
            let start = open + open_curly.len_utf8();
            let extracted = title[start..start + close].trim();
            if !extracted.is_empty() {
                return Some(extracted);
            }
        }

        None
    }

    /// Calculate string similarity using Jaro-Winkler algorithm.
    ///
    /// Returns a score from 0.0 to 1.0 where 1.0 is an exact match.
    pub fn similarity_score(a: &str, b: &str) -> f64 {
        strsim::jaro_winkler(a, b)
    }

    /// Calculate word-level similarity between two strings.
    ///
    /// This helps match titles where individual words are similar but not identical
    /// (e.g., "accessible" vs "accessibility").
    pub fn word_similarity_score(a: &str, b: &str) -> f64 {
        let a_words: Vec<&str> = a.split_whitespace().collect();
        let b_words: Vec<&str> = b.split_whitespace().collect();

        if a_words.is_empty() || b_words.is_empty() {
            return 0.0;
        }

        let mut matched_words = 0.0;
        let mut used_b: Vec<bool> = vec![false; b_words.len()];

        for a_word in &a_words {
            let mut best_score = 0.0;
            let mut best_idx = None;

            for (i, b_word) in b_words.iter().enumerate() {
                if used_b[i] {
                    continue;
                }
                let score = strsim::jaro_winkler(a_word, b_word);
                if score > best_score && score > 0.85 {
                    best_score = score;
                    best_idx = Some(i);
                }
            }

            if let Some(idx) = best_idx {
                used_b[idx] = true;
                matched_words += best_score;
            }
        }

        // Normalize by the length of the shorter word list
        let min_len = a_words.len().min(b_words.len()) as f64;
        matched_words / min_len
    }

    /// Find the best matching playlist item for a given title.
    ///
    /// Uses multiple matching strategies:
    /// 1. Jaro-Winkler similarity on cleaned titles
    /// 2. Substring containment
    /// 3. Quoted title extraction
    /// 4. Speaker name matching (if provided)
    /// 5. Word-level similarity for partial matches
    pub fn find_match<'a>(
        &self,
        title: &str,
        speakers: &[String],
        items: &'a [ParsedPlaylistItem],
    ) -> Option<MatchResult<'a>> {
        // Strip common prefixes like "Rust Global" that appear in schedule but not in video titles
        let stripped_title = title.strip_prefix("Rust Global ").unwrap_or(title);

        let clean_search = self.clean_title(stripped_title);
        let minimal_search = Self::clean_title_minimal(stripped_title);
        let title_lower = stripped_title.to_lowercase();
        let is_keynote_search = title.to_lowercase().contains("keynote");

        log::debug!(
            "YouTube title match lookup for '{}': cleaned='{}'",
            title,
            clean_search
        );

        let mut best_score = 0.0;
        let mut best_item = None;

        // Also try matching on subtitle (part after colon)
        let subtitle_search = title
            .split_once(':')
            .map(|(_, sub)| sub.trim().to_lowercase())
            .filter(|s| s.len() >= 5);

        for item in items {
            let clean_candidate = self.clean_title(&item.title);
            let candidate_lower = item.title.to_lowercase();

            // Calculate Jaro-Winkler similarity on cleaned titles
            let mut score = Self::similarity_score(&clean_search, &clean_candidate);

            // Also try similarity on original titles (helps with exact substring matches)
            let original_score = Self::similarity_score(&title_lower, &candidate_lower);
            score = score.max(original_score);

            // Try minimal cleaning for cases like "Rust for Linux"
            let minimal_candidate = Self::clean_title_minimal(&item.title);
            let minimal_score = Self::similarity_score(&minimal_search, &minimal_candidate);
            score = score.max(minimal_score);

            // Try word-level similarity for cases like "accessible" vs "accessibility"
            let word_score = Self::word_similarity_score(&clean_search, &clean_candidate);
            score = score.max(word_score);

            // Check if the search title appears as a substring in the candidate
            if candidate_lower.contains(&title_lower) {
                score = score.max(0.90);
            }

            // Check if the subtitle (part after colon) matches
            if let Some(ref subtitle) = subtitle_search
                && candidate_lower.contains(subtitle)
            {
                score = score.max(0.88);
            }

            // Check if the YouTube video's quoted title contains our search title
            if let Some(quoted) = Self::extract_quoted_title(&item.title) {
                let quoted_lower = quoted.to_lowercase();
                let search_similarity = Self::similarity_score(&title_lower, &quoted_lower);
                score = score.max(search_similarity);

                // Also check if quoted title contains our search
                if quoted_lower.contains(&title_lower) || title_lower.contains(&quoted_lower) {
                    score = score.max(0.90);
                }
            }

            // Check if any speaker name is present in the candidate title
            // Boost score if speaker matches (helps when titles are abbreviated)
            for speaker in speakers {
                let speaker_lower = speaker.to_lowercase();
                // Check both full name and individual name parts
                if candidate_lower.contains(&speaker_lower) {
                    score += 0.15;
                    break;
                }
                // Check individual name parts (first/last name)
                for name_part in speaker_lower.split_whitespace() {
                    if name_part.len() > 2 && candidate_lower.contains(name_part) {
                        score += 0.1;
                        break;
                    }
                }
            }

            // Check for significant substring containment
            if clean_search.len() >= 10
                && clean_candidate.len() >= 10
                && (clean_candidate.contains(&clean_search)
                    || clean_search.contains(&clean_candidate))
            {
                score = score.max(0.85);
            }

            // Special handling for keynote matching
            if is_keynote_search && candidate_lower.contains("keynote") {
                score = score.max(0.70);
            }

            if score > best_score {
                best_score = score;
                best_item = Some(item);
            }
        }

        if best_score >= self.config.threshold
            && let Some(item) = best_item
        {
            log::debug!(
                "YouTube title fuzzy match for '{}': '{}' (score {:.2})",
                title,
                item.title,
                best_score
            );
            return Some(MatchResult { item });
        }

        log::debug!(
            "YouTube title match not found for '{}'; best score {:.2}",
            title,
            best_score
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_matcher() -> TitleMatcher {
        TitleMatcher::new(TitleMatcherConfig {
            conference: "rustconf".to_string(),
            year: "2024".to_string(),
            threshold: 0.70,
        })
    }

    #[test]
    fn test_clean_title_removes_noise() {
        let matcher = create_matcher();

        let cleaned =
            matcher.clean_title("Making Open Source Secure by Design | KEYNOTE | RustConf 2024");
        assert!(!cleaned.contains("keynote"));
        assert!(!cleaned.contains("rustconf"));
        assert!(!cleaned.contains("2024"));
        assert!(cleaned.contains("open source"));
    }

    #[test]
    fn test_extract_quoted_title() {
        // Straight quotes
        assert_eq!(
            TitleMatcher::extract_quoted_title(r#"Speaker: "Hello World" | Conf"#),
            Some("Hello World")
        );

        // Curly quotes
        assert_eq!(
            TitleMatcher::extract_quoted_title("Speaker: \u{201C}Hello World\u{201D} | Conf"),
            Some("Hello World")
        );

        // No quotes
        assert_eq!(
            TitleMatcher::extract_quoted_title("Speaker: Hello World | Conf"),
            None
        );
    }

    #[test]
    fn test_similarity_score() {
        // Exact match
        assert!(TitleMatcher::similarity_score("hello", "hello") > 0.99);

        // Very similar
        assert!(TitleMatcher::similarity_score("hello", "hallo") > 0.8);

        // Different
        assert!(TitleMatcher::similarity_score("hello", "world") < 0.5);
    }

    #[test]
    fn test_word_similarity_score() {
        // Similar words
        let score = TitleMatcher::word_similarity_score("accessible gui", "accessibility gui");
        assert!(score > 0.8, "Expected high score, got {}", score);

        // Identical
        let score = TitleMatcher::word_similarity_score("rust for linux", "rust for linux");
        assert!(score > 0.99, "Expected ~1.0, got {}", score);
    }

    #[test]
    fn test_find_match_exact() {
        let matcher = create_matcher();
        let items = vec![ParsedPlaylistItem {
            video_id: "abc123".to_string(),
            title: r#"Speaker: "Making Rust Fast" | RustConf 2024"#.to_string(),
            description: String::new(),
            published_at: "2024-01-01".to_string(),
            thumbnail_url: None,
        }];

        let result = matcher.find_match("Making Rust Fast", &[], &items);
        assert!(result.is_some());
        assert_eq!(result.unwrap().item.video_id, "abc123");
    }

    #[test]
    fn test_find_match_with_speaker_boost() {
        let matcher = create_matcher();
        let items = vec![ParsedPlaylistItem {
            video_id: "abc123".to_string(),
            title: "John Smith: Some Talk | RustConf 2024".to_string(),
            description: String::new(),
            published_at: "2024-01-01".to_string(),
            thumbnail_url: None,
        }];

        let speakers = vec!["John Smith".to_string()];
        let result = matcher.find_match("Some Talk", &speakers, &items);
        assert!(result.is_some());
        // Speaker boost should help find the match
        assert_eq!(result.unwrap().item.video_id, "abc123");
    }
}
