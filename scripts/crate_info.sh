#!/usr/bin/env bash
#
# Throwaway helper: dump crates.io facts for a single crate, to help fill in a
# new tool's `repository` / `crate` fields in data/tools/<id>.toml.
#
# Usage: bash scripts/crate_info.sh <crate-name>
# Requires: curl, jq.

set -euo pipefail

UA="rust-tool-index-discovery (+https://tools.corrode.dev)"
NAME="${1:?usage: crate_info.sh <crate-name>}"

curl -fsS -A "$UA" "https://crates.io/api/v1/crates/${NAME}" \
  | jq '{
      name:        .crate.name,
      downloads:   .crate.downloads,
      recent:      .crate.recent_downloads,
      version:     .crate.newest_version,
      repository:  .crate.repository,
      homepage:    .crate.homepage,
      description: .crate.description
    }'
