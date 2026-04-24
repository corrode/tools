use scraper::{Html, Selector};
fn main() {
    let html = std::fs::read_to_string("/tmp/oxidize.html").unwrap();
    let document = Html::parse_document(&html);
    let session_selector = Selector::parse("a.session").unwrap();
    let summary_selector = Selector::parse(".session_summary p").unwrap();
    let speaker_selector = Selector::parse(".session_speaker").unwrap();
    
    for session in document.select(&session_selector) {
        let href = session.value().attr("href").unwrap_or("");
        let summary = session.select(&summary_selector).next().map(|el| el.inner_html().trim().to_string()).unwrap_or_default();
        let speakers = session.select(&speaker_selector).map(|el| el.inner_html().trim().to_string()).collect::<Vec<_>>();
        println!("Href: {} | Summary: {} | Speakers: {:?}", href, summary, speakers);
    }
}
