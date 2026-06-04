# Discovery scripts

Throwaway helpers for **finding candidate tools** and pulling quick facts while
curating `data/tools/*.toml`. They are not part of the build or the deployed
app — they just save typing during research. Requires `curl` and `jq`.

## `discover.sh` — surface candidates from crates.io

Lists crates matching a keyword, ranked by recent downloads, with version and a
short description — a quick way to spot tools worth adding.

```sh
bash scripts/discover.sh cargo 2      # top "cargo" crates, 2 pages
bash scripts/discover.sh wasm 1
```

## `crate_info.sh` — quick facts for one crate

Dumps the crates.io summary (downloads, repository, latest version) for a single
crate, handy when filling in a new tool's `repository`/`crate` fields.

```sh
bash scripts/crate_info.sh cargo-nextest
```

Once a tool is added to `data/tools/`, the `generator` fills in the authoritative
`[metrics]` table — these scripts are only for the human discovery step.
