//! Parser for This Week in Rust (TWiR) Markdown content files

use anyhow::{Result, bail};
use chrono::NaiveDate;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use types::{EntryId, Quote};

/// This Week in Rust configuration
const TWIR_GITHUB_OWNER: &str = "rust-lang";
const TWIR_GITHUB_REPO: &str = "this-week-in-rust";
const TWIR_GITHUB_BRANCH: &str = "main";
const TWIR_DATE_FORMATS: [&str; 2] = ["%Y-%m-%d", "%Y-%m-%d %H:%M"];

/// Result of parsing a TWiR file
pub struct ParseResult {
    /// Extracted entries
    pub entries: Vec<EntryId>,
    /// Extracted quotes
    pub quotes: Vec<Quote>,
    /// Issue number
    pub issue_number: Option<u32>,
}

/// Parser for TWiR content
pub struct TwirParser {
    client: reqwest::Client,
}

impl TwirParser {
    /// Creates a new parser instance
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Fetches TWiR entries from GitHub
    pub async fn fetch_twir_entries(&self) -> Result<Vec<serde_json::Value>> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/contents/content?ref={}",
            TWIR_GITHUB_OWNER, TWIR_GITHUB_REPO, TWIR_GITHUB_BRANCH
        );

        let response = self
            .client
            .get(&url)
            .header("User-Agent", "Rust GitHub API Client")
            .send()
            .await?;

        response.error_for_status_ref()?;
        Ok(response.json().await?)
    }

    /// Downloads content from a URL
    pub async fn download_content(&self, url: &str) -> Result<String> {
        Ok(self
            .client
            .get(url)
            .header("User-Agent", "Rust GitHub API Client")
            .send()
            .await?
            .text()
            .await?)
    }

    /// Parses a TWiR file into entries and quotes
    pub fn parse_file(&self, content: &str) -> Option<ParseResult> {
        let (meta, body) = content.split_once("\n\n")?;
        let date = self.parse_date_from_meta(meta).ok()?;
        let mut issue_number = self.parse_number_from_meta(meta);

        if issue_number.is_none() {
            issue_number = self.parse_number_from_body(body);
        }

        // Extract quotes from the full body
        let quotes = self.extract_quotes(body, date);

        let body = self.skip_intro_section(body);
        let entries = self.content_to_entry_ids(&body, date);

        Some(ParseResult {
            entries,
            quotes,
            issue_number,
        })
    }

    /// Parses the issue number from TWiR metadata
    fn parse_number_from_meta(&self, meta: &str) -> Option<u32> {
        let mut title_number = None;

        for line in meta.lines() {
            if let Some((key, val)) = line.split_once(":") {
                let key = key.trim();
                let val = val.trim();

                if key == "Number" {
                    return val.parse().ok();
                }

                if key == "Title"
                    && let Some(num_str) = val.strip_prefix("This Week in Rust ")
                    && let Ok(num) = num_str.parse::<u32>()
                {
                    title_number = Some(num);
                }
            }
        }
        title_number
    }

    /// Parses the issue number from the body content
    fn parse_number_from_body(&self, body: &str) -> Option<u32> {
        for line in body.lines().take(10) {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("# This Week in Rust ") {
                let num_str = rest.split_whitespace().next().unwrap_or(rest);
                if let Ok(num) = num_str.parse::<u32>() {
                    return Some(num);
                }
            }
        }
        None
    }

    /// Parses the date from TWiR metadata
    fn parse_date_from_meta(&self, meta: &str) -> Result<NaiveDate> {
        for line in meta.lines() {
            if let Some((key, date_str)) = line.split_once(":")
                && key.trim() == "Date"
            {
                let date_formats = TWIR_DATE_FORMATS;

                for format in date_formats.iter() {
                    if let Ok(date) = NaiveDate::parse_from_str(date_str.trim(), format) {
                        return Ok(date);
                    }
                }
                bail!("Found date but failed to parse it: {}", date_str);
            }
        }
        bail!("Did not find post date")
    }

    /// Skips the intro section of TWiR content
    fn skip_intro_section(&self, body: &str) -> String {
        let mut in_header = false;
        let mut header_line = 0;

        for (i, line) in body.lines().enumerate() {
            if line.trim_start().starts_with("#") {
                in_header = true;
                header_line = i;
                continue;
            }

            if in_header && line.trim_start().starts_with("-") {
                return body
                    .lines()
                    .skip(header_line)
                    .collect::<Vec<&str>>()
                    .join("\n");
            }
        }

        body.to_string()
    }

    /// Extracts quotes from the content
    fn extract_quotes(&self, content: &str, date: NaiveDate) -> Vec<Quote> {
        let mut quotes = Vec::new();
        let mut lines = content.lines().peekable();

        while let Some(line) = lines.next() {
            if line.trim_start().starts_with("#")
                && line.to_lowercase().contains("quote of the week")
            {
                let mut quote_text = String::new();
                let mut author = String::new();
                let mut url = None;

                // Read until next header or end
                while let Some(line) = lines.peek() {
                    if line.trim_start().starts_with("#") {
                        break;
                    }
                    let line = lines.next().unwrap();
                    let trimmed = line.trim();

                    if trimmed.is_empty() {
                        continue;
                    }

                    if let Some(text) = trimmed.strip_prefix(">") {
                        if !quote_text.is_empty() {
                            quote_text.push('\n');
                        }
                        quote_text.push_str(text.trim());
                    } else if !quote_text.is_empty() && author.is_empty() {
                        // Assume this is the attribution line if we have text and no author yet
                        // Filter out "Thanks to..." lines which usually come after attribution
                        if trimmed.starts_with("Thanks to") {
                            continue;
                        }

                        // Parse attribution line for Author and URL
                        // Remove leading dashes/em-dashes
                        let clean_line = trimmed.trim_start_matches(['—', '–', '-']).trim();

                        let parser = Parser::new(clean_line);
                        for event in parser {
                            match event {
                                Event::Text(text) => author.push_str(&text),
                                Event::Start(Tag::Link { dest_url, .. }) => {
                                    if let Ok(u) = url::Url::parse(&dest_url) {
                                        url = Some(u);
                                    }
                                }
                                Event::Code(text) => author.push_str(&text),
                                _ => {}
                            }
                        }
                    }
                }

                if !quote_text.is_empty() && !author.is_empty() {
                    quotes.push(Quote {
                        text: quote_text,
                        author: author.trim().to_string(),
                        url,
                        date,
                    });
                }
            }
        }
        quotes
    }

    /// Converts Markdown content to EntryId structs
    fn content_to_entry_ids(&self, content: &str, date: NaiveDate) -> Vec<EntryId> {
        let mut entries = Vec::new();
        let mut current_category = String::new();
        let mut current_title = String::new();
        let mut current_url = String::new();
        let mut in_link = false;

        let parser = Parser::new_ext(content, Options::all());

        for event in parser {
            match event {
                Event::Start(Tag::Heading { .. }) => {
                    current_category = String::new();
                }
                Event::Start(Tag::Link { dest_url, .. }) => {
                    in_link = true;
                    current_url = dest_url.to_string();
                }
                Event::End(TagEnd::Link) => {
                    in_link = false;
                    // Don't add entries from "Quote of the Week" section to search results
                    if !current_category
                        .to_lowercase()
                        .contains("quote of the week")
                        && let Ok(url) = url::Url::parse(&current_url)
                    {
                        entries.push(EntryId {
                            title: current_title.clone(),
                            url,
                            category: current_category.clone(),
                            date,
                        });
                    }
                    current_title.clear();
                    current_url.clear();
                }
                Event::Text(text) => {
                    if current_category.is_empty() {
                        current_category = text.to_string();
                    } else if in_link {
                        if current_title.is_empty() {
                            current_title = text.to_string();
                        } else {
                            current_title.push_str(&text);
                        }
                    }
                }
                Event::Code(text) => {
                    if in_link {
                        current_title.push('`');
                        current_title.push_str(&text);
                        current_title.push('`');
                    }
                }
                _ => {}
            }
        }

        entries
    }
}

impl Default for TwirParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    fn setup() -> TwirParser {
        TwirParser::new()
    }

    #[test]
    fn test_skip_intro() {
        let parser = setup();
        let input = indoc! {"
            # What's cooking on master?
        
            89 pull requests were merged this week. This is the most pull requests merged
            in a week, ever. 10 1.0 issues were closed this week, and 0 opened.
            
            ## Breaking Changes
             
            - Unique vector patterns (matching on a `~[]`) [has been removed from the
            language](https://github.com/mozilla/rust/pull/12244). One can still match
            against a slice.
        "};

        let expected = indoc! {"
            ## Breaking Changes
             
            - Unique vector patterns (matching on a `~[]`) [has been removed from the
            language](https://github.com/mozilla/rust/pull/12244). One can still match
            against a slice.
        "};

        assert_eq!(parser.skip_intro_section(input), expected.trim_end());
    }

    #[test]
    fn test_skip_intro_header3() {
        let parser = setup();
        let input = indoc! {"
            ## What's cooking on master?
        
            89 pull requests were merged this week. This is the most pull requests merged
            in a week, ever. 10 1.0 issues were closed this week, and 0 opened.
            
            ### Breaking Changes
             
            - Unique vector patterns (matching on a `~[]`) [has been removed from the
            language](https://github.com/mozilla/rust/pull/12244). One can still match
            against a slice.
        "};

        let expected = indoc! {"
            ### Breaking Changes
             
            - Unique vector patterns (matching on a `~[]`) [has been removed from the
            language](https://github.com/mozilla/rust/pull/12244). One can still match
            against a slice.
        "};

        assert_eq!(parser.skip_intro_section(input), expected.trim_end());
    }

    #[test]
    fn test_skip_intro_not_found() {
        let parser = setup();
        let input = indoc! {"
            ## What's cooking on master?
        
            89 pull requests were merged this week. This is the most pull requests merged
            in a week, ever. 10 1.0 issues were closed this week, and 0 opened.
        "};

        assert_eq!(parser.skip_intro_section(input), input);
    }

    #[test]
    fn test_content_parse_category() {
        let parser = setup();
        let content = indoc! {"
                ### Official
                
                * [This Development-cycle in Cargo: 1.81](https://blog.rust-lang.org/inside-rust/2024/08/15/this-development-cycle-in-cargo-1.81.html)
                * [Async Closures MVP: Call for Testing!](https://blog.rust-lang.org/inside-rust/2024/08/09/async-closures-call-for-testing.html)
        "};

        let date = NaiveDate::from_ymd_opt(2024, 8, 21).unwrap();
        let entries = parser.content_to_entry_ids(content, date);

        assert_eq!(entries.len(), 2);
        let first = &entries[0];
        assert_eq!(first.category, "Official");
        assert_eq!(first.title, "This Development-cycle in Cargo: 1.81");
        assert_eq!(
            first.url,
            url::Url::parse("https://blog.rust-lang.org/inside-rust/2024/08/15/this-development-cycle-in-cargo-1.81.html").unwrap()
        );

        let second = &entries[1];
        assert_eq!(second.category, "Official");
        assert_eq!(second.title, "Async Closures MVP: Call for Testing!");
        assert_eq!(
            second.url,
            url::Url::parse("https://blog.rust-lang.org/inside-rust/2024/08/09/async-closures-call-for-testing.html").unwrap()
        );
    }

    #[test]
    fn test_code_in_content() {
        let parser = setup();
        let content = indoc! {"
                ### Official
                
                * [This `code` in Cargo](https://example.com)
        "};

        let date = NaiveDate::from_ymd_opt(2024, 8, 21).unwrap();
        let entries = parser.content_to_entry_ids(content, date);

        assert_eq!(entries.len(), 1);
        let first = &entries[0];
        assert_eq!(first.category, "Official");
        assert_eq!(first.title, "This `code` in Cargo");
        assert_eq!(first.url, url::Url::parse("https://example.com").unwrap());
    }

    #[test]
    fn test_parse_date() {
        let parser = setup();
        let meta = indoc! {"
            Number: 561
            Date: 2024-08-21
        "};

        let date = parser.parse_date_from_meta(meta);
        assert!(date.is_ok());
        assert_eq!(date.unwrap(), NaiveDate::from_ymd_opt(2024, 8, 21).unwrap());
    }

    #[test]
    fn test_parse_date_time() {
        let parser = setup();
        let meta = indoc! {"
            Number: 561
            Date: 2024-08-21 12:00
        "};

        let date = parser.parse_date_from_meta(meta);
        assert!(date.is_ok());
        assert_eq!(date.unwrap(), NaiveDate::from_ymd_opt(2024, 8, 21).unwrap());
    }

    #[test]
    fn test_extract_quotes() {
        let parser = setup();
        let content = indoc! {"
            # Quote of the Week
            
            > Memory errors are fundamentally state errors.
            
            — [desiringmachines on /r/rust](https://www.reddit.com/r/rust).
            
            Thanks to dikaiosune.
            
            # Next Section
        "};

        let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let quotes = parser.extract_quotes(content, date);

        assert_eq!(quotes.len(), 1);
        assert_eq!(
            quotes[0].text,
            "Memory errors are fundamentally state errors."
        );
        assert_eq!(quotes[0].author, "desiringmachines on /r/rust.");
        assert_eq!(
            quotes[0].url.as_ref().unwrap().as_str(),
            "https://www.reddit.com/r/rust"
        );
    }
}
