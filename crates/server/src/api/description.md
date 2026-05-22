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
- `per_page` — page size override (1–100). Defaults to 20.
- `in` — comma-separated list of `doc_id`s (e.g. `article:1,podcast:42`) to
  restrict results to a specific set of documents. Up to 200 ids per request.
  Useful for follow-up queries: shortlist documents with `/search`, then
  drill in with `/search?q=...&in=...`.
- `snippets` — when set to `N` (1–5), each result includes up to `N`
  additional ranked passages in a `passages` array, with character
  offsets into the document body. Returned as plain text — no markup,
  so the response stays compact for LLM consumers. The default FTS
  `snippet` field is still returned (also plain text).

## Documents (full content)

Every result carries a stable `doc_id` of the form `"{kind}:{id}"`, e.g.
`"article:42"`. Pass this id to the `/documents/*` endpoints to fetch or
drill into the full body — ideal for LLM "deep research" workflows that
need to ground their reasoning in the source text without re-fetching it
from the open web.

- `GET /api/v1/documents/{doc_id}` — full document with `content.text`,
  `char_count`, and a heuristic `token_estimate`.
- `POST /api/v1/documents:batch` — fetch up to 25 documents in one round
  trip. Request body: `{"ids": ["article:1", "podcast:42"]}`. Response
  splits found docs from `missing` ids.
- `GET /api/v1/documents/{doc_id}/search?q=...&max=N` — ranked text
  passages from inside a single document. Returns char-offset-stable
  excerpts an LLM can quote verbatim or link back into
  `/documents/{doc_id}`.



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
- Wider candidate set for an LLM agent:
  `GET /api/v1/search?q=tokio&per_page=50&snippets=3`
- Restrict a follow-up search to a shortlist of documents:
  `GET /api/v1/search?q=lifetime&in=article:1,article:7,talk:42`
- Fetch one document by its stable id:
  `GET /api/v1/documents/article:42`
- Batch-fetch ten documents in one round trip:
  `POST /api/v1/documents:batch` with body
  `{"ids":["article:1","article:7","podcast:42"]}`
- Find the passages inside a podcast that mention a topic:
  `GET /api/v1/documents/podcast:42/search?q=lifetime%20elision&max=5`
- Get an episode's transcript (legacy shortcut):
  `GET /api/v1/podcasts/42`
- Autocomplete a prefix:
  `GET /api/v1/suggestions?q=asyn`
