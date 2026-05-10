# Release Setup

How peek is packaged and published. User-facing install: [README.md](../README.md).

## Pipeline

`.github/workflows/release.yml`, manual dispatch (`workflow_dispatch`, no inputs). Three jobs:

1. **`prepare`** (ubuntu-24.04) — reads version from `Cargo.toml`, computes `TAG=vX.Y.Z`, fails fast
   if the tag exists on `origin`. Records previous tag (for release notes) and the dispatched SHA.
2. **`build`** — 5-target matrix, all built from the SHA from `prepare`:
    - `aarch64-apple-darwin` on `macos-14`
    - `x86_64-apple-darwin` on `macos-14` (cross-compiled from aarch64; macOS ships a universal SDK
      so `rustup target add` works out of the box, and the free `macos-13` Intel runner is gone)
    - `x86_64-unknown-linux-gnu` on `ubuntu-24.04`
    - `aarch64-unknown-linux-gnu` on `ubuntu-24.04-arm`
    - `x86_64-pc-windows-msvc` on `windows-latest`

   Each emits `peek-<version>-<target>.tar.gz` (`.zip` on Windows) + `.sha256` companion. Unix
   builds are stripped via the release profile. Each archive also bundles the matching Pdfium
   dynamic library (see [Pdfium bundling](#pdfium-bundling) below).
3. **`release`** — downloads artifacts, creates and pushes the tag at the built SHA, publishes a
   GitHub Release with all 5 archives + 5 `.sha256` + `install.sh` + auto-generated notes (
   `git log <previous-tag>..<tag>`).

Workflow needs `contents: write` to push the tag and create the release. No other secrets.

## Cutting a release

1. Bump `version` in `Cargo.toml` on `main`. Commit, push.
2. **Actions → Release → Run workflow**, branch `main`, dispatch.
3. Wait for all three jobs. Verify the release page has 5 archives + 5 `.sha256` + `install.sh`.
4. Smoke-test the installer:

   ```sh
   curl -fsSL https://raw.githubusercontent.com/thaapasa/peek/main/install.sh | sh
   peek --version
   ```

## Pdfium bundling

PDF support relies on Pdfium, a dynamic library built from Chromium's PDF stack. peek loads it via
`dlopen` at startup (see `src/types/pdf/package.rs::locate_bindings`) — first the directory of the
running executable, then `.pdfium/lib` under the project root (dev fallback), then system search.

The release pipeline ships the dylib next to the binary so the exe-dir lookup wins:

- Each build matrix entry carries `pdfium_asset` (e.g. `pdfium-mac-arm64.tgz`) and `pdfium_lib`
  (e.g. `lib/libpdfium.dylib`).
- The `Bundle Pdfium` step reads `BUILD` from `.pdfium/VERSION`, fetches
  `https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/$BUILD/<asset>`,
  extracts the platform's `pdfium_lib`, and drops it into `dist/$STAGE/` alongside `peek`.
- Pdfium's `LICENSE` is also copied as `LICENSE-pdfium`.

Tarball / zip layout:

```
peek-<version>-<target>/
  peek                   # or peek.exe on Windows
  libpdfium.dylib        # macOS — or libpdfium.so (Linux), pdfium.dll (Windows)
  README.md
  LICENSE
  LICENSE-pdfium
```

`install.sh` moves both `peek` and any `libpdfium.*` siblings into `$PEEK_INSTALL_DIR`. Windows
users extract manually; `pdfium.dll` lands next to `peek.exe`.

To bump the bundled Pdfium version, replace `.pdfium/` locally with a fresh tarball from
`bblanchon/pdfium-binaries` and commit the new `.pdfium/VERSION` (the rest of `.pdfium/` is
gitignored). The release workflow reads `BUILD` from that file, so the dev-time and release-time
versions stay in lockstep.

## Linux glibc baseline

Linux builds link against the glibc shipped with `ubuntu-24.04` (2.39). Older distros (Ubuntu 22.04,
Debian 12, RHEL 9) hit `version 'GLIBC_2.39' not found` and need to build from source. If broader
compatibility matters, switch to `ubuntu-22.04` (glibc 2.35, GitHub support through April 2027) or
add a musl static target.

## macOS signing

Binaries are **not** signed or notarized. Fine for `curl | sh` because `curl` doesn't set
`com.apple.quarantine` — Gatekeeper never inspects the binary. Browser-downloaded archives hit the
quarantine prompt; users need `xattr -d com.apple.quarantine peek` (or right-click → Open once).

## Windows caveats

Zip only — no install script. Piping text into `peek.exe` on Windows renders once to stdout but
doesn't enter the interactive viewer; the Unix tty-reopen trick has no Windows equivalent yet.

## Recovering from a failed release

Workflow refuses to run if `vX.Y.Z` already exists on `origin`. If a run fails partway and the tag
was already pushed, delete release + tag before retrying:

```sh
gh release delete vX.Y.Z --cleanup-tag -y    # deletes release + remote tag
git fetch --prune --prune-tags                # sync local tag list
git tag -d vX.Y.Z 2>/dev/null || true         # belt and braces
```

Then either:

- **Re-dispatch the same version** — transient failure (CI flake, GitHub outage), code unchanged →
  just run the workflow again.
- **Bump and re-dispatch** — content bug (bad binary, missing file) → bump `Cargo.toml` to the next
  patch, commit, push, dispatch.

If the failure happened *before* the tag was pushed (`prepare` or `build`), nothing needs cleanup —
fix and re-dispatch.

## Install script

`install.sh` at the repo root is hosted from `main` (raw GitHub URL) and attached to each release.
It:

- detects OS/arch via `uname`
- resolves the latest tag via the GitHub API (or honors `PEEK_VERSION`)
- downloads archive + `.sha256`
- verifies the checksum (`sha256sum` or `shasum`)
- extracts and moves `peek` to `$PEEK_INSTALL_DIR` (default `$HOME/.local/bin`)
- prints a PATH hint if the install dir isn't on `$PATH`

Strict POSIX `sh` — no Bashisms. Test changes via `sh -n install.sh` (syntax) and a real-release dry
run.

## Deliberately not automated

- **No Homebrew tap.** A formula could point at the same release tarballs; not yet.
- **No crates.io publish.** Separate decision.
- **No signed/notarized macOS binaries.** Apple Developer enrollment isn't worth it for the current
  audience.
- **No musl/static Linux build.** Revisit if older-distro support matters.
- **No auto-update.** Users re-run `install.sh`.
