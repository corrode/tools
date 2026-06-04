# Rust Tool Index

A dense, curated reference of Rust development tooling, served at
[tools.corrode.dev](https://tools.corrode.dev). For every tool it shows the
editorial "what it's for / when not to use it" notes plus live, machine-refreshed
metrics — downloads, stars, last activity, license, maintainers — so you can tell
at a glance whether a tool is relevant and maintained. Archived tools are kept but
clearly marked **deprecated** and linked to their successors.

## How it works

The catalog is just **TOML files in `data/`** — there is no database. The server
loads them into memory at startup and renders one dense page, a JSON API, and an
LLM feed.

Each tool file mixes two ownership layers:

- **Human-owned** editorial fields (`name`, `repository`, `category`, `remarks`,
  `alternatives`, `successors`). Humans only ever edit these.
- A **bot-owned** `[metrics]` table refreshed daily by the `generator` from the
  source forge (GitHub/GitLab/Codeberg) and crates.io. A GitHub Action opens a
  single rolling PR with the changes for human review — it never touches the
  prose.

```text
data/
  categories.toml        # controlled category vocabulary (validated in CI)
  tools/<id>.toml        # one file per tool
crates/
  types/                 # data model + loading + validation (the Catalog)
  server/                # axum + askama site, /api/v1 JSON API, /llms.txt
  generator/             # metric refresher (forge + crates.io -> [metrics])
scripts/                 # throwaway crates.io discovery helpers
```

## Running locally

```sh
cargo run -p server          # serves http://localhost:3000
```

Refresh metrics into the TOML files (writes the `[metrics]` tables in place):

```sh
cargo run -p generator -- --data-dir data
# or a single tool:
cargo run -p generator -- --only cargo-nextest
```

The server loads data once at startup; restart it to pick up changes.

## Adding a tool

1. Create `data/tools/<id>.toml` with the human-owned fields:

   ```toml
   id = "cargo-nextest"
   name = "cargo-nextest"
   repository = "https://github.com/nextest-rs/nextest"
   category = "testing"              # must exist in data/categories.toml
   crate = "cargo-nextest"           # optional: crates.io crate name

   remarks = """
   What it does, when to reach for it, and — crucially — when *not* to.
   """

   alternatives = ["cargo test (built-in)"]
   # For a deprecated tool, point at its replacements instead:
   # successors = ["bacon", "watchexec"]
   ```

2. Run the generator to populate `[metrics]`.
3. `cargo test` validates that every `category` exists and ids are unique.

The `scripts/` helpers (`discover.sh`, `crate_info.sh`) speed up finding
candidates and their crate facts.

## API & LLM access

All read-only and public:

- `GET /api/v1/tools` — the full machine-readable catalog (JSON).
- `GET /api/v1/tools/{id}` — a single tool.
- `GET /api/v1/openapi.json` and `GET /api/v1/docs` — OpenAPI spec + Swagger UI.
- `GET /llms.txt` — a flat, token-efficient plaintext rendering of the whole
  index for pasting into an LLM context.

## Deployment

`docker.yml` builds a minimal static image (just the `server` binary plus the
baked-in `data/` and `static/`), pushes it to ghcr.io, and triggers a Coolify
redeploy. Because the data is baked in at build time, the running container makes
no external API calls. Merging a metrics PR rebuilds and redeploys automatically.

> The Coolify deploy `uuid` in `docker.yml` is a placeholder until the app is
> created.
