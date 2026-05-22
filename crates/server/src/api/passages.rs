//! Passage extraction for "search-within-document" and multi-snippet hits.
//!
//! This module owns the logic for finding the most relevant excerpts of a
//! single document given a set of search terms. It's intentionally simple,
//! string-based, and dependency-free: FTS5's `snippet()` function only
//! returns one excerpt per row, but LLM-style consumers benefit from
//! several non-overlapping passages with stable character offsets.
//!
//! ## Algorithm
//!
//! 1. Normalize the document text to a single lowercase copy for matching.
//! 2. For each search term, find every case-insensitive occurrence. We only
//!    require a substring match: this means "async" matches "asynchronous"
//!    too, which is the desired behavior for a Porter-stemmed FTS index.
//! 3. Walk the merged, sorted match list and cluster matches whose start
//!    positions fall within `window / 2` chars of each other into the same
//!    candidate passage.
//! 4. Expand each cluster to a `window`-char excerpt centered on its first
//!    match, then snap both ends to the nearest whitespace so we don't cut
//!    words in half.
//! 5. Drop overlapping passages, keeping the one with more matches (and
//!    earlier start as tiebreak). Return up to `max` of the remaining
//!    passages, ordered by descending match count.
//!
//! Passages are returned as plain text. The API surface does not include
//! a pre-marked copy — callers that need highlighting can use the
//! character offsets and the (already-known) query terms to render it
//! themselves.

use std::cmp::Ordering;

/// Result of extracting one passage from a longer document.
#[derive(Debug, Clone)]
pub(crate) struct Passage {
    /// Character start offset into the original document text (0-based,
    /// inclusive). Counted in `char` indices, not bytes, so it's safe to use
    /// from any language environment.
    pub char_start: usize,
    /// Character end offset (exclusive).
    pub char_end: usize,
    /// The raw passage text (no markup).
    pub text: String,
    /// Number of (possibly overlapping) term matches in the passage.
    pub match_count: usize,
}

/// Extracts up to `max` non-overlapping ranked passages from `text`.
///
/// `terms` are matched case-insensitively as substrings; empty terms are
/// ignored. `window` controls the approximate passage width in characters
/// (a reasonable default is 400). Returns an empty `Vec` when `text` is
/// empty, when `terms` is empty, or when no term occurs in `text`.
pub(crate) fn extract(text: &str, terms: &[&str], max: usize, window: usize) -> Vec<Passage> {
    if text.is_empty() || terms.is_empty() || max == 0 {
        return Vec::new();
    }
    let terms: Vec<String> = terms
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }

    // Build a char-indexed view of the text. We need char offsets (not byte
    // offsets) so the values are safe to expose to JSON clients.
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let lower: String = chars.iter().flat_map(|c| c.to_lowercase()).collect();

    // For each term, find all char-aligned occurrences in `lower`. We do this
    // by stepping through the byte index returned by `find` and mapping it
    // back to a char index.
    let mut matches: Vec<(usize, usize, usize)> = Vec::new(); // (char_start, char_end, term_idx)
    for (term_idx, term) in terms.iter().enumerate() {
        let term_chars = term.chars().count();
        let term_bytes = term.len();
        let mut byte_cursor = 0usize;
        while let Some(found_byte) = lower[byte_cursor..].find(term.as_str()) {
            let abs_byte = byte_cursor + found_byte;
            // Convert byte offset to char offset by counting chars up to it.
            let char_start = lower[..abs_byte].chars().count();
            matches.push((char_start, char_start + term_chars, term_idx));
            byte_cursor = abs_byte + term_bytes.max(1);
        }
    }
    if matches.is_empty() {
        return Vec::new();
    }

    matches.sort_by_key(|(s, e, _)| (*s, *e));

    // Cluster nearby matches into candidate passages.
    let cluster_gap = window / 2;
    let mut clusters: Vec<Vec<(usize, usize, usize)>> = Vec::new();
    for m in matches {
        let pushed = if let Some(last) = clusters.last() {
            let last_end = last.last().map_or(0, |(_, e, _)| *e);
            m.0.saturating_sub(last_end) <= cluster_gap
        } else {
            false
        };
        if pushed {
            clusters.last_mut().unwrap().push(m);
        } else {
            clusters.push(vec![m]);
        }
    }

    // Expand each cluster into a passage, snapping to whitespace.
    let total_chars = chars.len();
    let mut passages: Vec<Passage> = Vec::with_capacity(clusters.len());
    for cluster in clusters {
        let first_start = cluster.first().unwrap().0;
        let last_end = cluster.last().unwrap().1;
        let span = last_end - first_start;
        // Pad each side so the total passage width is ~window.
        let pad = window.saturating_sub(span) / 2;
        let raw_start = first_start.saturating_sub(pad);
        let raw_end = (last_end + pad).min(total_chars);
        let (snap_start, snap_end) = snap_to_whitespace(&chars, raw_start, raw_end);

        let passage_text: String = chars[snap_start..snap_end].iter().collect();
        passages.push(Passage {
            char_start: snap_start,
            char_end: snap_end,
            text: passage_text,
            match_count: cluster.len(),
        });
    }

    // Drop overlapping passages, preferring higher match counts.
    passages.sort_by(|a, b| match b.match_count.cmp(&a.match_count) {
        Ordering::Equal => a.char_start.cmp(&b.char_start),
        ord => ord,
    });
    let mut chosen: Vec<Passage> = Vec::with_capacity(max);
    for p in passages {
        if chosen.len() >= max {
            break;
        }
        let overlaps = chosen
            .iter()
            .any(|c| p.char_start < c.char_end && c.char_start < p.char_end);
        if !overlaps {
            chosen.push(p);
        }
    }
    chosen
}

/// Snaps an `[start, end)` char range outward so the boundaries fall on
/// whitespace (or document edges), avoiding mid-word cuts.
fn snap_to_whitespace(chars: &[char], mut start: usize, mut end: usize) -> (usize, usize) {
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    while end < chars.len() && !chars[end - 1].is_whitespace() {
        end += 1;
    }
    (start, end.min(chars.len()))
}

/// Rough token estimate using a 4-chars-per-token heuristic. Cheap, language-
/// agnostic, and good enough for budgeting LLM context windows.
#[must_use]
pub(crate) fn estimate_tokens(text: &str) -> u32 {
    let chars = text.chars().count();
    u32::try_from(chars.div_ceil(4)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_passages_around_terms() {
        let text = "Rust has a borrow checker. The borrow checker prevents data races. \
                    Async runtimes like Tokio build on top of futures. \
                    Some other unrelated content lives here for filler.";
        let p = extract(text, &["borrow", "tokio"], 5, 80);
        assert!(!p.is_empty());
        // Top hit should be the borrow-checker cluster (two matches).
        assert!(p[0].text.to_lowercase().contains("borrow"));
        // Plain text — no markup leaks through.
        assert!(!p[0].text.contains("<mark>"));
    }

    #[test]
    fn empty_inputs_return_empty() {
        assert!(extract("", &["foo"], 3, 100).is_empty());
        assert!(extract("hello", &[], 3, 100).is_empty());
        assert!(extract("hello", &["xyz"], 3, 100).is_empty());
    }

    #[test]
    fn highlights_case_insensitively() {
        let p = extract("Async Rust is fun. async is a keyword.", &["async"], 3, 60);
        assert_eq!(p.len(), 1);
        // Both cases are inside the single returned passage.
        let lower = p[0].text.to_lowercase();
        assert!(lower.matches("async").count() >= 2);
        assert!(!p[0].text.contains("<mark>"));
    }

    #[test]
    fn passages_dont_overlap() {
        let text = "alpha ".repeat(200);
        let p = extract(&text, &["alpha"], 3, 50);
        for i in 0..p.len() {
            for j in (i + 1)..p.len() {
                assert!(
                    p[i].char_end <= p[j].char_start || p[j].char_end <= p[i].char_start,
                    "passages {i} and {j} overlap"
                );
            }
        }
    }
}
