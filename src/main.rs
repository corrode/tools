use std::fs;
use std::io::Write;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use reqwest;
use serde::Deserialize;
use serde::Serialize;
use tokio;

use chrono::NaiveDate;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag};
use url::Url;

/// Entry from TWiR markdown file
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EntryId {
    title: String,
    url: Url,
    category: String,
    // date: NaiveDate,
}

use pulldown_cmark::{Options, TagEnd};

fn md_extensions() -> Options {
    Options::all()
}

fn content_to_entry_ids(content: &str) -> Vec<EntryId> {
    let mut entries = Vec::new();

    let mut current_category = String::new();
    let mut current_title = String::new();
    let mut current_url = String::new();
    let mut in_link = false;

    let parser = Parser::new_ext(content, md_extensions());

    for event in parser {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H3,
                ..
            }) => {
                current_category = String::new();
            }
            Event::End(TagEnd::Heading(_)) => {}

            Event::Start(Tag::Link { dest_url, .. }) => {
                in_link = true;
                current_url = dest_url.to_string();
            }
            Event::End(TagEnd::Link) => {
                in_link = false;
                if let Ok(url) = Url::parse(&current_url) {
                    entries.push(EntryId {
                        title: current_title.clone(),
                        url,
                        category: current_category.clone(),
                    });
                } else {
                    println!("Failed to parse URL: {}", current_url);
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
                    // push with backticks
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

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    id: EntryId,
    date: NaiveDate,
}

fn parse_file(content: &str) -> Vec<Entry> {
    // TODO: Do something with meta
    let (meta, body) = content.split_once("\n\n").unwrap();

    let date = parse_date_from(meta).unwrap();
    let body = skip_intro(&body);

    let ids = content_to_entry_ids(&body);

    ids.into_iter().map(|id| Entry { id, date }).collect()
}

/// Parses the date from this format:
/// Number: 561
/// Date: 2024-08-21
fn parse_date_from(meta: &str) -> Result<NaiveDate> {
    for line in meta.lines() {
        if line.starts_with("Date") {
            let date_str = line.split_once(":").unwrap().1.trim();

            let date_formats = ["%Y-%m-%d", "%Y-%m-%d %H:%M"];

            for format in date_formats.iter() {
                if let Ok(date) = NaiveDate::parse_from_str(date_str, format) {
                    return Ok(date);
                }
            }
            bail!("Found date but failed to parse it: {}", date_str);
        }
    }

    bail!("Did not find post date")
}

// TODO: Store the parsed entries in a database
const JSON_OUPUT_FILE: &str = "twir.jsonl";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let owner = "rust-lang";
    let repo = "this-week-in-rust";
    let path = "content";
    let branch = "master";

    let url = format!(
        "https://api.github.com/repos/{}/{}/contents/{}?ref={}",
        owner, repo, path, branch
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", "Rust GitHub API Client")
        .send()
        .await?
        .json::<Vec<serde_json::Value>>()
        .await?;

    let output_file = fs::File::create(JSON_OUPUT_FILE)?;
    let mut writer = std::io::BufWriter::new(output_file);

    for item in resp {
        let file_name = item["name"].as_str().unwrap();
        let download_url = item["download_url"].as_str().unwrap();

        let content = client
            .get(download_url)
            .header("User-Agent", "Rust GitHub API Client")
            .send()
            .await?
            .text()
            .await?;

        fs::create_dir_all(path)?;
        fs::write(format!("{}/{}", path, file_name), &content)?;
        println!("Downloaded: {}", file_name);

        println!("Parsing: {}", file_name);
        // println!("Raw content: {}", content);

        let entries = parse_file(&content);

        // for entry in entries {
        //     println!("{:#?}", entry);
        // }

        // Store the parsed entries in a database
        for entry in entries {
            let json = serde_json::to_string(&entry)?;
            writer.write_all(json.as_bytes())?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
    }

    println!("Successfully downloaded all files from the specified path.");
    Ok(())
}

/// Skips the intro of the TWiR markdown file
/// by removing everything before the first `###` heading.
fn skip_intro(body: &str) -> String {
    body.split_once("###")
        .map(|(_, content)| format!("###{}", content))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use std::fs;

    use super::*;

    /// Skips the intro of the TWiR markdown file
    /// by removing everything before the first `###` heading.
    fn skip_intro(body: &str) -> String {
        body.split_once("###")
            .map(|(_, content)| format!("###{}", content))
            .unwrap_or_default()
    }

    #[test]
    fn test_normal_case() {
        let input = "Some intro\nMore intro\n### First heading\nContent".to_string();
        let expected = "### First heading\nContent".to_string();
        assert_eq!(skip_intro(&input), expected);
    }

    #[test]
    fn test_no_heading() {
        let input = "Some content without headings".to_string();
        assert_eq!(skip_intro(&input), "");
    }

    #[test]
    fn test_only_heading() {
        let input = "### Heading".to_string();
        assert_eq!(skip_intro(&input), "### Heading");
    }

    #[test]
    fn test_multiple_headings() {
        let input = "Intro\n### First\n### Second".to_string();
        let expected = "### First\n### Second".to_string();
        assert_eq!(skip_intro(&input), expected);
    }

    #[test]
    fn test_heading_at_start() {
        let input = "### No intro\nJust content".to_string();
        assert_eq!(skip_intro(&input), input);
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(skip_intro(""), "");
    }

    #[test]
    fn test_content_parse_category() {
        let content = indoc! {"
                ### Official
                
                * [This Development-cycle in Cargo: 1.81](https://blog.rust-lang.org/inside-rust/2024/08/15/this-development-cycle-in-cargo-1.81.html)
                * [Async Closures MVP: Call for Testing!](https://blog.rust-lang.org/inside-rust/2024/08/09/async-closures-call-for-testing.html)
        "};

        let entries = content_to_entry_ids(&content);
        println!("{entries:#?}");
        assert_eq!(entries.len(), 2);

        let first = &entries[0];
        assert_eq!(first.category, "Official");
        assert_eq!(first.title, "This Development-cycle in Cargo: 1.81");
        assert_eq!(first.url, Url::parse("https://blog.rust-lang.org/inside-rust/2024/08/15/this-development-cycle-in-cargo-1.81.html").unwrap());

        let second = &entries[1];
        assert_eq!(second.category, "Official");
        assert_eq!(second.title, "Async Closures MVP: Call for Testing!");
        assert_eq!(second.url, Url::parse("https://blog.rust-lang.org/inside-rust/2024/08/09/async-closures-call-for-testing.html").unwrap());
    }

    #[test]
    fn test_code_in_content() {
        let content = indoc! {"
                ### Official
                
                * [This `code` in Cargo](https://example.com)
        "};

        let entries = content_to_entry_ids(&content);
        println!("{entries:#?}");
        assert_eq!(entries.len(), 1);

        let first = &entries[0];
        assert_eq!(first.category, "Official");
        assert_eq!(first.title, "This `code` in Cargo");
        assert_eq!(first.url, Url::parse("https://example.com").unwrap());
    }

    #[test]
    fn test_parse_date() {
        let meta = indoc! {"
            Number: 561
            Date: 2024-08-21
        "};

        let date = parse_date_from(meta);
        assert!(date.is_ok());
        assert_eq!(date.unwrap(), NaiveDate::from_ymd(2024, 8, 21));
    }

    // Old TWiR format
    #[test]
    fn test_parse_date_time() {
        let meta = indoc! {"
            Number: 561
            Date: 2024-08-21 12:00
        "};

        let date = parse_date_from(meta);
        assert!(date.is_ok());
        assert_eq!(date.unwrap(), NaiveDate::from_ymd(2024, 8, 21));
    }

    #[test]
    fn test_content_to_entry_ids() {
        let content = fs::read_to_string("fixtures/test.md");
        assert!(content.is_ok());

        let entries = parse_file(content.unwrap().as_str());
        println!("{entries:#?}");
        // assert_eq!(entries.len(), 2);
    }
}
