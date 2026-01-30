# Rust Search

An experimental search engine for Rust-related content.

This project crawls sources like "This Week in Rust" posts, storing the data in an sqlite database.

## Configuration

To crawl YouTube videos, you need a YouTube Data API v3 key.

1. Create a `.env` file in the root directory.
2. Add your API key:

```env
YOUTUBE_API_KEY=your_api_key_here
```

## Starting the server

```sh
cargo run
```

## Indexing content

To index content, run the crawler:

```sh
RUST_LOG=crawler=debug cargo run -p crawler
```

this will crawl new articles and create/update the `data/index.db` sqlite file.

Or use `cargo-watch` to automatically restart the application when files change:

```sh
cargo watch -x 'run'
```
