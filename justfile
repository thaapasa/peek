install_dir := env_var_or_default("PEEK_INSTALL_DIR", env_var_or_default("HOME", env_var_or_default("USERPROFILE", "")) / ".local/bin")

default:
    @just --list

# Install opt-in dev tools from mise.dev.toml (fuzzing, audit, demo capture)
setup:
    MISE_ENV=dev mise install

# Rust: two passes, because with imports_granularity a merge that pushes a line
# past max_width doesn't get wrapped until the next pass, so one run can leave
# the tree failing `--check`.
# Format Rust (nightly rustfmt) and Markdown (Prettier)
format:
    cargo +nightly fmt
    cargo +nightly fmt
    prettier --write --log-level warn "**/*.md"

# Check Rust + Markdown formatting, cargo check + clippy
lint:
    cargo +nightly fmt -- --check
    prettier --check --log-level warn "**/*.md"
    cargo check --workspace --all-targets
    cargo clippy --workspace --all-targets -- -D warnings

# Run all tests
test:
    cargo test --workspace

# Scan dependency tree for known vulnerabilities (needs `just setup`)
audit:
    cargo audit

# Capture manual demo stills (needs `just setup`); pass shot names to re-render some, none for all
demos *shots:
    ./scripts/capture-demos.sh {{ shots }}

# Run a detection fuzz target (needs nightly + `just setup`); see fuzz/README.md.
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

# Preview the manual at http://localhost:12620 (port from the shared 1AARR scheme)
manual:
    mdbook serve manual --port 12620

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
