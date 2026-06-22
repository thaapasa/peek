install_dir := env_var_or_default("PEEK_INSTALL_DIR", env_var_or_default("HOME", env_var_or_default("USERPROFILE", "")) / ".local/bin")

default:
    @just --list

# Install dev tooling (Rust fuzzing + demo-capture pipeline). Idempotent; skips what's present.
setup:
    #!/usr/bin/env bash
    set -euo pipefail
    # Rust fuzzing (see `just fuzz`)
    command -v cargo-fuzz >/dev/null || cargo install --locked cargo-fuzz
    # Vulnerability scan of the dependency tree (see `just audit`)
    command -v cargo-audit >/dev/null || cargo install --locked cargo-audit
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

# Scan dependency tree for known vulnerabilities (needs `just setup` or `cargo install cargo-audit`)
audit:
    cargo audit

# Capture manual demo stills (needs `just setup`); pass shot names to re-render some, none for all
demos *shots:
    ./scripts/capture-demos.sh {{ shots }}

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
    ./scripts/fetch-pdfium.sh {{ build }}

# Bump project version, kind = patch | minor | major
bump kind="patch":
    #!/usr/bin/env bash
    set -euo pipefail
    new=$(./scripts/bump-version.sh {{ kind }})
    git add Cargo.toml Cargo.lock
    git commit -m "Bump version to $new"
