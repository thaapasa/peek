install_dir := env_var_or_default("PEEK_INSTALL_DIR", env_var("HOME") / ".local/bin")

default:
    @just --list

# Format codebase
format:
    cargo +nightly fmt

# Check formatting, cargo check + clippy
lint:
    cargo +nightly fmt -- --check
    cargo check --all-targets
    cargo clippy --all-targets -- -D warnings

# Run all tests
test:
    cargo test

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
    cargo check
    git add Cargo.toml Cargo.lock
    git commit -m "Bump version to $new"
