# Release Setup

How peek is packaged and published. For the user-facing install instructions,
see [README.md](../README.md).

## Pipeline overview

Releases are produced by `.github/workflows/release.yml`, triggered manually
from the GitHub Actions tab (`workflow_dispatch`, no inputs). The workflow
has three jobs:

1. **`prepare`** (ubuntu-24.04) — reads the version from `Cargo.toml`,
   computes `TAG=vX.Y.Z`, and fails fast if that tag already exists on
   `origin`. Also records the previous tag (for release notes) and the SHA
   of the commit the workflow was dispatched against.
2. **`build`** — matrix of 5 targets, each built from the exact SHA from
   `prepare`:
   - `aarch64-apple-darwin` on `macos-14`
   - `x86_64-apple-darwin` on `macos-14` (cross-compiled from aarch64 —
     GitHub retired the free `macos-13` Intel runner, and macOS ships a
     universal SDK so cross-compilation works out of the box with
     `rustup target add`)
   - `x86_64-unknown-linux-gnu` on `ubuntu-24.04`
   - `aarch64-unknown-linux-gnu` on `ubuntu-24.04-arm`
   - `x86_64-pc-windows-msvc` on `windows-latest`

   Each produces `peek-<version>-<target>.tar.gz` (or `.zip` on Windows)
   with a `.sha256` companion file. Unix builds are stripped via the
   release profile in `Cargo.toml`.
3. **`release`** — downloads all artifacts, creates and pushes the tag
   pointing at the built SHA, then publishes a GitHub Release with:
   - all 5 archives,
   - all 5 `.sha256` files,
   - `install.sh`,
   - auto-generated notes (`git log <previous-tag>..<tag>`).

The workflow needs `contents: write` to push the tag and create the
release. No other secrets.

## Cutting a release

1. Bump `version` in `Cargo.toml` on `main`. Commit and push.
2. Go to **Actions → Release → Run workflow**, select the `main` branch,
   dispatch.
3. Wait for all three jobs to complete. Verify the release page has
   the 5 archives + 5 `.sha256` + `install.sh` attached.
4. Smoke-test the installer on at least one platform:

   ```sh
   curl -fsSL https://raw.githubusercontent.com/thaapasa/peek/main/install.sh | sh
   peek --version
   ```

## Linux glibc baseline

Linux builds link against the glibc shipped with `ubuntu-24.04` (2.39).
Users on older distros (Ubuntu 22.04, Debian 12, RHEL 9) will see
`version 'GLIBC_2.39' not found` and need to build from source instead.
If broader compatibility becomes important, switch to `ubuntu-22.04`
(glibc 2.35, supported by GitHub through April 2027) or add a musl
static target.

## macOS signing

Binaries are **not** signed or notarized. This is fine for the `curl | sh`
install path because `curl` does not set the `com.apple.quarantine`
extended attribute, so Gatekeeper never inspects the binary. Users who
download the archive via a browser will hit the quarantine prompt and
need `xattr -d com.apple.quarantine peek` (or right-click → Open once).

## Windows caveats

Windows gets a `.zip` only — no install script. Piping text into
`peek.exe` on Windows renders once to stdout but does not enter the
interactive viewer; the Unix tty-reopen trick in `src/main.rs` has no
Windows equivalent yet.

## Recovering from a failed release

The workflow refuses to run if `vX.Y.Z` already exists on `origin`. If a
run fails partway — e.g. a matrix job died, or artifacts are wrong — and
the tag was already pushed by the `release` job, you need to delete both
the release and the tag before re-running:

```sh
gh release delete vX.Y.Z --cleanup-tag -y    # deletes release + remote tag
git fetch --prune --prune-tags                # sync local tag list
git tag -d vX.Y.Z 2>/dev/null || true         # belt and braces
```

Then either:

- **Re-dispatch the same version** — if the failure was transient (CI
  flake, GitHub outage) and the code on `main` is unchanged, just run
  the workflow again.
- **Bump and re-dispatch** — if the failure revealed a content bug (bad
  binary, missing file), bump `Cargo.toml` to the next patch version,
  commit, push, and dispatch.

If a run failed *before* the tag was pushed (i.e. in `prepare` or
`build`), nothing needs cleanup — just fix the problem and re-dispatch.

## Install script

`install.sh` at the repo root is hosted from `main` (raw GitHub URL) and
also attached to each release. It:

- detects OS/arch via `uname`,
- resolves the latest tag via the GitHub API (or honors `PEEK_VERSION`),
- downloads the archive + `.sha256`,
- verifies the checksum (`sha256sum` or `shasum`),
- extracts and moves `peek` to `$PEEK_INSTALL_DIR` (default
  `$HOME/.local/bin`),
- prints a PATH hint if the install dir is not on `$PATH`.

It is strict POSIX `sh` — no Bashisms. Test changes by piping to
`sh -n install.sh` (syntax) and running locally against a real release.

## What's deliberately *not* automated

- **No Homebrew tap.** Can be added later; a formula would point at the
  same release tarballs.
- **No crates.io publish.** Separate decision.
- **No signed/notarized macOS binaries.** Not worth the Apple Developer
  enrollment for the current audience.
- **No musl/static Linux build.** Revisit if we need older-distro support.
- **No auto-update.** peek does not self-update; users re-run `install.sh`.
