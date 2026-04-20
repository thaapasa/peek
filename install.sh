#!/bin/sh
# peek installer — downloads a prebuilt binary from GitHub Releases.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/thaapasa/peek/main/install.sh | sh
#
# Environment variables:
#   PEEK_VERSION        Install a specific version (e.g. v0.2.0). Defaults to latest.
#   PEEK_INSTALL_DIR    Destination directory. Defaults to $HOME/.local/bin.

set -eu

REPO='thaapasa/peek'
BIN='peek'
INSTALL_DIR="${PEEK_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf 'peek-install: %s\n' "$1"; }
err() { printf 'peek-install: error: %s\n' "$1" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || err "missing required command: $1"
}

need uname
need curl
need tar
need mkdir
need mv
need rm
need chmod

# ---- detect target triple ---------------------------------------------------

os=$(uname -s)
arch=$(uname -m)

case "$os" in
  Darwin) os_part='apple-darwin' ;;
  Linux)  os_part='unknown-linux-gnu' ;;
  MINGW*|MSYS*|CYGWIN*)
    err "Windows is not supported by this installer. Download the .zip from https://github.com/$REPO/releases and extract peek.exe manually." ;;
  *) err "unsupported OS: $os" ;;
esac

case "$arch" in
  x86_64|amd64) arch_part='x86_64' ;;
  arm64|aarch64) arch_part='aarch64' ;;
  *) err "unsupported architecture: $arch" ;;
esac

TARGET="${arch_part}-${os_part}"

# ---- pick sha256 verifier ---------------------------------------------------

if command -v sha256sum >/dev/null 2>&1; then
  sha_check() { sha256sum -c "$1"; }
elif command -v shasum >/dev/null 2>&1; then
  sha_check() { shasum -a 256 -c "$1"; }
else
  err 'need sha256sum or shasum for checksum verification'
fi

# ---- resolve version --------------------------------------------------------

if [ -n "${PEEK_VERSION:-}" ]; then
  TAG="$PEEK_VERSION"
  case "$TAG" in
    v*) ;;
    *) TAG="v$TAG" ;;
  esac
else
  say 'resolving latest release...'
  latest_json=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest") \
    || err 'failed to query GitHub API for latest release'
  TAG=$(printf '%s' "$latest_json" | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')
  [ -n "$TAG" ] || err 'could not parse tag_name from GitHub API response'
fi

VERSION=${TAG#v}
ARCHIVE="${BIN}-${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/$REPO/releases/download/$TAG/$ARCHIVE"
SHA_URL="$URL.sha256"

say "installing $BIN $TAG for $TARGET"

# ---- download and verify ----------------------------------------------------

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t peek-install)
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT INT TERM

say "downloading $URL"
curl -fsSL -o "$tmp/$ARCHIVE" "$URL" \
  || err "failed to download $URL — check that $TAG has an asset for $TARGET"
curl -fsSL -o "$tmp/$ARCHIVE.sha256" "$SHA_URL" \
  || err "failed to download $SHA_URL"

say 'verifying checksum...'
(cd "$tmp" && sha_check "$ARCHIVE.sha256") >/dev/null \
  || err 'checksum verification failed — refusing to install'

# ---- extract and install ----------------------------------------------------

say "extracting to $tmp"
tar xzf "$tmp/$ARCHIVE" -C "$tmp"

STAGE="${BIN}-${VERSION}-${TARGET}"
SRC="$tmp/$STAGE/$BIN"
[ -f "$SRC" ] || err "archive did not contain $STAGE/$BIN"

mkdir -p "$INSTALL_DIR"
mv "$SRC" "$INSTALL_DIR/$BIN"
chmod +x "$INSTALL_DIR/$BIN"

say "installed $BIN to $INSTALL_DIR/$BIN"

# ---- PATH hint --------------------------------------------------------------

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    say "note: $INSTALL_DIR is not on your \$PATH"
    say "      add it with: export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac

"$INSTALL_DIR/$BIN" --version 2>/dev/null || true
