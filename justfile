install_dir := env_var_or_default("PEEK_INSTALL_DIR", env_var("HOME") / ".local/bin")

default:
    @just --list

format:
    cargo +nightly fmt

lint:
    cargo +nightly fmt -- --check
    cargo check --all-targets
    cargo clippy --all-targets -- -D warnings

test:
    cargo test

install:
    cargo build --release
    mkdir -p "{{install_dir}}"
    install -m 755 target/release/peek "{{install_dir}}/peek"
    @echo "installed peek to {{install_dir}}/peek"

bump kind="patch":
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{kind}}" in patch|minor|major) ;; *) echo "kind must be patch|minor|major" >&2; exit 1 ;; esac
    cur=$(awk -F'"' '/^version *=/ {print $2; exit}' Cargo.toml)
    IFS=. read -r maj min pat <<<"$cur"
    case "{{kind}}" in
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
