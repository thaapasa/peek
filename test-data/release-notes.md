# Release Notes — Acme Toolkit `v3.7.0`

> **Heads up:** this release reshuffles a couple of CLI flags. See the
> [Migration](#migration) section before upgrading production hosts.

Released **2026-04-18** · Codename _"Northwind"_ · ~~Codename "Mistral" (dropped)~~

---

## Highlights

- New `acme scan` subcommand for streaming directory audits.
- 4× faster cold start on macOS (Apple Silicon).
- First-class support for `.acmerc.toml` configuration files.
- Bug fixes for the long-standing [#421](https://example.com/issues/421)
  "phantom file" issue.

## Table of contents

1. [Highlights](#highlights)
2. [Installation](#installation)
3. [What's new](#whats-new)
    - [Streaming scans](#streaming-scans)
    - [Configuration](#configuration)
4. [Migration](#migration)
5. [Benchmarks](#benchmarks)
6. [Known issues](#known-issues)
7. [Contributors](#contributors)

---

## Installation

The toolkit ships as a single static binary. Pick your platform:

| Platform     | Architecture | Download                          | SHA-256 (truncated) |
|--------------|--------------|-----------------------------------|---------------------|
| macOS        | `arm64`      | `acme-3.7.0-macos-arm64.tar.gz`   | `9f4c…b21a`         |
| macOS        | `x86_64`     | `acme-3.7.0-macos-x86_64.tar.gz`  | `2c8a…7f93`         |
| Linux        | `x86_64`     | `acme-3.7.0-linux-x86_64.tar.gz`  | `e1d7…0428`         |
| Linux (musl) | `aarch64`    | `acme-3.7.0-linux-aarch64.tar.gz` | `b602…1c5e`         |
| Windows      | `x86_64`     | `acme-3.7.0-windows-x86_64.zip`   | `47ee…a991`         |

Or install via the one-liner:

```sh
curl -fsSL https://example.com/install.sh | sh
```

If you'd rather build from source:

```sh
git clone https://github.com/example/acme-toolkit.git
cd acme-toolkit
cargo install --path . --locked
```

> **Note:** building requires Rust ≥ `1.78`. Older toolchains will fail
> at the `let-else` site in `src/scan/mod.rs`.

---

## What's new

### Streaming scans

The new `acme scan` command walks a directory tree without buffering
the full file list in memory:

```sh
acme scan ./src --include '*.{rs,toml}' --exclude target --json
```

Under the hood, `scan` uses a producer/consumer pipeline:

1. A walker thread emits `DirEntry` values onto a bounded channel.
2. Worker threads pull entries, hash file contents, and emit
   `ScanRecord` values back.
3. The serializer thread streams records to `stdout` as JSON Lines.

The pipeline is encoded roughly like this:

```rust
let (tx_entries, rx_entries) = bounded::<DirEntry>(256);
let (tx_records, rx_records) = bounded::<ScanRecord>(256);

thread::scope( | s| {
s.spawn( | _ | walker::walk(root, tx_entries));
for _ in 0..workers {
let rx = rx_entries.clone();
let tx = tx_records.clone();
s.spawn( move | _ | hasher::run(rx, tx));
}
s.spawn( | _ | serializer::run(rx_records, stdout));
});
```

Inline code such as `bounded::<DirEntry>(256)` and references to
identifiers like `ScanRecord` are highlighted as monospaced spans.

#### Output format

Each JSON Lines record contains:

| Field      | Type        | Description                            |
|------------|-------------|----------------------------------------|
| `path`     | `string`    | Repository-relative path.              |
| `size`     | `integer`   | File size in bytes.                    |
| `sha256`   | `string`    | Hex-encoded SHA-256 digest.            |
| `modified` | `timestamp` | RFC 3339 mtime (UTC).                  |
| `tags`     | `string[]`  | User-defined tags from `.acmerc.toml`. |

Example record:

```json
{
  "path": "src/main.rs",
  "size": 8421,
  "sha256": "9f4c01ee…",
  "modified": "2026-04-12T08:13:42Z",
  "tags": [
    "entry-point",
    "needs-review"
  ]
}
```

### Configuration

Place an `.acmerc.toml` file at any level of your tree. Settings cascade
from the workspace root down to the closest match.

```toml
# .acmerc.toml
[scan]
workers = 8
follow_symlinks = false

[scan.include]
patterns = ["**/*.rs", "**/*.toml"]

[scan.exclude]
patterns = ["target/**", ".git/**"]
```

> _Why TOML?_ It's the same format used elsewhere in the toolkit, the
> diagnostics are friendly, and we no longer need a YAML parser at all.

---

## Migration

The following flags were renamed for clarity. Old flags still work in
this release but emit a deprecation warning, and will be removed in
`v4.0.0`.

| Old flag      | New flag           | Notes                           |
|---------------|--------------------|---------------------------------|
| `--no-color`  | `--color=plain`    | Aligns with `--color=auto`.     |
| `--workers=N` | `--scan-workers=N` | Disambiguates from `--workers`. |
| `--threads`   | _removed_          | Use `--scan-workers`.           |

A minimal migration:

```diff
- acme scan ./src --no-color --workers=4
+ acme scan ./src --color=plain --scan-workers=4
```

If you rely on the legacy JSON shape from `acme list`, pin the schema:

```sh
acme list --schema=v1 ./src
```

The default schema is now `v2`.

### Breaking change checklist

- [x] Renamed `--no-color` → `--color=plain`
- [x] Renamed `--workers` → `--scan-workers`
- [x] Removed `--threads`
- [ ] _(planned for v4.0)_ Remove the legacy v1 JSON schema

---

## Benchmarks

Wall-clock time on a fresh checkout of the `linux` kernel
(*~95k files, ~1.4 GB*), median of five runs:

| Tool        | v3.6.0 | v3.7.0 | Δ        |
|-------------|--------|--------|----------|
| `acme scan` | 4.81 s | 1.12 s | **−77%** |
| `acme list` | 0.92 s | 0.88 s | −4%      |
| Cold start  | 280 ms | 65 ms  | **−77%** |

> Benchmarks run on an M3 Max, macOS 15.3, with the working set already
> in the page cache.

---

## Known issues

1. `acme scan` on case-insensitive filesystems can double-count paths
   that differ only in case. Tracking issue:
   [#503](https://example.com/issues/503).
2. The `--json` output for `scan` does not yet include extended
   attributes (xattrs / ADS) — see the design doc at
   `docs/scan/xattrs.md`.
3. Windows: long-path support requires `git config core.longpaths true`
   in repositories with deeply nested vendor directories.

For the full list, run:

```sh
acme bug-report --since v3.6.0
```

---

## Contributors

Thanks to everyone who shipped fixes, docs, or review feedback for this
release:

- @ada — recursion fix in `walker::walk`
- @grace — TOML config schema, migration docs
- @linus — Linux musl build pipeline
- @alan — bench harness rewrite

> _If we missed you, please open a PR against `CONTRIBUTORS.md` — the
> generator script lags behind by a release._

---

[^1]: `let-else` was stabilized in Rust 1.65, but our MSRV bumped to
1.78 because we now depend on `core::error::Error`.

<!-- internal: don't forget to bump the homebrew formula after tagging -->
