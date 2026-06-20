#!/usr/bin/env bash
# Capture demo stills into manual/src/img (needs `just setup`). Shared by the README and the
# mdbook manual — mdbook only bundles files under src/, so they live there. freeze keeps bg
# colors, so every mode renders faithfully; all shots are SVG (font.family=monospace → no
# 366KB font embed). Image/SVG/PDF renders are hand-captured — half-block pixels don't survive
# tmux+freeze — so no shot drives them here.
#
# Usage: capture-demos.sh [-s|--system] [shot-name ...]
#   No args   → capture every shot.
#   Shot names→ capture only those (e.g. `capture-demos.sh markdown-render csv-table`), so you can
#               re-render one still while iterating instead of rebuilding the whole set.
#   -s|--system → use `peek` from PATH (skip cargo build) when available. Fast iteration.
#   -h|--help → list shot names.
set -euo pipefail

names=(image-contour source-highlight markdown-render structured-data file-info \
       notebook csv-table sqlite-table archive-browser iso-browser dir-browser html-render \
       email-render ebook-render obj-symbols obj-sections java-bytecode)

use_system=
while [ "${1:-}" ]; do
  case "$1" in
    -h|--help)
      printf 'shots: %s\n' "${names[*]}"
      exit 0 ;;
    -s|--system)
      use_system=1; shift ;;
    *) break ;;
  esac
done

# want NAME → true when no selection was given (capture all) or NAME was named on the command line.
# Gating both the helpers and the prep-heavy blocks on this keeps a single source of truth for
# "is this shot wanted".
selected=("$@")
want() {
  [ "${#selected[@]}" -eq 0 ] && return 0
  local n; for n in "${selected[@]}"; do [ "$n" = "$1" ] && return 0; done
  return 1
}

cd "$(dirname "$0")/.."
# --system: use installed `peek` from PATH, skip the build. Falls back to a debug build when
# none is installed. abspeek (tshot drives peek by name inside tmux) is the bare command then,
# so the pane resolves it off PATH.
if [ "$use_system" ] && command -v peek >/dev/null 2>&1; then
  peek=$(command -v peek); abspeek=peek
  echo "using system peek: $peek"
else
  cargo build
  peek=target/debug/peek; abspeek="$PWD/$peek"
fi
out=manual/src/img; mkdir -p "$out"

shot() { # name width rows(0=full) peek-args...
  local name=$1 width=$2 rows=$3; shift 3
  want "$name" || return 0
  # awk (not head) so the stream is fully drained — head closing early would SIGPIPE
  # peek and trip `set -o pipefail`.
  local cap=(cat); [ "$rows" -gt 0 ] && cap=(awk -v n="$rows" 'NR<=n')
  "$peek" "$@" -p --color truecolor -w "$width" | "${cap[@]}" \
    | freeze --output "$out/$name.svg" --padding 20 --border.radius 8 --font.family monospace -c full
  echo "  $out/$name.svg"
}
# tshot captures an *interactive* peek screen (the browsers/viewers, not print mode): drive
# peek inside a tmux pane sized to the still, let it draw, dump the live screen as ANSI, and
# pipe to freeze. RGB terminal-feature keeps peek's 24-bit color through the capture.
# ready = regex unique to the TARGET screen (poll waits for it); keys = tmux send-keys
# sequence to drive a non-landing mode (empty for the landing view). q:quit is in every
# interactive status bar, so it's the universal "peek is up" signal before keys are sent.
tshot() { # name width height ready_regex keys peek-args...
  local name=$1 w=$2 h=$3 ready=$4 keys=$5; shift 5
  want "$name" || return 0
  tmux kill-server 2>/dev/null || true
  tmux new-session -d -s pkdemo -x "$w" -y "$h"
  tmux set -g default-terminal tmux-256color
  tmux set -as terminal-features ",*:RGB"
  for _ in $(seq 1 300); do
    if tmux capture-pane -t pkdemo -p 2>/dev/null | grep -qE '[$%#] *$|➜|❯'; then break; fi
  done
  tmux send-keys -t pkdemo "$abspeek $* --color truecolor --cell-aspect 1.7" Enter
  for _ in $(seq 1 600); do
    if tmux capture-pane -t pkdemo -p 2>/dev/null | grep -q 'q:quit'; then break; fi
  done
  # Send keys one at a time with a settle between each — peek's event loop
  # can coalesce a burst, and a descend (Enter) needs the row render to land
  # before it fires. Word-split on spaces: each token is one tmux key name.
  for k in $keys; do
    tmux send-keys -t pkdemo "$k"
    sleep 0.25
  done
  for _ in $(seq 1 600); do
    if tmux capture-pane -t pkdemo -p 2>/dev/null | grep -qiE "$ready"; then break; fi
  done
  tmux capture-pane -t pkdemo -e -p -J \
    | freeze --output "$out/$name.svg" --padding 20 --border.radius 8 --font.family monospace -c full
  tmux kill-server 2>/dev/null || true
  echo "  $out/$name.svg"
}

# --cell-aspect 2.0 pins the render to freeze's font geometry; without it peek auto-detects
# the *running* terminal's cell aspect and the still comes out stretched under freeze.
shot image-contour 80  0 test-images/heron.jpg -m contour --cell-aspect 2.0 # Sobel edge line-art
shot file-info 78 36 test-images/river-woods-hdr.jpg --info  # info screen (cap before GPS rows)

# Interactive container browsers
tshot archive-browser 96 18 'TOC' '' test-data/archive.zip      # ZIP, nested tree + status bar
tshot iso-browser 90 14 'TOC' '' test-data/sample.iso       # ISO 9660, multi-level nesting
tshot source-highlight 92 30 'Source' '' test-data/theme.rs      # syntax highlight
tshot markdown-render 88 32 'Rendered' '' test-data/release-notes.md   # rich markdown render
tshot notebook 88 30 'Rendered' '' test-data/notebook.ipynb  # Jupyter notebook
tshot html-render 88 30 'Rendered' '' test-data/formatted.html  # HTML content
tshot email-render 72 20 'Rendered' '' test-data/sample.eml      # email render
tshot structured-data 80 26 'Content' '' test-data/settings.jsonc   # JSON with comments
tshot csv-table 130 20 'Table' '' test-data/books.csv         # aligned CSV table
tshot sqlite-table 100 28 'Rows' 'Down Enter' test-data/library.sqlite  # rows from sqlite table
tshot ebook-render 88 28 'Read' 'n n n' test-books/frankenstein.epub    # EPUB chapter
tshot obj-sections 64 18 'Sections' 'Tab' test-data/tiny.obj     # .obj sections
tshot obj-symbols 64 18 'Symbols' 'Tab Tab' test-data/tiny.obj  # .obj symbols
tshot java-bytecode 80 32 'Bytecode' 'Tab Tab Tab' test-data/Sample.class  # Java classfile bytecode

# NB: pdf-render.webp (manual/src/img) is captured by hand, not here. The PDF Read view renders
# half-block pixels that tmux/freeze can't reproduce faithfully, so no tshot line drives it —
# replace it with a manual screenshot when the render changes.

# Directory browser — capture a clean checkout (worktree) so the still shows the structure a
# fresh clone sees, no local target/ / editor dirs. NB: mtimes are checkout-time, so unlike
# the fixture shots this one changes each run.
if want dir-browser; then
  tmp=$(mktemp -d); work="$tmp/peek"; git worktree add -q --detach "$work" HEAD
  tshot dir-browser 92 26 'Listing' '' "$work"
  git worktree remove --force "$work"; rm -rf "$tmp"
fi
