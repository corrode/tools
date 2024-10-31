# Rust Search

This project crawls and analyzes "This Week in Rust" posts, storing the data in an sqlite database.

## Getting Started

First, index TWIR with

```sh
cargo run -- index
```

this will create/update the twir.db sqlite file.

After that, you can run the server with

```sh
cargo run -- serve
```

Or use `cargo-watch` to automatically restart the application when files change:

```sh
cargo watch --exec 'run -- serve'                                  
```
