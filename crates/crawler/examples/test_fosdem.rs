use scraper::{Html, Selector};

fn main() {
    let html = std::fs::read_to_string("/tmp/fosdem.html").unwrap();
    let document = Html::parse_document(&html);
    
    let row_selector = Selector::parse("table.table-striped tbody tr").unwrap();
    let td_selector = Selector::parse("td").unwrap();
    let a_selector = Selector::parse("a").unwrap();
    
    for row in document.select(&row_selector) {
        let tds: Vec<_> = row.select(&td_selector).collect();
        if tds.len() >= 4 {
            let title_td = tds[1];
            let speakers_td = tds[2];
            
            let title = title_td.text().collect::<Vec<_>>().join("").trim().to_string();
            if title.is_empty() { continue; }
            
            let href = title_td.select(&a_selector).next().map(|a| a.value().attr("href").unwrap_or("")).unwrap_or("");
            let speakers = speakers_td.text().collect::<Vec<_>>().join("").trim().to_string();
            
            println!("Title: {} | Href: {} | Speakers: {}", title, href, speakers);
        }
    }
}
