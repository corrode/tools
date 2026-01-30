#[path = "../src/youtube.rs"]
mod youtube;

use std::env;
use youtube::YouTube;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    // Set up logging
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    // Ensure API key is set for the test
    if env::var("YOUTUBE_API_KEY").is_err() {
        eprintln!("Error: YOUTUBE_API_KEY environment variable is not set.");
        eprintln!("Please export YOUTUBE_API_KEY=your_api_key_here");
        std::process::exit(1);
    }

    // Allow passing a URL as an argument, default to the one from the prompt
    let args: Vec<String> = env::args().collect();
    let default_url = "https://www.youtube.com/watch?v=aZ5sfhGmEVU";
    let url = args.get(1).map(|s| s.as_str()).unwrap_or(default_url);

    println!("Crawling YouTube URL: {}", url);

    match YouTube::new(url).await {
        Ok(video) => {
            println!("----------------------------------------");
            println!("Successfully fetched video details (API V3)!");
            println!("----------------------------------------");
            println!("Title:       {}", video.title);
            println!("Thumbnail:   {}", video.thumbnails.maxres);
            println!("----------------------------------------");
            println!("Description:\n{}", video.description);
            println!("----------------------------------------");
        }
        Err(e) => {
            eprintln!("Error crawling video: {:?}", e);
        }
    }

    Ok(())
}
