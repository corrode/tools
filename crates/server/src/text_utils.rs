use pulldown_cmark::{Event, Parser, Tag, TagEnd};

pub fn clean_preview(content: &str) -> String {
    let parser = Parser::new(content);
    let mut preview = String::new();
    let mut in_brackets = false;
    let mut link_text = String::new();

    for event in parser {
        match event {
            // References like `[foo][someref]` aren't parsed as links in pulldown_cmark,
            // so we have to handle them manually.
            Event::Text(text) => {
                let text = text.trim();
                if text == "[" {
                    in_brackets = true;
                    link_text.clear();
                } else if text == "]" {
                    in_brackets = false;
                    // Exclude the `42` in references like `[foo][42]`
                    if !link_text.chars().all(|c| c.is_ascii_digit()) {
                        preview.push_str(&link_text);
                        preview.push(' ');
                    }
                } else if in_brackets {
                    link_text.push_str(text);
                } else if !text.starts_with("```") {
                    preview.push_str(text);
                    preview.push(' ');
                }
            }
            Event::Start(Tag::Link { .. }) => {
                in_brackets = true;
                link_text.clear();
            }
            Event::End(TagEnd::Link) => {
                in_brackets = false;
                preview.push_str(&link_text);
                preview.push(' ');
                link_text.clear();
            }
            Event::Code(code) => {
                preview.push('`');
                preview.push_str(&code);
                preview.push('`');
                preview.push(' ');
            }
            Event::SoftBreak | Event::HardBreak => {
                preview.push(' ');
            }
            _ => {}
        }
    }

    // Clean up multiple spaces and trim
    preview.split_whitespace().collect::<Vec<_>>().join(" ")
}
