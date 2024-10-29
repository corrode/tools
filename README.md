# TWIR

This project crawls and analyzes "This Week in Rust" posts, storing the data in a PostgreSQL database.

Show HN: I built a search engine for 'This Week in Rust'

## Getting Started

Follow these steps to set up and run the project on your local machine.

### Start Postgres

Either start a local Postgres instance or use Docker to run a container.

```sh
docker compose up db
```

In a separate terminal, run the application:

```sh
cargo run -- serve
```

Or use `cargo-watch` to automatically restart the application when files change:

```sh
cargo watch --exec 'run -- serve'                                  
```