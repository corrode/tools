//! Markdown to plain text conversion.
//!
//! Strips all markdown formatting and returns clean plain text suitable for
//! full-text indexing. Uses `pulldown-cmark` (already a project dependency)
//! so no additional crates are needed.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Converts a markdown string to plain text by stripping all markup.
///
/// - Headings, paragraphs, and block elements are separated by newlines
/// - Ordered lists preserve numbers; unordered lists use bullet points
/// - Nested lists are indented with tabs
/// - Inline code and code blocks emit their content as-is (terms remain searchable)
/// - Strikethrough text is dropped entirely
/// - Links and images emit their alt/title text only
#[must_use]
pub fn to_plaintext(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(markdown, options);
    let mut tags_stack: Vec<Tag> = Vec::new();
    let mut buffer = String::new();

    for event in parser {
        match event {
            Event::Start(tag) => {
                start_tag(&tag, &mut buffer, &mut tags_stack);
                tags_stack.push(tag);
            }
            Event::End(tag) => {
                tags_stack.pop();
                end_tag(&tag, &mut buffer, &tags_stack);
            }
            Event::Text(content) => {
                if !tags_stack.iter().any(is_strikethrough) {
                    buffer.push_str(&content);
                }
            }
            Event::Code(content) => buffer.push_str(&content),
            Event::SoftBreak => buffer.push(' '),
            _ => {}
        }
    }

    buffer.trim().to_string()
}

fn start_tag(tag: &Tag, buffer: &mut String, tags_stack: &mut [Tag]) {
    match tag {
        Tag::Link { title, .. } | Tag::Image { title, .. } => buffer.push_str(title),
        Tag::Item => {
            buffer.push('\n');
            let mut lists_stack = tags_stack
                .iter_mut()
                .filter_map(|tag| match tag {
                    Tag::List(nb) => Some(nb),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let prefix_tabs_count = lists_stack.len() - 1;
            for _ in 0..prefix_tabs_count {
                buffer.push('\t');
            }
            if let Some(Some(nb)) = lists_stack.last_mut() {
                buffer.push_str(&nb.to_string());
                buffer.push_str(". ");
                *nb += 1;
            } else {
                buffer.push_str("• ");
            }
        }
        Tag::Paragraph | Tag::CodeBlock(_) | Tag::Heading { .. } => buffer.push('\n'),
        _ => {}
    }
}

fn end_tag(tag: &TagEnd, buffer: &mut String, tags_stack: &[Tag]) {
    match tag {
        TagEnd::Paragraph | TagEnd::Heading { .. } => buffer.push('\n'),
        TagEnd::CodeBlock => {
            if !buffer.ends_with('\n') {
                buffer.push('\n');
            }
        }
        TagEnd::List(_) => {
            let is_sublist = tags_stack.iter().any(|tag| matches!(tag, Tag::List { .. }));
            if !is_sublist {
                buffer.push('\n');
            }
        }
        _ => {}
    }
}

fn is_strikethrough(tag: &Tag) -> bool {
    matches!(tag, Tag::Strikethrough)
}

#[cfg(test)]
mod tests {
    use super::to_plaintext;

    #[test]
    fn basic_inline_strong() {
        assert_eq!(to_plaintext("**Hello**"), "Hello");
    }

    #[test]
    fn basic_inline_emphasis() {
        assert_eq!(to_plaintext("_Hello_"), "Hello");
    }

    #[test]
    fn basic_header() {
        let markdown = "# Header\n\n## Sub header\n\nEnd paragraph.";
        let expected = "Header\n\nSub header\n\nEnd paragraph.";
        assert_eq!(to_plaintext(markdown), expected);
    }

    #[test]
    fn alt_header() {
        let markdown = "\nHeader\n======\n\nEnd paragraph.";
        let expected = "Header\n\nEnd paragraph.";
        assert_eq!(to_plaintext(markdown), expected);
    }

    #[test]
    fn strong_emphasis() {
        assert_eq!(
            to_plaintext("**asterisks and _underscores_**"),
            "asterisks and underscores"
        );
    }

    #[test]
    fn strikethrough() {
        assert_eq!(
            to_plaintext("This was ~~erased~~ deleted."),
            "This was  deleted."
        );
    }

    #[test]
    fn mixed_list() {
        let markdown = "Start paragraph.\n\n1. First ordered list item\n2. Another item\n1. Actual numbers don't matter, just that it's a number\n  1. Ordered sub-list\n4. And another item.\n\nEnd paragraph.";
        let expected = "Start paragraph.\n\n1. First ordered list item\n2. Another item\n3. Actual numbers don't matter, just that it's a number\n4. Ordered sub-list\n5. And another item.\n\nEnd paragraph.";
        assert_eq!(to_plaintext(markdown), expected);
    }

    #[test]
    fn nested_lists() {
        let markdown = "\n* alpha\n* beta\n    * one\n    * two\n* gamma\n";
        let expected = "• alpha\n• beta\n\t• one\n\t• two\n• gamma";
        assert_eq!(to_plaintext(markdown), expected);
    }

    #[test]
    fn list_with_header() {
        let markdown = "# Title\n* alpha\n* beta\n";
        let expected = "Title\n\n• alpha\n• beta";
        assert_eq!(to_plaintext(markdown), expected);
    }

    #[test]
    fn basic_link() {
        assert_eq!(
            to_plaintext("I'm an [inline-style link](https://www.google.com)."),
            "I'm an inline-style link."
        );
    }

    #[test]
    fn basic_image() {
        assert_eq!(
            to_plaintext("As displayed in ![img alt text](https://example.com/img.png)."),
            "As displayed in img alt text."
        );
    }

    #[test]
    fn inline_code() {
        assert_eq!(
            to_plaintext("This is `inline code`."),
            "This is inline code."
        );
    }

    #[test]
    fn code_block() {
        let markdown = "Start paragraph.\n```javascript\nvar s = \"JavaScript syntax highlighting\";\nalert(s);\n```\nEnd paragraph.";
        let expected = "Start paragraph.\n\nvar s = \"JavaScript syntax highlighting\";\nalert(s);\n\nEnd paragraph.";
        assert_eq!(to_plaintext(markdown), expected);
    }

    #[test]
    fn block_quote() {
        let markdown = "Start paragraph.\n\n> Blockquotes are very handy in email to emulate reply text.\n> This line is part of the same quote.\n\nEnd paragraph.";
        let expected = "Start paragraph.\n\nBlockquotes are very handy in email to emulate reply text. This line is part of the same quote.\n\nEnd paragraph.";
        assert_eq!(to_plaintext(markdown), expected);
    }

    #[test]
    fn paragraphs() {
        let markdown = "Paragraph 1.\n\nParagraph 2.";
        let expected = "Paragraph 1.\n\nParagraph 2.";
        assert_eq!(to_plaintext(markdown), expected);
    }
}
