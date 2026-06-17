#!/usr/bin/env bash
# Bump the workspace version in Cargo.toml and refresh Cargo.lock. Arg: patch | minor | major
# (default patch). Prints ONLY the new version to stdout (the git commit in `just bump` captures
# it); progress + cargo output go to stderr so the capture stays clean.
set -euo pipefail

cd "$(dirname "$0")/.."

kind="${1:-patch}"
case "$kind" in patch|minor|major) ;; *) echo "kind must be patch|minor|major" >&2; exit 1 ;; esac

cur=$(awk -F'"' '/^version *=/ {print $2; exit}' Cargo.toml)
IFS=. read -r maj min pat <<<"$cur"
case "$kind" in
  major) maj=$((maj+1)); min=0; pat=0 ;;
  minor) min=$((min+1)); pat=0 ;;
  patch) pat=$((pat+1)) ;;
esac
new="$maj.$min.$pat"

awk -v v="$new" 'BEGIN{done=0} /^version *=/ && !done {sub(/"[^"]+"/, "\"" v "\""); done=1} {print}' Cargo.toml > Cargo.toml.tmp
mv Cargo.toml.tmp Cargo.toml
cargo check --workspace >&2

echo "$new"
