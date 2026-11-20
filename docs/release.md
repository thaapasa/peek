# Release setup

This doc describes how peek is packaged and published. For the user-facing install steps, see
[README.md](../README.md).

## Pipeline

The release pipeline is `.github/workflows/release.yml`. You start it by hand (`workflow_dispatch`),
and it takes one input, `bump`, which is `patch` (the default), `minor`, or `major`. The version
bump happens _in_ the pipeline. Nothing is bumped or committed locally. The pipeline has three jobs.

### 1. `prepare`

The `prepare` job runs on `ubuntu-24.04` and does these steps:

1. It reads the current version from `Cargo.toml` and applies the `bump` level to compute `vX.Y.Z`.
   If that tag already exists on `origin`, the job fails right away.
2. It bumps the version line in `Cargo.toml`. The version is the single workspace version under
   `[workspace.package]`. The `peek` bin and every member crate inherit it through
   `version.workspace = true`.
3. It runs `cargo update --workspace`, so that Cargo rewrites every internal crate entry in
   `Cargo.lock`. The `--workspace` flag keeps registry dependencies pinned. As a result, `--locked`
   builds stay valid, and adding a member crate never breaks this step again.
4. It commits as `github-actions[bot]` on a fresh `release/vX.Y.Z` branch and force-pushes that
   branch.

The job outputs the new version, the tag, the previous tag for the release notes, the branch name,
and the commit SHA of the branch.

### 2. `build`

The `build` job is a matrix of five targets. All of them build from the SHA of the release branch:

- `aarch64-apple-darwin` on `macos-15`
- `x86_64-apple-darwin` on `macos-15`, cross-compiled from aarch64. macOS ships a universal SDK, so
  `rustup target add` works out of the box. The free `macos-13` Intel runner is gone.
- `x86_64-unknown-linux-gnu` on `ubuntu-24.04`
- `aarch64-unknown-linux-gnu` on `ubuntu-24.04-arm`
- `x86_64-pc-windows-msvc` on `windows-latest`

Each target produces `peek-<version>-<target>.tar.gz`, or a `.zip` on Windows, with a `.sha256` file
beside it. The release profile strips the Unix builds. Each archive also bundles the matching Pdfium
dynamic library. For details, see [Pdfium bundling](#pdfium-bundling).

Before bundling, every _native_ matrix entry runs
`cargo test --workspace --release --locked --target <target>`. The native entries are Ubuntu x64,
Ubuntu ARM, Windows, and macOS ARM. This catches regressions specific to an architecture or an
operating system in targets that CI doesn't run, which are ARM Linux and both macOS targets. The
`x86_64-apple-darwin` target is cross-compiled on the arm64 `macos-15` runner, so its tests are
skipped, but the binary still ships. If a test fails in any target, the matrix stops
(`fail-fast: true`), and no release artifacts are published.

### 3. `release`

The `release` job does these steps:

1. It downloads the artifacts.
2. It merges `release/vX.Y.Z` into `main`. If `main` hasn't moved, it uses `--ff-only`. Otherwise it
   makes a `--no-ff` merge commit. The log line says which one it used.
3. It pushes `main` and tags the **commit on the release branch**. The reason follows this list.
4. It publishes a GitHub Release with all five archives, the five `.sha256` files, `install.sh`, and
   notes generated from `git log <previous-tag>..<tag>`.
5. It deletes the release branch.

### Ordering and the tag target

The bump commit reaches `main` only through a _successful_ release. If `build` fails, all that's
left is the throwaway `release/vX.Y.Z` branch. Because the merge runs before the tag, the tag is
created only once `main` actually has the commit.

The tag points at the release-branch commit that was _built_, not at the merge commit. With a
`--no-ff` merge, the tree of the merge commit picks up whatever landed on `main` after the branch
was cut. The release artifacts don't contain that code, so a tag on the merge commit would
misrepresent the release. The built commit is reachable from `main` through the merge in both cases,
so `git checkout vX.Y.Z` still works.

The workflow needs `contents: write` to push the branch, `main`, and the tag, and to create the
release. It needs no other secrets.

## Cutting a release

1. Go to **Actions → Release → Run workflow** and choose the `main` branch.
2. Pick the `bump` level. The default is `patch`.
3. Optionally, fill in **Highlights**. This is free-form Markdown that appears before the generated
   commit list on the release page. To ship only the commit list, leave it empty.
4. Dispatch the workflow.
5. Wait for all three jobs to finish. The pipeline bumps the version, builds, merges to `main`,
   tags, and publishes. You don't need to bump or commit anything locally.
6. Check that the release page has five archives, five `.sha256` files, and `install.sh`.
7. Smoke-test the installer:

   ```sh
   curl -fsSL https://raw.githubusercontent.com/thaapasa/peek/main/install.sh | sh
   peek --version
   ```

## Pdfium bundling

PDF support relies on Pdfium, a dynamic library built from the PDF stack of Chromium. peek loads it
with `dlopen` at startup, in `locate_bindings` in `crates/peek-types/src/types/pdf/package.rs`. It
looks in these places, in order:

1. The directory of the running executable.
2. `.pdfium/lib` under the project root, as a fallback for development.
3. The system search path.

The release pipeline ships the library next to the binary, so the lookup in the directory of the
executable wins:

- Each entry of the build matrix carries `pdfium_asset`, such as `pdfium-mac-arm64.tgz`, and
  `pdfium_lib`, such as `lib/libpdfium.dylib`.
- The `Bundle Pdfium` step reads `BUILD` from `.pdfium/VERSION` and fetches
  `https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/$BUILD/<asset>`. It
  extracts the platform's `pdfium_lib` and puts it into `dist/$STAGE/` beside `peek`.
- The step also copies the Pdfium `LICENSE` as `LICENSE-pdfium`.

The tarball or zip has this layout:

```
peek-<version>-<target>/
  peek                   # or peek.exe on Windows
  libpdfium.dylib        # macOS — or libpdfium.so (Linux), pdfium.dll (Windows)
  README.md
  LICENSE
  LICENSE-pdfium
```

`install.sh` moves both `peek` and any `libpdfium.*` files beside it into `$PEEK_INSTALL_DIR`.
Windows users extract the archive by hand, and `pdfium.dll` lands next to `peek.exe`.

To bump the bundled Pdfium version, replace `.pdfium/` locally with a fresh tarball from
`bblanchon/pdfium-binaries`. Then commit the new `.pdfium/VERSION`. Git ignores the rest of
`.pdfium/`. The release workflow reads `BUILD` from that file, so the development and release
versions always match.

## Linux glibc baseline

Linux builds link against the glibc that ships with `ubuntu-24.04`, which is 2.39. Older
distributions, such as Ubuntu 22.04, Debian 12, and RHEL 9, fail with
`version 'GLIBC_2.39' not found`, and their users need to build from source. If broader
compatibility matters, switch to `ubuntu-22.04`, which has glibc 2.35 and GitHub support through
April 2027. Another option is to add a static musl target.

## macOS signing

The binaries are **not** signed or notarized. This is fine for `curl | sh`, because `curl` doesn't
set `com.apple.quarantine`, so Gatekeeper never inspects the binary. Archives downloaded in a
browser trigger the quarantine prompt. Users then need to run `xattr -d com.apple.quarantine peek`,
or right-click the binary and choose **Open** once.

## Windows caveats

Windows gets only a zip archive, with no install script. If you pipe text into `peek.exe` on
Windows, it renders the text once to stdout and doesn't enter the interactive viewer. The Unix trick
of reopening the TTY has no Windows equivalent yet.

## Recovering from a failed release

The cleanup depends on where the release failed.

**If `prepare` or `build` failed:** `main` is untouched, and no tag exists. The only thing left is
the `release/vX.Y.Z` branch. The next run force-pushes that branch anyway, so cleanup is optional.
To delete the branch for tidiness, run `git push origin --delete release/vX.Y.Z`. Then fix the
problem and dispatch the workflow again.

**If `release` failed after the tag was pushed:** `main` already has the bump commit, and the tag
exists. Delete the release and the tag before you retry:

```sh
gh release delete vX.Y.Z --cleanup-tag -y    # deletes release + remote tag
git push origin --delete release/vX.Y.Z      # drop the release branch
git fetch --prune --prune-tags               # sync local tag list
```

The bump commit stays on `main`. If you dispatch again with the same `bump` level, the pipeline
computes a _new_ version on top of it. To cut the _same_ version again, first hard-reset `main` to
before the bump commit and force-push it. Or accept the skipped version and dispatch again.

For a transient failure, such as a flaky CI run or a GitHub outage, dispatch again with the same
`bump` level. The pipeline computes the version from `Cargo.toml` on each run, so a clean `main`
always gives the expected next version.

## Install script

`install.sh` is in the repository root. It's served from `main` through its raw GitHub URL, and it's
also attached to each release. The script does these steps:

1. It detects the operating system and the architecture with `uname`.
2. It finds the latest tag through the GitHub API, or uses `PEEK_VERSION` if that's set.
3. It downloads the archive and its `.sha256` file.
4. It verifies the checksum with `sha256sum` or `shasum`.
5. It extracts the archive and moves `peek` to `$PEEK_INSTALL_DIR`. The default is
   `$HOME/.local/bin`.
6. If the install directory isn't on `$PATH`, it prints a hint.

The script is strict POSIX `sh`, with no Bash-only features. To test a change, run
`sh -n install.sh` to check the syntax, and do a dry run against a real release.

## Deliberately not automated

- **No Homebrew tap.** A formula could point at the same release tarballs, but there isn't one yet.
- **No crates.io publish.** This is a separate decision.
- **No signed or notarized macOS binaries.** Apple Developer enrollment isn't worth it for the
  current audience.
- **No static musl Linux build.** Revisit this if support for older distributions matters.
- **No auto-update.** Users run `install.sh` again.
