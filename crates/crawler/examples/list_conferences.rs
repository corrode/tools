// Example to list all registered conference parsers
//
// Run with: cargo run --example list_conferences

use crawler::indexer::conference::get_all_parsers;

fn main() {
    let parsers = get_all_parsers();

    println!("Total conferences registered: {}\n", parsers.len());
    println!("Registered conferences:");

    for parser in parsers {
        let metadata = parser.metadata();
        let playlist = if metadata.youtube_playlist_url.is_some() {
            "with playlist"
        } else {
            "no playlist"
        };
        println!(
            "  - {} {} (id: {}, {})",
            metadata.conference, metadata.year, metadata.id, playlist
        );
    }

    // Check for newly added conferences
    let all_ids: Vec<String> = get_all_parsers()
        .iter()
        .map(|p| p.metadata().id.to_string())
        .collect();

    println!("\nNewly added conferences:");
    let new_conferences = vec![
        "oxidize-2024",
        "rustfest-2024",
        "rustnation-2024",
        "fosdem-2024",
        "rustlab-2024",
    ];

    for id in new_conferences {
        if all_ids.contains(&id.to_string()) {
            println!("  ✓ {} is registered", id);
        } else {
            println!("  ✗ {} is NOT registered", id);
        }
    }
}
