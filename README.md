# Rust Search

An experimental search engine for Rust-related content.

This project crawls sources like "This Week in Rust" posts, storing the data in an sqlite database.

## Starting the server

```sh
cargo run
```

## Starting the crawler

### Configuration

To crawl YouTube videos, you need a YouTube Data API v3 key.

1. Create a `.env` file in the root directory.
2. Add your API key:

```env
YOUTUBE_API_KEY=your_api_key_here
```

### Indexing Content

Finally, start the crawler:

```sh
RUST_LOG=crawler=debug cargo run -p crawler
```

This will crawl new articles and create/update the `data/index.db` sqlite file.

Alternatively, you can use `cargo-watch` to automatically restart the
application when files change:

```sh
cargo watch -x 'run'
```
