# Rust Tool Index

A curated guide to Rust development tooling, live at
[tools.corrode.dev](https://tools.corrode.dev).

For each tool you get an honest, hand-written take (what it's good at and when
*not* to reach for it) next to fresh numbers: downloads, stars, last activity,
license, maintainers. Deprecated tools stay listed, clearly marked, and point you
to what to use instead.

## How it works

There's no database. The catalog is just TOML files in `data/`, loaded into memory
when the server starts. Every tool file has two halves:

- **The prose** (`name`, `repository`, `category`, `remarks`, `alternatives`,
  `successors`): written and owned by humans.
- **The `[metrics]`**: refreshed daily by the `generator`, which pulls from the
  source forge and crates.io and opens a single rolling PR for review. It never
  touches your words.

```text
data/
  categories.toml     # the allowed categories (CI checks every tool against it)
  tools/<id>.toml     # one file per tool
  stacks/<id>.toml    # optional: curated, cross-cutting toolboxes
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
# installable = false         # for library crates (criterion, insta, …): keeps
#                             # them out of derived stack install lines (default: true)

remarks = """
What it does, when to reach for it, and (just as important) when not to.
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

## Adding a stack

A **stack** is a curated, opinionated toolbox for a kind of project (e.g. web,
embedded). It's pure editorial: it *references* catalog tools by id and the
metrics bot never touches it. Picks are grouped by each tool's own `category`
(the index's existing sections), so a stack composes the existing vocabulary
rather than adding a parallel one. There are only two concepts: `category` and
`stack`.

Drop a file in `data/stacks/<id>.toml`:

```toml
id = "web"
name = "Web Frontend (WASM)"
description = "Shipping a Rust app to the browser via WebAssembly."

intro = """
Markdown lead-in: what's distinctive about tooling for this domain, and (in the
honest-take spirit) what it deliberately leaves out.
"""

[[pick]]
tool = "trunk"            # must resolve to data/tools/trunk.toml
note = "Why it earns a place here; the role is derived from its category."

[[pick]]
tool = "cargo-nextest"
```

`cargo test` checks that every `pick.tool` resolves to a known tool and that
stack ids are unique. Stacks surface on the index as the green **Stack** dropdown
(rightmost in the filter row): pick one and the page narrows to its tools. The
stack's `intro` and a derived `cargo install` line show in a banner, and each
pick's `note` appears inline on its tool's row. The install command is built
automatically from the picks that ship an installable binary; toolchain
components and library crates (marked `installable = false`) are listed as a
caveat instead.

## API

Everything is public and read-only:

- `GET /api/v1/tools`: the whole catalog as JSON (`?category=<id>` to filter)
- `GET /api/v1/tools/{id}`: a single tool
- `GET /?stack={id}`: the index filtered to a curated stack (the retired
  `/stacks` and `/stacks/{id}` pages redirect here)
- `GET /api/v1/docs`: Swagger UI (`/api/v1/openapi.json` for the raw spec)
- `GET /llms.txt`: the index as plain text, ready to paste into an LLM

## Deploying

`docker.yml` bakes the `server` binary together with `data/` and `static/` into a
small image, pushes it to ghcr.io, and triggers a Coolify redeploy. Since the data
ships inside the image, the running container makes no external calls, and merging
a metrics PR redeploys automatically.
