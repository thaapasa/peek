#!/usr/bin/env bash
# Fetch the Pdfium dynamic library into .pdfium/ and pin .pdfium/VERSION (used by the release
# workflow). Arg: build number (e.g. 7825) or "latest" (default) to resolve via the GitHub API.
set -euo pipefail

cd "$(dirname "$0")/.."

os=$(uname -s); arch=$(uname -m)
case "$os/$arch" in
  Darwin/arm64)              asset="pdfium-mac-arm64.tgz" ;;
  Darwin/x86_64)             asset="pdfium-mac-x64.tgz" ;;
  Linux/x86_64)              asset="pdfium-linux-x64.tgz" ;;
  Linux/aarch64|Linux/arm64) asset="pdfium-linux-arm64.tgz" ;;
  *) echo "unsupported host: $os/$arch" >&2; exit 1 ;;
esac

build="${1:-latest}"
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
