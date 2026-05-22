A public, read-only JSON API over a curated index of Rust ecosystem content:
blog posts, RFCs, conference talks, recorded videos, podcast episodes, and
academic research papers.

## Conventions

- **Base URL** — All paths are relative to `/api/v1`. The version prefix is
  the API's stability contract: breaking changes will bump it.
- **Content type** — Requests are query-string only; responses are
  `application/json; charset=utf-8`.
- **Dates** — ISO-8601 (`YYYY-MM-DD`).
- **Durations** — Raw seconds as integers (no `"5m 30s"` strings).
- **URLs** — Plain strings, no envelope.
- **Pagination** — 1-based pages. The page size is fixed server-side and
  returned as `per_page` so clients never need to hard-code it.

## Query syntax

The `q` parameter on `/search` is parsed with a minimal, predictable grammar:

| Example                          | Meaning                                                    |
|----------------------------------|------------------------------------------------------------|
| `async runtime`                  | Both words must appear; contiguous matches rank higher.    |
| `"async await"`                  | Exact phrase match.                                        |
| `site:github.com tokio`          | Restrict to one host (only one `site:` filter per query).  |

Operators that are **not** supported: negation (`-foo`), boolean (`OR`),
parentheses. We may add them later under explicit feature flags.

## Filters

- `type` — restrict to a single content type
  (`articles`, `video`, `podcast`, `talks`, `research`).
- `start-year` / `end-year` — inclusive year bounds in `[1900, 2050]`.
- `sort-by` — `relevance` (default), `date-desc`, `date-asc`.

## Errors

Every non-2xx response is a JSON object of the form:

```json
{
  "code": "invalid_params",
  "message": "Human readable explanation"
}
```

Stable error codes: `invalid_params` (400), `not_found` (404),
`internal` (500). Clients should branch on `code`, not on `message`.

## CORS

All `/api/v1/*` endpoints respond with `Access-Control-Allow-Origin: *` and
allow `GET` from any origin. The API is read-only and unauthenticated.

## Examples

- Search for "async runtime":
  `GET /api/v1/search?q=async%20runtime`
- Limit to RustConf talks:
  `GET /api/v1/search?q=lifetimes&type=talks`
- Articles from 2024, newest first:
  `GET /api/v1/search?type=articles&start-year=2024&sort-by=date-desc`
- Get an episode's transcript:
  `GET /api/v1/podcasts/42`
- Autocomplete a prefix:
  `GET /api/v1/suggestions?q=asyn`
