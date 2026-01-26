# Rust Search

This project crawls and analyzes "This Week in Rust" posts, storing the data in an sqlite database.

## Starting the server

```sh
cargo run
```

## Indexing content

To index 'This Week in Rust' posts, run the crawler:

```sh
cargo run --bin crawler
```

this will crawl new articles and create/update the `data/index.db` sqlite file.

After that, you can run your server with

Or use `cargo-watch` to automatically restart the application when files change:

```sh
cargo watch -x 'run'
```
