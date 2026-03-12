use scraper::{Html, Selector};

fn main() {
    let html = std::fs::read_to_string("/tmp/rustfest_schedule.html").unwrap();
    let document = Html::parse_document(&html);

    // Rows in table
    let row_selector = Selector::parse("table.time-schedule tbody tr").unwrap();
    let td_selector = Selector::parse("td").unwrap();
    let a_selector = Selector::parse("a").unwrap();

    for row in document.select(&row_selector) {
        let tds: Vec<_> = row.select(&td_selector).collect();
        if tds.len() >= 2 {
            let _time = tds[0].inner_html();
            let content_td = tds[1];

            // Check if it's a joint event (break, lunch, etc)
            if content_td.value().attr("class") == Some("joint-event") {
                continue;
            }

            // It's a talk. The content usually has: Text <a href="...">Speaker</a>
            let full_text = content_td.text().collect::<Vec<_>>().join("");
            let mut speaker_names = Vec::new();

            for a in content_td.select(&a_selector) {
                let speaker = a.text().collect::<Vec<_>>().join("").trim().to_string();
                if !speaker.is_empty() {
                    speaker_names.push(speaker);
                }
            }

            let title = if !speaker_names.is_empty() {
                // Remove the speaker names from the full text to get the title
                let mut t = full_text.clone();
                for s in &speaker_names {
                    t = t.replace(s, "");
                }
                t.trim().to_string()
            } else {
                full_text.trim().to_string()
            };

            println!("Title: {} | Speakers: {:?}", title, speaker_names);
        }
    }
}
