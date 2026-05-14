# Release Setup

How peek is packaged and published. User-facing install: [README.md](../README.md).

## Pipeline

`.github/workflows/release.yml`, manual dispatch (`workflow_dispatch`) with one input: `bump` —
`patch` (default) / `minor` / `major`. The version bump happens *in* the pipeline; nothing is
bumped or committed locally. Three jobs:

1. **`prepare`** (ubuntu-24.04) — reads the current version from `Cargo.toml`, applies the `bump`
   level to compute `vX.Y.Z`, fails fast if that tag exists on `origin`. Bumps `Cargo.toml` +
   the `peek` entry in `Cargo.lock`, commits as `github-actions[bot]` on a fresh
   `release/vX.Y.Z` branch, and force-pushes that branch. Outputs the new version, tag, previous
   tag (for release notes), branch name, and the branch commit SHA.
2. **`build`** — 5-target matrix, all built from the release-branch SHA:
    - `aarch64-apple-darwin` on `macos-14`
    - `x86_64-apple-darwin` on `macos-14` (cross-compiled from aarch64; macOS ships a universal SDK
      so `rustup target add` works out of the box, and the free `macos-13` Intel runner is gone)
    - `x86_64-unknown-linux-gnu` on `ubuntu-24.04`
    - `aarch64-unknown-linux-gnu` on `ubuntu-24.04-arm`
    - `x86_64-pc-windows-msvc` on `windows-latest`

   Each emits `peek-<version>-<target>.tar.gz` (`.zip` on Windows) + `.sha256` companion. Unix
   builds are stripped via the release profile. Each archive also bundles the matching Pdfium
   dynamic library (see [Pdfium bundling](#pdfium-bundling) below).
3. **`release`** — downloads artifacts, then merges `release/vX.Y.Z` into `main`:
   `--ff-only` when `main` hasn't moved, otherwise a `--no-ff` merge commit (the log line says
   which). Pushes `main`, tags the **release-branch commit** (see below), publishes a GitHub
   Release with all 5 archives + 5 `.sha256` + `install.sh` + auto-generated notes
   (`git log <previous-tag>..<tag>`), and deletes the release branch.

The bump commit only ever reaches `main` through a *successful* release — a failed `build`
leaves nothing but the throwaway `release/vX.Y.Z` branch. Merge-before-tag ordering means the
tag is only created once `main` actually has the commit.

The tag points at the release-branch commit that was *built*, not the merge commit. On a
`--no-ff` merge the merge commit's tree picks up whatever raced onto `main` after the branch was
cut — code the release artifacts don't contain — so tagging it would misrepresent the release.
The built commit is reachable from `main` through the merge either way, so `git checkout vX.Y.Z`
still works.

Workflow needs `contents: write` to push the branch, push `main`, push the tag, and create the
release. No other secrets.

## Cutting a release

1. **Actions → Release → Run workflow**, branch `main`, pick the `bump` level (default `patch`),
   dispatch.
2. Wait for all three jobs. The pipeline bumps the version, builds, merges to `main`, tags, and
   publishes — no local bump or commit needed.
3. Verify the release page has 5 archives + 5 `.sha256` + `install.sh`.
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

Where the failure lands decides the cleanup:

- **`prepare` or `build` failed** — `main` is untouched, no tag exists. The only residue is the
  `release/vX.Y.Z` branch. The next run force-pushes that branch anyway, so cleanup is optional;
  delete it for tidiness with `git push origin --delete release/vX.Y.Z`. Fix and re-dispatch.
- **`release` failed after the tag was pushed** — `main` already has the bump commit and the tag
  exists. Delete the release + tag before retrying:

  ```sh
  gh release delete vX.Y.Z --cleanup-tag -y    # deletes release + remote tag
  git push origin --delete release/vX.Y.Z      # drop the release branch
  git fetch --prune --prune-tags               # sync local tag list
  ```

  The bump commit stays on `main` — re-dispatching with the same `bump` level would compute a
  *new* version on top of it. To re-cut the *same* version, hard-reset `main` back past the bump
  commit and force-push first, or just accept the version skip and re-dispatch.

Re-dispatch with the same `bump` level for a transient failure (CI flake, GitHub outage); the
pipeline recomputes the version from `Cargo.toml` each run, so a clean `main` always yields the
expected next version.

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
