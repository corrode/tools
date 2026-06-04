# Rust Tool Index

A curated guide to Rust development tooling, live at
[tools.corrode.dev](https://tools.corrode.dev).

For each tool you get an honest, hand-written take — what it's good at and when
*not* to reach for it — next to fresh numbers: downloads, stars, last activity,
license, maintainers. Deprecated tools stay listed, clearly marked, and point you
to what to use instead.

## How it works

There's no database. The catalog is just TOML files in `data/`, loaded into memory
when the server starts. Every tool file has two halves:

- **The prose** (`name`, `repository`, `category`, `remarks`, `alternatives`,
  `successors`) — written and owned by humans.
- **The `[metrics]`** — refreshed daily by the `generator`, which pulls from the
  source forge and crates.io and opens a single rolling PR for review. It never
  touches your words.

```text
data/
  categories.toml     # the allowed categories (CI checks every tool against it)
  tools/<id>.toml     # one file per tool
crates/
  types/              # the data model: loading + validation
  server/             # the site, JSON API, and /llms.txt (axum + askama)
  generator/          # the daily metrics refresher
scripts/              # helpers for finding new tools
```

## Running it

```sh
cargo run -p server          # http://localhost:3000
```

Data is read once at startup, so restart to pick up changes.

To refresh metrics in place:

```sh
cargo run -p generator -- --data-dir data        # everything
cargo run -p generator -- --only cargo-nextest   # just one
```

## Adding a tool

Drop a file in `data/tools/<id>.toml`:

```toml
id = "cargo-nextest"
name = "cargo-nextest"
repository = "https://github.com/nextest-rs/nextest"
category = "testing"          # must exist in data/categories.toml
crate = "cargo-nextest"       # optional, for crates.io metrics

remarks = """
What it does, when to reach for it, and — just as important — when not to.
"""

alternatives = ["cargo test (built-in)"]
related = ["cargo-llvm-cov"]   # complementary tools, not replacements
recommended = true              # editor's pick: badge + floats to top, filterable
added = "2025-01-15"            # optional; shows a "New" badge for 30 days, enables "recently added" sort
# Deprecated instead? Point at the replacements:
# successors = ["bacon", "watchexec"]
```

Then run the generator to fill in `[metrics]`, and `cargo test` to check the
category exists and the id is unique. The `scripts/` helpers can speed up finding
candidates.

## API

Everything is public and read-only:

- `GET /api/v1/tools` — the whole catalog as JSON (`?category=<id>` to filter)
- `GET /api/v1/tools/{id}` — a single tool
- `GET /api/v1/docs` — Swagger UI (`/api/v1/openapi.json` for the raw spec)
- `GET /llms.txt` — the index as plain text, ready to paste into an LLM

## Deploying

`docker.yml` bakes the `server` binary together with `data/` and `static/` into a
small image, pushes it to ghcr.io, and triggers a Coolify redeploy. Since the data
ships inside the image, the running container makes no external calls — and merging
a metrics PR redeploys automatically.
