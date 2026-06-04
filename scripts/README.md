# Scripts

Helpers used while curating the index. The discovery scripts (`discover.sh`,
`crate_info.sh`) are **throwaway**: they help with research and aren't part of
the build or deployed app. `build_icons.sh` is different: it vendors and
regenerates committed assets. The discovery scripts require `curl` and `jq`.

## `discover.sh`: surface candidates from crates.io

Lists crates matching a keyword, ranked by recent downloads, with version and a
short description (a quick way to spot tools worth adding).

```sh
bash scripts/discover.sh cargo 2      # top "cargo" crates, 2 pages
bash scripts/discover.sh wasm 1
```

## `crate_info.sh`: quick facts for one crate

Dumps the crates.io summary (downloads, repository, latest version) for a single
crate, handy when filling in a new tool's `repository`/`crate` fields.

```sh
bash scripts/crate_info.sh cargo-nextest
```

Once a tool is added to `data/tools/`, the `generator` fills in the authoritative
`[metrics]` table; these scripts are only for the human discovery step.

## `build_icons.sh`: vendor & build the category icons

Unlike the discovery helpers, this one **is** part of the committed assets. It
vendors the category icons from the [Lucide](https://lucide.dev) iconset
(ISC-licensed) into `static/icons/lucide/` (the individual source SVGs plus
their `LICENSE`) and regenerates the `static/icons/categories.svg` sprite from
those vendored files.

```sh
bash scripts/build_icons.sh            # download any missing icons, rebuild sprite
bash scripts/build_icons.sh --update   # re-download every icon, then rebuild
```

The category → icon mapping and the pinned Lucide version live at the top of the
script. To change an icon (or add one for a new category), edit the `MAP` and
re-run. Never hand-edit the generated `categories.svg`. The vendored files are
committed so the deployed container needs no network access. Requires `curl`
and `perl`.
