# Public Search API — Design & Implementation Plan

Status: **in progress** (branch `feature/public-api-utoipa`).

This document captures the design of the public JSON API that sits alongside
the existing HTML/HTMX UI, and the OpenAPI documentation that describes it.

## Goals

1. Expose the existing search index as a clean, versioned, public JSON API.
2. Auto-generate an OpenAPI 3.1 schema from the Rust source of truth using
   [`utoipa`](https://docs.rs/utoipa).
3. Host the schema and a rendered docs UI at stable, public URLs.
4. Share parsing, validation, and storage code with the HTML routes — don't
   duplicate query parsing or DB access.
5. Keep the HTML routes byte-for-byte unchanged.

Explicitly **out of scope** for this iteration:

- Rate limiting (will be added later; CORS is open for now).
- Authentication / API keys.
- Write endpoints.
- Bulk export / streaming.

## Surface

All API routes are mounted under `/api/v1`. The version prefix is our escape
hatch for breaking changes.

| Method | Path                              | Description                                     |
|--------|-----------------------------------|-------------------------------------------------|
| GET    | `/api/v1/health`                  | Liveness probe                                  |
| GET    | `/api/v1/search`                  | Full-text search with filters & pagination      |
| GET    | `/api/v1/suggestions`             | Query autocomplete                              |
| GET    | `/api/v1/stats`                   | Aggregate index statistics                      |
| GET    | `/api/v1/podcasts/{id}`           | Podcast episode detail (incl. transcript)       |
| GET    | `/api/v1/openapi.json`            | The OpenAPI 3.1 spec                            |
| GET    | `/api/v1/docs`                    | Swagger UI rendered docs page                   |

## Architecture

```
crates/server/src/
├── main.rs              # router composition
├── error.rs             # AppError (HTML-only)
├── handlers/            # existing HTML/HTMX handlers — unchanged
│   ├── index.rs
│   ├── search.rs
│   ├── podcast.rs
│   ├── stats.rs
│   ├── suggestions.rs
│   └── mod.rs
└── api/                 # NEW: public JSON API
    ├── mod.rs           # ApiDoc, router builder, info description
    ├── error.rs         # ApiError → JSON IntoResponse
    ├── dto.rs           # response DTOs (ToSchema)
    ├── search.rs
    ├── suggestions.rs
    ├── stats.rs
    ├── podcast.rs
    └── health.rs
```

### Code sharing strategy

- **Query parsing** is in `types::params::RawParams`. Both HTML and API routes
  use `axum::extract::Query<RawParams>` and `RawParams::normalize_or_fallback`.
- **Storage access** goes through `storage::Repository`. Same methods, no
  duplication.
- **Response shapes diverge by design.** The HTML handlers convert
  `SearchResult` → view types (`Article`, `Video`, `Podcast`, …) tailored for
  Askama templates: they carry UI-only fields like `highlighted_title` with
  `<mark>` tags and human strings like `"~5 min read"`. Per-result favicons
  are fetched at render time from DuckDuckGo (`icons.duckduckgo.com/ip3/...`).
  The API returns clean DTOs in `api::dto` with raw,
  client-friendly types (e.g. `duration_seconds: u32`, ISO dates, no SVG).
  This keeps the API contract stable independent of UI churn.

### Discriminated union for results

`SearchResponse.results` is a tagged union so clients can branch on a single
discriminator field instead of looking for type-specific keys:

```json
{
  "results": [
    { "kind": "article", "title": "...", "url": "...", "reading_minutes": 5 },
    { "kind": "video",   "title": "...", "url": "...", "duration_seconds": 730 },
    { "kind": "podcast", "id": 42, "podcast_name": "...", "episode_name": "..." }
  ]
}
```

In Rust this is `#[serde(tag = "kind", rename_all = "snake_case")]` on the
`SearchHit` enum, which utoipa renders as a proper OpenAPI `oneOf` with a
`discriminator`.

### Errors

`ApiError` is a single response type for all non-2xx responses:

```json
{
  "code": "invalid_params",
  "message": "start year 1850 is out of range 1900–2050",
  "details": null
}
```

Variants:

| Code              | HTTP | Cause                                                  |
|-------------------|------|--------------------------------------------------------|
| `invalid_params`  | 400  | `ParamsError` from `types::params`                     |
| `not_found`       | 404  | e.g. unknown podcast id                                |
| `internal`        | 500  | anything else (logged, message scrubbed)               |

`ApiError` implements `From<anyhow::Error>` so `?` keeps working, and
`From<ParamsError>` so the 400 path is automatic.

## OpenAPI

### Hosting

- The raw spec is served at `/api/v1/openapi.json` directly from the in-memory
  `OpenApi` struct (no file on disk → no drift).
- A rendered docs UI is served at `/api/v1/docs` via
  [`utoipa-swagger-ui`](https://docs.rs/utoipa-swagger-ui).
- The root of the docs UI is also linked from the homepage footer.

### Description content

The `info.description` field uses Markdown and covers:

- What the index contains (articles, videos, podcasts, talks, research).
- Query syntax (phrases, `site:` filter, phrase-first ranking).
- Filtering (`type`, `start-year`/`end-year`, `sort-by`).
- Pagination conventions (1-based pages, `per_page` fixed at the server).
- Error format.
- Versioning policy.
- A short list of example URLs.

This same text is rendered by Swagger UI on the docs landing page.

### Schema derivation

- `types` crate gains an optional `openapi` feature that enables
  `utoipa::ToSchema` / `utoipa::IntoParams` derives behind `cfg_attr`. This
  keeps the schema definitions colocated with the types without forcing other
  consumers (the crawler) to depend on utoipa.
- `RawParams`, `SortOrder`, `ContentType`, `Quote`, `Stats` (and the
  sub-`*Stats` types it transitively contains) all derive `ToSchema`.
- API-only DTOs live in `crates/server/src/api/dto.rs` and derive `ToSchema`
  there.

### `utoipa-axum` integration

We use `utoipa_axum::router::OpenApiRouter` so that handlers registered with
`routes!(...)` automatically contribute both the route and the OpenAPI path
item. `split_for_parts()` yields a regular `axum::Router` and an `OpenApi`,
which we then mount and serve respectively.

## CORS

The `/api/v1` subtree gets a permissive CORS layer:

```rust
CorsLayer::new()
    .allow_origin(Any)
    .allow_methods([Method::GET])
    .allow_headers(Any)
```

This is appropriate for a public, read-only search API. HTML routes are
unaffected.

## Cleanup performed alongside the API work

- `server::error::AppError` keeps its HTML-only semantics; new `api::error::
  ApiError` lives next to the JSON handlers.
- `Cargo.toml` workspace deps are consolidated (no duplicate version strings
  across crate manifests).
- The `types` crate manifest gets a trailing newline.

## Validation

- `cargo build -p server` succeeds.
- `cargo test -p server` and `cargo test -p types` pass.
- Manual smoke test: `curl /api/v1/search?q=async%20await | jq`.

## Future work

- Rate limiting via `tower_governor` keyed by IP.
- `Cache-Control` headers for `/api/v1/search` and `/api/v1/stats`.
- API key auth via `utoipa::Modify` (`securitySchemes`) when quotas are
  needed.
- A small `examples/` section in the OpenAPI doc covering common queries.
