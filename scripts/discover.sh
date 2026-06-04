#!/usr/bin/env bash
#
# Throwaway discovery helper: list crates matching a keyword on crates.io,
# ranked by recent downloads, to surface candidates for the Rust Tool Index.
#
# Usage: bash scripts/discover.sh [query] [pages]
#   query  search term (default: "cargo")
#   pages  number of 100-result pages to fetch (default: 1)
#
# Requires: curl, jq. Output columns: name, recent_downloads, version, description.

set -euo pipefail

UA="rust-tool-index-discovery (+https://tools.corrode.dev)"
QUERY="${1:-cargo}"
PAGES="${2:-1}"

printf 'name\trecent_dl\tversion\tdescription\n'
for page in $(seq 1 "$PAGES"); do
  curl -fsS -A "$UA" \
    "https://crates.io/api/v1/crates?q=${QUERY}&per_page=100&page=${page}&sort=recent-downloads" \
    | jq -r '.crates[]
        | [ .name,
            (.recent_downloads // 0 | tostring),
            (.max_stable_version // .newest_version // ""),
            ((.description // "") | gsub("[\n\t]";" ") | .[0:80]) ]
        | @tsv'
done
