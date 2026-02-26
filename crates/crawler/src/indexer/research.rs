//! Indexer for Rust programming language research papers from arXiv
//!
//! This indexer fetches research papers from arXiv API using targeted queries
//! restricted to computer science categories, then applies a relevance filter
//! to ensure papers are actually about the Rust programming language (not
//! rust the fungus, rust the corrosion process, etc.).

use super::Indexer;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::NaiveDate;
use reqwest::header;
use serde::Deserialize;
use storage::Repository;
use tracing::{debug, info, warn};
use types::{Metadata, NewResearchPaper, Url};

/// arXiv API base URL
const ARXIV_API_URL: &str = "http://export.arxiv.org/api/query";

/// Maximum results per query
const MAX_RESULTS_PER_QUERY: usize = 200;

/// Search queries for Rust programming language papers.
///
/// Strategy:
/// - Restrict to CS categories via `cat:cs.*` to avoid biology, physics, agriculture, etc.
/// - Use specific terms that strongly signal the Rust programming language
/// - The `all:` field searches title, abstract, and comments
///
/// Even with these targeted queries, we still apply a post-fetch relevance
/// filter since some CS papers might mention "rust" metaphorically or in passing.
const SEARCH_QUERIES: &[&str] = &[
    // Broad but CS-restricted: "Rust" + "programming"
    "cat:cs.*+AND+all:%22Rust+programming%22",
    // Rust + borrow checker / ownership (very specific to Rust-lang)
    "cat:cs.*+AND+all:Rust+AND+(all:%22borrow+checker%22+OR+all:%22ownership+type%22+OR+all:%22borrow+checking%22)",
    // Rust + memory safety in CS context
    "cat:cs.*+AND+all:Rust+AND+all:%22memory+safety%22",
    // Rust + unsafe code (specific to Rust-lang)
    "cat:cs.*+AND+all:Rust+AND+(all:%22unsafe+code%22+OR+all:%22unsafe+Rust%22)",
    // Rust + cargo / crates / rustc (extremely specific)
    "cat:cs.*+AND+(all:rustc+OR+all:%22cargo+crate%22+OR+all:crates.io+OR+all:%22Rust+compiler%22)",
    // C-to-Rust translation papers
    "cat:cs.*+AND+(all:%22C+to+Rust%22+OR+all:%22C-to-Rust%22)",
    // Rust + formal verification / type system
    "cat:cs.*+AND+all:Rust+AND+(all:%22type+system%22+OR+all:%22formal+verification%22+OR+all:%22lifetime%22)+AND+all:programming",
    // Rust + systems programming
    "cat:cs.*+AND+all:%22Rust+language%22+AND+(all:%22systems+programming%22+OR+all:operating+OR+all:kernel+OR+all:embedded)",
    // Rust + WebAssembly / WASM
    "cat:cs.*+AND+all:Rust+AND+(all:WebAssembly+OR+all:WASM)+AND+all:programming",
    // Rust + concurrency / async
    "cat:cs.*+AND+all:Rust+AND+(all:concurrency+OR+all:%22async%22+OR+all:%22fearless+concurrency%22)+AND+all:programming",
    // Rust + fuzzing / testing
    "cat:cs.*+AND+all:Rust+AND+(all:fuzzing+OR+all:fuzz)+AND+(all:programming+OR+all:compiler+OR+all:crate)",
    // Rust + async runtime / async programming
    "cat:cs.*+AND+all:Rust+AND+(all:%22async+runtime%22+OR+all:%22async+Rust%22+OR+all:tokio+OR+all:async-std)+AND+all:programming",
    // Rust + language interoperability / FFI (general)
    "cat:cs.*+AND+all:Rust+AND+(all:interoperability+OR+all:%22foreign+function%22+OR+all:FFI+OR+all:%22language+binding%22)+AND+all:programming",
    // Rust + C++ interoperability
    "cat:cs.*+AND+all:Rust+AND+all:C%2B%2B+AND+(all:interop+OR+all:binding+OR+all:bridge+OR+all:FFI+OR+all:integration)",
    // Rust + cross-platform development
    "cat:cs.*+AND+all:Rust+AND+(all:%22cross-platform%22+OR+all:%22cross+platform%22+OR+all:%22multi-platform%22)+AND+all:programming",
    // Rust + game development / Bevy / game engines
    "cat:cs.*+AND+all:Rust+AND+(all:bevy+OR+all:%22game+engine%22+OR+all:%22game+development%22+OR+all:wgpu)",
    // Rust + idiomatic code / design patterns / code quality
    "cat:cs.*+AND+all:Rust+AND+(all:idiomatic+OR+all:%22design+pattern%22+OR+all:%22type+state%22+OR+all:%22newtype%22+OR+all:%22code+quality%22)+AND+all:programming",
    // Rust + performance / benchmarking / optimization
    "cat:cs.*+AND+all:%22Rust+language%22+AND+(all:performance+OR+all:benchmark+OR+all:optimization+OR+all:%22zero-cost%22)",
    // Rust + performance comparison with C/C++
    "cat:cs.*+AND+all:Rust+AND+(all:%22performance+comparison%22+OR+all:%22performance+evaluation%22+OR+all:%22runtime+performance%22)+AND+all:programming",
    // Rust + scalability / distributed systems
    "cat:cs.*+AND+all:Rust+AND+(all:scalability+OR+all:scalable+OR+all:%22distributed+system%22+OR+all:%22high+performance%22)+AND+all:programming",
    // Rust + systems design / software architecture
    "cat:cs.*+AND+all:Rust+AND+(all:%22software+architecture%22+OR+all:%22system+design%22+OR+all:%22systems+design%22+OR+all:microservice)+AND+all:programming",
    // Rust + software design / modular architecture
    "cat:cs.*+AND+all:Rust+AND+(all:%22software+design%22+OR+all:%22modular+design%22+OR+all:%22component+architecture%22+OR+all:%22API+design%22)+AND+all:programming",
    // Rust vs C++ comparison
    "cat:cs.*+AND+all:Rust+AND+all:C%2B%2B+AND+(all:comparison+OR+all:versus+OR+all:%22compared+to%22+OR+all:%22compare%22)",
    // Rust vs Java / Go comparison
    "cat:cs.*+AND+all:Rust+AND+(all:Java+OR+all:Golang+OR+all:%22Go+language%22)+AND+(all:comparison+OR+all:versus+OR+all:%22compared+to%22)+AND+all:programming",
    // Rust vs newer systems languages (Zig, Nim, Swift)
    "cat:cs.*+AND+all:Rust+AND+(all:Zig+OR+all:Nim+OR+all:Swift)+AND+(all:comparison+OR+all:versus+OR+all:%22compared+to%22)+AND+all:%22programming+language%22",
    // Rust + language comparison / empirical study
    "cat:cs.*+AND+all:Rust+AND+(all:%22language+comparison%22+OR+all:%22comparative+study%22+OR+all:%22empirical+study%22+OR+all:%22empirical+comparison%22)+AND+all:programming",
    // Rust + migration / adoption / porting
    "cat:cs.*+AND+all:Rust+AND+(all:migration+OR+all:migrating+OR+all:porting+OR+all:%22Rust+adoption%22+OR+all:%22adopt+Rust%22+OR+all:rewrite)+AND+all:programming",
    // Rust + best practices / guidelines / coding standards
    "cat:cs.*+AND+all:Rust+AND+(all:%22best+practice%22+OR+all:guideline+OR+all:%22coding+standard%22+OR+all:%22coding+convention%22)+AND+all:programming",
    // Rust + API guidelines / API usability / library design
    "cat:cs.*+AND+all:Rust+AND+(all:%22API+guideline%22+OR+all:%22API+usability%22+OR+all:%22library+design%22+OR+all:%22API+evolution%22)+AND+all:programming",
];

/// Keywords that strongly indicate a paper is about the Rust programming language.
/// We check title + abstract for at least one of these (case-insensitive).
const RUSTLANG_STRONG_SIGNALS: &[&str] = &[
    "rust programming",
    "rust language",
    "rust compiler",
    "rustc",
    "cargo crate",
    "crates.io",
    "borrow checker",
    "borrow checking",
    "ownership type",
    "ownership system",
    "lifetime annotation",
    "lifetime system",
    "unsafe rust",
    "unsafe code",
    "safe rust",
    "memory safety",
    "c-to-rust",
    "c to rust",
    "rust-to-c",
    "transpil", // transpile, transpiler, transpilation, transpiling
    "rustfmt",
    "clippy lint",
    "rust crate",
    "rust library",
    "rust ecosystem",
    "rust community",
    "rust program",
    "fearless concurrency",
    "rust trait",
    "rust macro",
    "proc macro",
    "procedural macro",
    "rust type system",
    "rust's type",
    "rust's ownership",
    "rust's borrow",
    "rust's lifetime",
    "tokio",
    "actix",
    "axum",
    "serde",
    "rsmpi",
    "miri",
    "rust-based",
    "written in rust",
    "implemented in rust",
    "rust implementation",
    "embedded rust",
    "rust for linux",
    "rustacean",
    "ferris",
    "rust bindings",
    "rust ffi",
    "rust api",
    "webassembly", // combined with "rust" mention in CS category, very likely relevant
    "cargo.toml",
    "rustup",
    "rust toolchain",
    "rust binary",
    "crate ecosystem",
    "pyo3",
    "rkyv",
    "rust project",
    "rust code",
    "rust source",
    "rust package",
    "rust application",
    "rust framework",
    "rust develop",
    "bevy",
    "wgpu",
    "rust game",
    "async runtime",
    "cxx crate",
    "autocxx",
    "bindgen",
    "cbindgen",
    "cross-platform rust",
    "rust interop",
    "idiomatic rust",
    "rust idiom",
    "rust pattern",
    "rust design",
    "rust architect",
    "rust performance",
    "rust benchmark",
    "zero-cost abstraction",
    "rust scalab",
    "rust microservice",
    "rust vs",
    "rust versus",
    "compared to rust",
    "compared with rust",
    "rust adoption",
    "adopt rust",
    "migrate to rust",
    "migrating to rust",
    "porting to rust",
    "rewrite in rust",
    "rewritten in rust",
    "rust migration",
    "rust best practice",
    "rust guideline",
    "rust api",
];

/// Words/phrases that indicate a paper is NOT about the Rust programming language.
/// If these appear prominently and no strong signals are found, skip the paper.
const NEGATIVE_SIGNALS: &[&str] = &[
    "coffee leaf rust",
    "coffee rust",
    "wheat rust",
    "soybean rust",
    "leaf rust",
    "stem rust",
    "stripe rust",
    "crown rust",
    "cedar rust",
    "white rust",
    "plant rust",
    "fungal rust",
    "rust fungi",
    "rust pathogen",
    "rust disease",
    "phakopsora",
    "puccinia",
    "melampsora",
    "hemileia",
    "uromyces",
    "urediniospore",
    "iron oxide",
    "corrosion",
    "oxidation",
    "rusting",
    "rust belt",
    "rust removal",
    "rust prevention",
    "rust stain",
    "rusty surface",
    "trust region", // "trust" often gets matched, common in ML optimization
    "trust-region",
];

/// Stats collected during indexing
#[derive(Debug, Default)]
struct ResearchStats {
    processed: usize,
    skipped_irrelevant: usize,
    skipped_existing: usize,
    failed: usize,
}

#[derive(Debug, Deserialize)]
struct ArxivFeed {
    #[serde(rename = "entry", default)]
    entries: Vec<ArxivEntry>,
}

#[derive(Debug, Deserialize)]
struct ArxivEntry {
    id: String,
    title: String,
    summary: String,
    published: String,
    #[serde(rename = "author", default)]
    authors: Vec<Author>,
    #[serde(rename = "category", default)]
    categories: Vec<Category>,
}

#[derive(Debug, Deserialize)]
struct Author {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Category {
    #[serde(rename = "@term")]
    term: String,
}

/// Indexer for research papers from arXiv
pub struct ResearchIndexer {
    client: reqwest::Client,
    dry_run: bool,
    overwrite: bool,
}

impl Default for ResearchIndexer {
    fn default() -> Self {
        Self::new()
    }
}

impl ResearchIndexer {
    /// Creates a new research paper indexer
    pub fn new() -> Self {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static("corrode/search crawler (arxiv indexer)"),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to build reqwest client");

        Self {
            client,
            dry_run: false,
            overwrite: false,
        }
    }

    /// Fetches papers from arXiv for a given query, handling pagination
    async fn fetch_papers(&self, query: &str) -> Result<Vec<ArxivEntry>> {
        info!("Fetching papers for query: {}", query);
        let mut all_entries = Vec::new();
        let mut start = 0;
        let page_size = 100;

        loop {
            let url = format!(
                "{}?search_query={}&start={}&max_results={}&sortBy=submittedDate&sortOrder=descending",
                ARXIV_API_URL, query, start, page_size
            );

            let response = self
                .client
                .get(&url)
                .send()
                .await
                .context("Failed to fetch from arXiv API")?;

            let body = response
                .text()
                .await
                .context("Failed to read response body")?;

            let feed: ArxivFeed =
                quick_xml::de::from_str(&body).context("Failed to parse arXiv feed")?;

            let count = feed.entries.len();
            all_entries.extend(feed.entries);

            if count < page_size || all_entries.len() >= MAX_RESULTS_PER_QUERY {
                break;
            }

            start += count;

            // Be polite to arXiv API between pages
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }

        info!("Found {} papers for query", all_entries.len());
        Ok(all_entries)
    }

    /// Checks if an arXiv entry is actually about the Rust programming language.
    ///
    /// We combine title + abstract into a single text and check for:
    /// 1. Negative signals (plant rust, corrosion, etc.) → reject
    /// 2. Strong positive signals (borrow checker, rustc, etc.) → accept
    /// 3. CS category + mentions "Rust" in a programming context → accept
    /// 4. Otherwise → reject
    fn is_rustlang_relevant(entry: &ArxivEntry) -> bool {
        let text = format!(
            "{} {}",
            entry.title.to_lowercase(),
            entry.summary.to_lowercase()
        );

        // First check: reject if negative signals are present
        for neg in NEGATIVE_SIGNALS {
            if text.contains(&neg.to_lowercase()) {
                debug!(
                    "Rejected (negative signal '{}'): {}",
                    neg,
                    entry.title.trim()
                );
                return false;
            }
        }

        // Check if the paper is in a CS category
        let is_cs = entry.categories.iter().any(|c| c.term.starts_with("cs."));
        if !is_cs {
            debug!(
                "Rejected (not CS category): {} [categories: {}]",
                entry.title.trim(),
                entry
                    .categories
                    .iter()
                    .map(|c| c.term.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return false;
        }

        // Check for strong positive signals
        for signal in RUSTLANG_STRONG_SIGNALS {
            if text.contains(&signal.to_lowercase()) {
                return true;
            }
        }

        // Additional heuristic: title contains "Rust" (capitalized, as a proper noun)
        // and abstract mentions programming-related terms
        let title_has_rust = entry.title.contains("Rust") || entry.title.contains("RUST");
        let has_programming_context = text.contains("compile")
            || text.contains("compiler")
            || text.contains("type system")
            || text.contains("programming")
            || text.contains("software")
            || text.contains("source code")
            || text.contains("binary")
            || text.contains("vulnerability")
            || text.contains("bug")
            || text.contains("testing")
            || text.contains("verification")
            || text.contains("concurrency")
            || text.contains("parallelism")
            || text.contains("async")
            || text.contains("runtime")
            || text.contains("operating system")
            || text.contains("kernel")
            || text.contains("embedded")
            || text.contains("firmware")
            || text.contains("llvm")
            || text.contains("webassembly")
            || text.contains("benchmark");

        if title_has_rust && has_programming_context {
            return true;
        }

        debug!(
            "Rejected (no strong Rust-lang signal): {}",
            entry.title.trim()
        );
        false
    }

    fn extract_arxiv_id(url: &str) -> Option<String> {
        // Extract arXiv ID from URL like "http://arxiv.org/abs/2301.00000v1"
        url.split('/').next_back().map(|id| {
            // Remove version suffix
            id.split('v').next().unwrap_or(id).to_string()
        })
    }

    fn parse_date(date_str: &str) -> Result<NaiveDate> {
        // arXiv dates are in ISO 8601 format: "2023-01-15T12:00:00Z"
        let date =
            chrono::DateTime::parse_from_rfc3339(date_str).context("Failed to parse date")?;
        Ok(date.naive_utc().date())
    }

    /// Check if a research paper URL already exists in the database
    async fn paper_exists(&self, repo: &Repository, url: &Url) -> Result<bool> {
        repo.research_paper_exists(url).await
    }

    async fn process_paper(
        &self,
        entry: &ArxivEntry,
        repo: &Repository,
        stats: &mut ResearchStats,
    ) -> Result<()> {
        let arxiv_id = Self::extract_arxiv_id(&entry.id).context("Failed to extract arXiv ID")?;

        // Relevance filter
        if !Self::is_rustlang_relevant(entry) {
            stats.skipped_irrelevant += 1;
            return Ok(());
        }

        let url = Url::parse(&entry.id).context("Failed to parse URL")?;

        // Skip if already indexed and not overwriting
        if !self.overwrite {
            match self.paper_exists(repo, &url).await {
                Ok(true) => {
                    debug!("Skipping existing paper: {}", arxiv_id);
                    stats.skipped_existing += 1;
                    return Ok(());
                }
                Ok(false) => {}
                Err(e) => {
                    // If the table doesn't exist yet or other DB error, log and continue
                    debug!("Error checking paper existence: {}", e);
                }
            }
        }

        let date = Self::parse_date(&entry.published)
            .unwrap_or_else(|_| chrono::Utc::now().naive_utc().date());

        let authors = entry
            .authors
            .iter()
            .map(|a| a.name.clone())
            .collect::<Vec<_>>()
            .join(", ");

        let category = entry
            .categories
            .first()
            .map(|c| c.term.clone())
            .unwrap_or_else(|| "arXiv".to_string());

        let paper = NewResearchPaper {
            metadata: Metadata {
                title: entry.title.trim().replace('\n', " ").to_string(),
                url,
                category,
                date,
            },
            authors,
            abstract_text: entry.summary.trim().to_string(),
            text: String::new(), // Full text not available via API
            paper_id: Some(format!("arXiv:{}", arxiv_id)),
            publication: Some("arXiv".to_string()),
        };

        if self.dry_run {
            info!("Would index paper: {} - {}", arxiv_id, paper.metadata.title);
            stats.processed += 1;
            return Ok(());
        }

        repo.insert_research_paper(&paper)
            .await
            .context("Failed to insert research paper")?;

        info!("Indexed paper: {} - {}", arxiv_id, paper.metadata.title);
        stats.processed += 1;
        Ok(())
    }
}

#[async_trait]
impl Indexer for ResearchIndexer {
    fn name(&self) -> &'static str {
        "research"
    }

    async fn index(&mut self, repo: &Repository) -> Result<()> {
        info!("Starting research paper indexing from arXiv");
        let mut stats = ResearchStats::default();

        // Deduplicate across queries: track arXiv IDs we've already seen
        let mut seen_ids = std::collections::HashSet::new();

        for query in SEARCH_QUERIES {
            match self.fetch_papers(query).await {
                Ok(papers) => {
                    for entry in papers {
                        // Deduplicate across queries
                        let arxiv_id = Self::extract_arxiv_id(&entry.id).unwrap_or_default();
                        if seen_ids.contains(&arxiv_id) {
                            continue;
                        }
                        seen_ids.insert(arxiv_id);

                        if let Err(e) = self.process_paper(&entry, repo, &mut stats).await {
                            warn!("Failed to process paper '{}': {}", entry.title.trim(), e);
                            stats.failed += 1;
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch papers for query '{}': {}", query, e);
                }
            }

            // Be polite to arXiv API - wait between queries
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }

        info!(
            "Research paper indexing complete: processed={}, skipped_irrelevant={}, skipped_existing={}, failed={}",
            stats.processed, stats.skipped_irrelevant, stats.skipped_existing, stats.failed
        );

        Ok(())
    }

    fn set_debug(&mut self, _value: bool) {}

    fn set_dry_run(&mut self, value: bool) {
        self.dry_run = value;
    }

    fn set_overwrite(&mut self, value: bool) {
        self.overwrite = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(title: &str, summary: &str, categories: &[&str]) -> ArxivEntry {
        ArxivEntry {
            id: "http://arxiv.org/abs/2301.00001v1".to_string(),
            title: title.to_string(),
            summary: summary.to_string(),
            published: "2023-01-15T12:00:00Z".to_string(),
            authors: vec![Author {
                name: "Test Author".to_string(),
            }],
            categories: categories
                .iter()
                .map(|t| Category {
                    term: t.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn test_accepts_clear_rust_programming_paper() {
        let entry = make_entry(
            "Compiling C to Safe Rust, Formalized",
            "We present a formalized translation from C to safe Rust code using the borrow checker.",
            &["cs.PL"],
        );
        assert!(ResearchIndexer::is_rustlang_relevant(&entry));
    }

    #[test]
    fn test_accepts_unsafe_rust_paper() {
        let entry = make_entry(
            "SafeFFI: Sanitization at the Boundary Between Safe and Unsafe Code in Rust",
            "We study the boundary between safe and unsafe Rust in mixed-language applications.",
            &["cs.SE", "cs.PL"],
        );
        assert!(ResearchIndexer::is_rustlang_relevant(&entry));
    }

    #[test]
    fn test_accepts_rustc_paper() {
        let entry = make_entry(
            "An Empirical Study of Rust-Specific Bugs in the rustc Compiler",
            "We analyze bugs found in rustc, the Rust compiler.",
            &["cs.SE"],
        );
        assert!(ResearchIndexer::is_rustlang_relevant(&entry));
    }

    #[test]
    fn test_rejects_coffee_leaf_rust() {
        let entry = make_entry(
            "Threshold-based impulsive biocontrol for coffee leaf rust",
            "We study the dynamics of coffee leaf rust disease caused by Hemileia vastatrix.",
            &["math.DS"],
        );
        assert!(!ResearchIndexer::is_rustlang_relevant(&entry));
    }

    #[test]
    fn test_rejects_soybean_rust() {
        let entry = make_entry(
            "Numerical simulation of Phakopsora pachyrhizi urediniospores",
            "Atmospheric transport of soybean rust spores in South America.",
            &["q-bio.PE"],
        );
        assert!(!ResearchIndexer::is_rustlang_relevant(&entry));
    }

    #[test]
    fn test_rejects_trust_region_ml_paper() {
        let entry = make_entry(
            "Trust-Region Adaptive Policy Optimization",
            "We propose a trust-region method for reinforcement learning with rust-like efficiency.",
            &["cs.LG"],
        );
        assert!(!ResearchIndexer::is_rustlang_relevant(&entry));
    }

    #[test]
    fn test_rejects_non_cs_paper() {
        let entry = make_entry(
            "Oscillate and Renormalize: Phonons in Rust Systems",
            "We study Kondo effect in flat band systems with Rust impurities.",
            &["cond-mat.str-el"],
        );
        assert!(!ResearchIndexer::is_rustlang_relevant(&entry));
    }

    #[test]
    fn test_accepts_memory_safety_cs_paper() {
        let entry = make_entry(
            "TYPEPULSE: Detecting Type Confusion Bugs in Rust Programs",
            "We present TYPEPULSE, a tool for detecting type confusion bugs in Rust programs through static analysis.",
            &["cs.CR"],
        );
        assert!(ResearchIndexer::is_rustlang_relevant(&entry));
    }

    #[test]
    fn test_accepts_rust_for_linux() {
        let entry = make_entry(
            "Safe and usable kernel extensions with Rex",
            "We present Rex, a framework for writing Rust-based kernel extensions for Linux.",
            &["cs.OS"],
        );
        assert!(ResearchIndexer::is_rustlang_relevant(&entry));
    }

    #[test]
    fn test_accepts_crates_io_paper() {
        let entry = make_entry(
            "A Comprehensive Study on Vulnerable Dependencies",
            "We study vulnerable dependencies in open-source software, analyzing packages from crates.io and npm.",
            &["cs.SE"],
        );
        assert!(ResearchIndexer::is_rustlang_relevant(&entry));
    }

    #[test]
    fn test_rejects_corrosion_paper_in_cs() {
        let entry = make_entry(
            "Detecting Rust and Corrosion in Infrastructure",
            "We use deep learning to detect corrosion and iron oxide rust on bridge surfaces.",
            &["cs.CV"],
        );
        assert!(!ResearchIndexer::is_rustlang_relevant(&entry));
    }

    #[test]
    fn test_extract_arxiv_id() {
        assert_eq!(
            ResearchIndexer::extract_arxiv_id("http://arxiv.org/abs/2301.00001v1"),
            Some("2301.00001".to_string())
        );
        assert_eq!(
            ResearchIndexer::extract_arxiv_id("http://arxiv.org/abs/2301.00001"),
            Some("2301.00001".to_string())
        );
    }
}
