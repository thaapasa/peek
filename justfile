install_dir := env_var_or_default("PEEK_INSTALL_DIR", env_var_or_default("HOME", env_var_or_default("USERPROFILE", "")) / ".local/bin")

default:
    @just --list

# Install dev tooling (Rust fuzzing + demo-capture pipeline). Idempotent; skips what's present.
setup:
    #!/usr/bin/env bash
    set -euo pipefail
    # Rust fuzzing (see `just fuzz`)
    command -v cargo-fuzz >/dev/null || cargo install --locked cargo-fuzz
    # Stills: charmbracelet/freeze — ANSI -> SVG/PNG, keeps fg AND bg truecolor (see `just demos`)
    command -v freeze    >/dev/null || brew install charmbracelet/tap/freeze
    # Interactive-screen capture for `just demos` (drives peek in a pane, dumps the live screen)
    command -v tmux      >/dev/null || brew install tmux
    # Animated demos: svg-term-cli — asciicast -> CSS-animated SVG that autoplays on GitHub
    command -v svg-term  >/dev/null || npm install -g svg-term-cli
    # Terminal recorder feeding the animated path
    command -v asciinema >/dev/null || brew install asciinema
    echo "dev tools ready"

# Format codebase
format:
    cargo +nightly fmt

# Check formatting, cargo check + clippy
lint:
    cargo +nightly fmt -- --check
    cargo check --workspace --all-targets
    cargo clippy --workspace --all-targets -- -D warnings

# Run all tests
test:
    cargo test --workspace

# Capture demo stills into manual/src/img (needs `just setup`). Shared by the README and the
# mdbook manual — mdbook only bundles files under src/, so they live there. freeze keeps bg
# colors, so every mode renders faithfully; SVG for text/line-art (font.family=monospace → no
# 366KB font embed), PNG for the photo render (raster is honest for half-block pixels).
demos:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release
    peek=target/release/peek
    out=manual/src/img; mkdir -p "$out"
    shot() { # name format width rows(0=full) peek-args...
      local name=$1 fmt=$2 width=$3 rows=$4; shift 4
      local font=(); [ "$fmt" = svg ] && font=(--font.family monospace)
      # awk (not head) so the stream is fully drained — head closing early would SIGPIPE
      # peek and trip `set -o pipefail`.
      local cap=(cat); [ "$rows" -gt 0 ] && cap=(awk -v n="$rows" 'NR<=n')
      "$peek" "$@" -p --color truecolor -w "$width" | "${cap[@]}" \
        | freeze --output "$out/$name.$fmt" --padding 20 --border.radius 8 "${font[@]}"
      echo "  $out/$name.$fmt"
    }
    # tshot captures an *interactive* peek screen (the browsers/viewers, not print mode): drive
    # peek inside a tmux pane sized to the still, let it draw, dump the live screen as ANSI, and
    # pipe to freeze. RGB terminal-feature keeps peek's 24-bit color through the capture.
    abspeek="$PWD/$peek"
    tshot() { # name width height peek-args...
      local name=$1 w=$2 h=$3; shift 3
      tmux kill-server 2>/dev/null || true
      tmux new-session -d -s pkdemo -x "$w" -y "$h"
      tmux set -g default-terminal tmux-256color
      tmux set -as terminal-features ",*:RGB"
      for _ in $(seq 1 300); do
        if tmux capture-pane -t pkdemo -p 2>/dev/null | grep -qE '[$%#] *$|➜|❯'; then break; fi
      done
      tmux send-keys -t pkdemo "$abspeek $* --color truecolor" Enter
      for _ in $(seq 1 600); do
        if tmux capture-pane -t pkdemo -p 2>/dev/null | grep -qiE 'TOC|Listing'; then break; fi
      done
      tmux capture-pane -t pkdemo -e -p -J \
        | freeze --output "$out/$name.svg" --padding 20 --border.radius 8 --font.family monospace
      tmux kill-server 2>/dev/null || true
      echo "  $out/$name.svg"
    }
    # --cell-aspect 2.0 pins the render to freeze's font geometry; without it peek auto-detects
    # the *running* terminal's cell aspect and the still comes out stretched under freeze.
    shot image-render     png 80  0 test-images/heron.jpg --cell-aspect 2.0            # glyph photo render
    shot image-contour    svg 80  0 test-images/heron.jpg -m contour --cell-aspect 2.0 # Sobel edge line-art
    shot source-highlight svg 92 28 test-data/theme.rs               # syntax highlight
    shot markdown-render  svg 88 30 test-data/release-notes.md       # rich markdown render
    shot structured-data  svg 80 26 test-data/config.json           # JSON pretty-print
    shot file-info        svg 78 36 test-images/river-woods-hdr.jpg --info  # info screen (cap before GPS rows)
    shot notebook         svg 88 24 test-data/notebook.ipynb        # Jupyter notebook
    shot csv-table        svg 100 14 test-data/books.csv            # aligned CSV table
    # SQLite browses table rows in a streaming viewer; print mode shows the schema instead, so
    # extract one table to CSV and render it as the same aligned table the row viewer draws.
    tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
    "$peek" test-data/library.sqlite -x tables/authors.csv -o "$tmp/authors.csv" >/dev/null
    shot sqlite-table     svg 90 14 "$tmp/authors.csv"             # a table's rows, browsable
    # Interactive container browsers — the nested TOC tree, which the flat --list can't show.
    tshot archive-browser 96 18 test-data/archive.zip               # ZIP, nested tree + status bar
    tshot iso-browser     90  9 test-data/sample.iso                # ISO 9660, multi-level nesting
    # Directory browser — capture a clean checkout (worktree) so the still shows the structure a
    # fresh clone sees, no local target/ / editor dirs. NB: mtimes are checkout-time, so unlike
    # the fixture shots this one changes each run.
    work="$tmp/peek"; git worktree add -q --detach "$work" HEAD
    tshot dir-browser 92 26 "$work"
    git worktree remove --force "$work"

# Run a detection fuzz target (needs nightly + `cargo install cargo-fuzz`); see fuzz/README.md.
# verbosity=0 (default) silences the per-event NEW/REDUCE spam — faster, crashes + final stats only; pass verbosity=1 to debug coverage.
fuzz target="detect_bytes" secs="60" verbosity="0":
    cargo +nightly fuzz run {{ target }} --fuzz-dir fuzz -- -max_total_time={{ secs }} -verbosity={{ verbosity }} -print_final_stats=1

# Install peek locally (binary + Pdfium dylib if .pdfium/lib/ has one)
install:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release
    mkdir -p "{{ install_dir }}"
    install -m 755 target/release/peek "{{ install_dir }}/peek"
    echo "installed peek to {{ install_dir }}/peek"
    shopt -s nullglob
    libs=(.pdfium/lib/libpdfium.*)
    if [ "${#libs[@]}" -gt 0 ]; then
      cp "${libs[@]}" "{{ install_dir }}/"
      for lib in "${libs[@]}"; do
        echo "installed $(basename "$lib") to {{ install_dir }}"
      done
    else
      echo "warning: no .pdfium/lib/libpdfium.* found — run 'just pdfium' first for PDF support" >&2
    fi

# Fetch Pdfium dynamic library into .pdfium/ (build="latest" or e.g. "7825"); also pins .pdfium/VERSION used by release workflow
pdfium build="latest":
    #!/usr/bin/env bash
    set -euo pipefail

    os=$(uname -s); arch=$(uname -m)
    case "$os/$arch" in
      Darwin/arm64)              asset="pdfium-mac-arm64.tgz" ;;
      Darwin/x86_64)             asset="pdfium-mac-x64.tgz" ;;
      Linux/x86_64)              asset="pdfium-linux-x64.tgz" ;;
      Linux/aarch64|Linux/arm64) asset="pdfium-linux-arm64.tgz" ;;
      *) echo "unsupported host: $os/$arch" >&2; exit 1 ;;
    esac

    build="{{ build }}"
    if [ "$build" = "latest" ]; then
      echo "resolving latest pdfium build..."
      # Pure-bash extract; piping into grep -m1 trips pipefail (SIGPIPE
      # back to the upstream printf/curl).
      api_body=$(curl -fsSL https://api.github.com/repos/bblanchon/pdfium-binaries/releases/latest)
      if [[ "$api_body" =~ \"tag_name\":[[:space:]]*\"chromium/([0-9]+)\" ]]; then
        build="${BASH_REMATCH[1]}"
      else
        echo "could not resolve latest pdfium build from GitHub API" >&2
        exit 1
      fi
    fi

    url="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/$build/$asset"
    echo "fetching $url"

    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    curl -fsSL -o "$tmp/pdfium.tgz" "$url"

    rm -rf .pdfium
    mkdir .pdfium
    tar xzf "$tmp/pdfium.tgz" -C .pdfium

    if [ -f .pdfium/VERSION ]; then
      echo "installed pdfium build $build to .pdfium/"
      cat .pdfium/VERSION
    else
      echo "warning: extracted but .pdfium/VERSION missing" >&2
    fi

# Bump project version, kind = patch | minor | major
bump kind="patch":
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{ kind }}" in patch|minor|major) ;; *) echo "kind must be patch|minor|major" >&2; exit 1 ;; esac
    cur=$(awk -F'"' '/^version *=/ {print $2; exit}' Cargo.toml)
    IFS=. read -r maj min pat <<<"$cur"
    case "{{ kind }}" in
      major) maj=$((maj+1)); min=0; pat=0 ;;
      minor) min=$((min+1)); pat=0 ;;
      patch) pat=$((pat+1)) ;;
    esac
    new="$maj.$min.$pat"
    awk -v v="$new" 'BEGIN{done=0} /^version *=/ && !done {sub(/"[^"]+"/, "\"" v "\""); done=1} {print}' Cargo.toml > Cargo.toml.tmp
    mv Cargo.toml.tmp Cargo.toml
    cargo check --workspace
    git add Cargo.toml Cargo.lock
    git commit -m "Bump version to $new"
