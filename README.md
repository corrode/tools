# Rust Search

An experimental search engine for Rust-related content.

This project crawls sources like "This Week in Rust" posts, storing the data in an sqlite database.

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
