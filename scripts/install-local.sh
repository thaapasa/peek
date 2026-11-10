#!/usr/bin/env bash
# Build release peek and install it with the Pdfium dylib (if .pdfium/lib/ has one) into
# $PEEK_INSTALL_DIR, default ~/.local/bin. Run via `mise run install`.
set -euo pipefail

cd "$(dirname "$0")/.."

install_dir="${PEEK_INSTALL_DIR:-${HOME:-$USERPROFILE}/.local/bin}"
cargo build --release
mkdir -p "$install_dir"
install -m 755 target/release/peek "$install_dir/peek"
echo "installed peek to $install_dir/peek"
shopt -s nullglob
libs=(.pdfium/lib/libpdfium.*)
if [ "${#libs[@]}" -gt 0 ]; then
  cp "${libs[@]}" "$install_dir/"
  for lib in "${libs[@]}"; do
    echo "installed $(basename "$lib") to $install_dir"
  done
else
  echo "warning: no .pdfium/lib/libpdfium.* found — run 'mise run pdfium' first for PDF support" >&2
fi
