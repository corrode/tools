# Rust Search

This project crawls and analyzes "This Week in Rust" posts, storing the data in an sqlite database.

## Getting Started

First, index 'This Week in Rust' posts by running the crawler:

```sh
cargo run --bin crawler
```

this will crawl the articles in 'This Week in Rust' and
create/update the `twir.db` sqlite file in the project root.

After that, you can run your server with

```sh
cargo run
```

Or use `cargo-watch` to automatically restart the application when files change:

```sh
cargo watch -x 'run'
```
