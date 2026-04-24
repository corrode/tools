use scraper::{Html, Selector};
fn main() {
    let html = std::fs::read_to_string("/tmp/oxidize.html").unwrap();
    let document = Html::parse_document(&html);
    let session_selector = Selector::parse("a.session").unwrap();
    let summary_selector = Selector::parse(".session_summary p").unwrap();
    let person_selector = Selector::parse(".session_person").unwrap();
    let person_no_pic_selector = Selector::parse(".session_person-no-pic div:first-child").unwrap();

    for session in document.select(&session_selector) {
        let href = session.value().attr("href").unwrap_or("");
        let summary = session
            .select(&summary_selector)
            .next()
            .map(|el| el.inner_html().trim().to_string())
            .unwrap_or_default();

        let mut speakers = Vec::new();
        for el in session.select(&person_selector) {
            speakers.push(el.inner_html().trim().to_string());
        }
        for el in session.select(&person_no_pic_selector) {
            speakers.push(el.inner_html().trim().to_string());
        }
        println!(
            "Href: {} | Summary: {} | Speakers: {:?}",
            href, summary, speakers
        );
    }
}
