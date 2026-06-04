#!/usr/bin/env bash
#
# Vendor the category icons from the Lucide iconset (ISC-licensed) and build the
# `static/icons/categories.svg` sprite from the vendored sources.
#
# Why vendor: the deployed container makes no external calls, and we want the
# exact icon sources (and their license) committed in-repo rather than hand-
# transcribed path data. This script is the single source of truth for the
# category iconography — edit the MAP below and re-run, don't hand-edit the
# generated sprite.
#
# Usage:
#   bash scripts/build_icons.sh            # download missing icons, rebuild sprite
#   bash scripts/build_icons.sh --update   # re-download every icon, then rebuild
#
# Requires: curl, perl (both preinstalled on macOS/Linux).

set -euo pipefail

# Pinned for reproducibility. Bump deliberately, then run with --update.
LUCIDE_VERSION="1.17.0"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENDOR_DIR="$ROOT/static/icons/lucide"
SPRITE="$ROOT/static/icons/categories.svg"
UA="rust-tool-index-icons (+https://tools.corrode.dev)"
RAW="https://raw.githubusercontent.com/lucide-icons/lucide/${LUCIDE_VERSION}"

# category id (from data/categories.toml) -> Lucide icon name.
# Order matches the category render order. Glyphs are chosen to read clearly
# and distinctly at 14-18px and to avoid look-alikes (e.g. security uses a lock,
# not a shield, so it can't be confused with linting's shield-check).
MAP=(
  "toolchain:layers"
  "build:hammer"
  "cargo-extensions:puzzle"
  "dependencies:package"
  "testing:circle-check"
  "linting:shield-check"
  "formatting:text-align-start"
  "security:lock"
  "performance:gauge"
  "debugging:bug"
  "docs:book-open"
  "release:rocket"
  "cross-platform:globe"
  "productivity:terminal"
)

mkdir -p "$VENDOR_DIR"

fetch() { curl -fsSL -A "$UA" "$1" -o "$2"; }

# Vendor the upstream license alongside the icons.
if [ "${1:-}" = "--update" ] || [ ! -f "$VENDOR_DIR/LICENSE" ]; then
  fetch "$RAW/LICENSE" "$VENDOR_DIR/LICENSE"
fi

# Vendor each mapped icon's source SVG.
for entry in "${MAP[@]}"; do
  name="${entry##*:}"
  dest="$VENDOR_DIR/$name.svg"
  if [ "${1:-}" = "--update" ] || [ ! -f "$dest" ]; then
    echo "vendoring lucide/$name.svg"
    fetch "$RAW/icons/$name.svg" "$dest"
  fi
done

# Assemble the sprite from the vendored sources. We strip each file's <svg>
# wrapper and re-emit the inner elements under a <symbol>. Presentation is set
# as attributes ON each <symbol> (not via a shared <style>): when a symbol is
# referenced cross-file with <use href="categories.svg#...">, only that symbol's
# subtree is cloned, so a sibling <style> would not apply and shapes would fall
# back to fill:black/stroke:none. Attributes are inherited by the clone.
{
  cat <<'HEAD'
<svg xmlns="http://www.w3.org/2000/svg" style="display:none">
  <!-- GENERATED FILE — do not edit by hand.
       Built by scripts/build_icons.sh from the vendored Lucide iconset in
       static/icons/lucide/ (ISC-licensed; see static/icons/lucide/LICENSE).
       To change an icon, edit the MAP in the script and re-run it.
       Each symbol id matches a category id in data/categories.toml and is
       referenced via <use href="/static/icons/categories.svg#cat-<id>">.
       Stroke colour comes from the host element's `color` via currentColor. -->
HEAD

  for entry in "${MAP[@]}"; do
    id="${entry%%:*}"
    name="${entry##*:}"
    src="$VENDOR_DIR/$name.svg"
    printf '\n  <!-- %s -> lucide/%s -->\n' "$id" "$name"
    printf '  <symbol id="cat-%s" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">\n' "$id"
    perl -0777 -pe 's{.*?<svg\b[^>]*>}{}s; s{</svg>.*$}{}s' "$src" \
      | sed -e 's/^[[:space:]]*//' -e '/^$/d' -e 's/^/    /'
    printf '  </symbol>\n'
  done

  printf '</svg>\n'
} >"$SPRITE"

echo "Wrote $SPRITE from ${#MAP[@]} vendored Lucide icons (v$LUCIDE_VERSION)."
