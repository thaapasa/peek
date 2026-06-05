# Crate split: extract `peek-io` + `peek-detect`

> **Status: Completed 2026-06-05.** Archived for reference.
>
> Workspace + `peek-io` + `peek-detect` extracted on branch `crate-split-detection`; the bin
> compiles via the `crate::input` façade. Green at landing: `cargo build`, `cargo test --workspace`
> (725 tests: 638 bin + 34 detect + 53 io), `cargo clippy --workspace`, fmt. Dependency boundary
> verified — `cargo tree -p peek-detect` carries no reader heavyweights. Live docs synced (CLAUDE.md,
> architecture-map.md, architecture.md, conventions.md). The crate roles are documented under
> "Crate structure" in [architecture.md](../architecture.md); the per-file map in
> [architecture-map.md](../architecture-map.md). The **detection-hardening backlog** below is the
> lasting value of this doc — it is the forward work the split was built to enable, referenced from
> [planned.md](../planned.md).

## Goal

Split the single `peek` crate (~46k LOC) into a Cargo workspace of three crates, so the
file-type-detection layer can be reviewed and hardened in isolation, and so the architectural rule
*"detection must not depend on content reading / viewers"* becomes a **compile-time** guarantee
instead of an unenforced convention.

Today the rule is not just unenforced — it is violated *in direction*: `src/input/detect.rs` reaches
**up** into 16 `crate::types::*` modules. A crate boundary forces the inversion: detection logic
moves *down*, readers depend *up* on the format enums.

## Target crate graph

```
crates/peek-io        IO + streaming primitives + raw codecs. "Stream don't load" layer.
   ▲                  Self-contained today (no crate::types/viewer/theme/info/extract refs).
   │
crates/peek-detect    FileType + all per-type format enums + per-type detect logic + mime
   ▲                  classification + the detect orchestrator + decompress-then-redetect.
   │                  Light deps: infer, mime_guess. ← the crate we want to review in isolation.
   │
peek (root bin)       types/ readers, viewer, theme, info, extract, output, cli, main.
```

Dependency rules (enforced by Cargo once split):
- `peek-io` depends on **nothing** in-tree.
- `peek-detect` depends on `peek-io` only.
- `peek` (bin) depends on both.
- Any reader reaching back into detection is now a missing-dependency compile error.

## Workspace layout

Convert root package into a workspace that still builds the `peek` binary at root:

```
peek/
  Cargo.toml            # [workspace] members + the root [package] "peek" (bin stays here)
  crates/
    peek-io/Cargo.toml + src/lib.rs
    peek-detect/Cargo.toml + src/lib.rs
  src/                  # unchanged location for the bin: main.rs, cli.rs, types/, viewer/, ...
```

Root `Cargo.toml` gains:
```toml
[workspace]
members = ["crates/peek-io", "crates/peek-detect"]
```
Root `[package]` keeps `name = "peek"`, `edition = "2024"`, and adds path deps:
```toml
peek-io = { path = "crates/peek-io" }
peek-detect = { path = "crates/peek-detect" }
```
Move only the dependencies each crate actually needs into its own manifest (see below); the rest stay
on the root bin. Keep `edition = "2024"` and the workspace `version`/lints consistent — lift shared
`[profile]`/lint config to `[workspace]` if present.

## What moves where

### → `peek-io` (`crates/peek-io/src/`)

Move whole, unchanged except `crate::input::` → `crate::` path fixups:

| from | to | notes |
|------|----|-------|
| `src/input/source.rs`   | `peek-io/src/source.rs`   | `InputSource`, `ByteSource`, `BytesByteSource` |
| `src/input/lines.rs`    | `peek-io/src/lines.rs`    | `LineSource` |
| `src/input/stream.rs`   | `peek-io/src/stream.rs`   | `ByteStream` |
| `src/input/stdin.rs`    | `peek-io/src/stdin.rs`    | unix tty reopen |
| `src/input/compression.rs` (codec half) | `peek-io/src/compression.rs` | **split — see below** |

`peek-io` deps (move off root): `bytes`, `tempfile`, `flate2`, `bzip2`, `liblzma`, `zstd`,
`lz4_flex`, `brotli-decompressor`, `anyhow`, plus the `cfg(unix)` stdin dep. (The archive container
crates — `zip`/`tar`/`sevenz-rust2` — stay on the root bin; they live in `types/archive`, not in the
io core.)

**New `peek-io` public type: `CompressionFormat`.** Today it lives in `detect.rs` (lines 170–215,
enum + `codec_label`/`suffix` impls). It is a pure codec enum (`Gz/Bz2/Xz/Zst/Lz4/Br`) — move enum +
both impl methods into `peek-io` (e.g. top of `compression.rs`). `peek-detect` then refers to it
downward.

### → `peek-detect` (`crates/peek-detect/src/`)

| from | to |
|------|----|
| `src/input/detect.rs` (orchestrator) | `peek-detect/src/lib.rs` (or `detect.rs` + thin lib re-export) |
| `src/input/mime.rs` | `peek-detect/src/mime.rs` (depends on `FileType`, so co-locate) |
| `src/input/compression.rs` (orchestration half) | `peek-detect/src/transparent.rs` — **see below** |
| each `src/types/<t>/format.rs` | `peek-detect/src/types/<t>.rs` (merge with detect below) |
| each `src/types/<t>/detect.rs` | merged into `peek-detect/src/types/<t>.rs` |

The 16 per-type pairs to merge (format + detect are tiny — 11–236 LOC each; merge into one module per
type under `peek-detect/src/types/`):

```
archive  audio  cert  comic  csv  disk_image  document  ebook
email    eps    font  objfile  spreadsheet  sqlite  structured  vobject
```
(`objfile` has only `detect.rs`, no `format.rs` — its `ObjectFile` variant carries no sub-format
enum; just move the detect fns.) (`pdf` has only `format.rs` (`PdfFlavor`) and no `detect.rs` — move
the enum; PDF is detected via magic mime in the orchestrator.)

Per-type detect modules expose only pure helpers — `format_from_ext(&str)`, `format_from_mime(&str)`,
`sniff_*(&[u8] | &str)` returning `Option<…Format>`. **Production code is already clean**; the only
reaches into reader/`info` modules are inside `#[cfg(test)]` mods (verified: `cert/detect.rs`
references `info::CertEntry` only under its test `mod`). Those tests either:
- move with the detect logic and get a `peek-detect`-local fixture, **or**
- stay in the bin beside the reader as a detection integration test.
Pick per-case; do not drag `info`/reader types into `peek-detect`.

`peek-detect` deps (move off root): `infer`, `mime_guess`, `anyhow`, `bytes`, plus `peek-io`.

### The `compression.rs` split (the one real cycle)

`compression.rs` is mutually entangled with `detect.rs`: it calls `detect` recursively (`redetect`)
to re-detect decompressed inner bytes, and references `CompressionFormat`/`Detected`/
`DecompressionContext`/`FileType`. Resolve by cutting it along the codec/orchestration seam:

- **Codec primitives → `peek-io`** (`peek-io/src/compression.rs`): `decompress_bytes(raw, fmt)` and
  the raw flate2/bzip2/liblzma/zstd/lz4_flex/brotli streaming decoders, plus the `CompressionFormat`
  enum + impls. Knows nothing about `FileType`/`Detected`.
- **Orchestration → `peek-detect`** (`peek-detect/src/transparent.rs`): `resolve_transparent`,
  `DecompressionContext`, and `Detected.decompressed_from`. `resolve_transparent` calls *down* into
  `peek-io` (`decompress_bytes`, `InputSource::memory`) and *sideways* into the orchestrator
  (`detect` / `redetect`). `compression_format_from_name` / `compression_format_from_mime`
  (detect.rs 581/607 — name→`CompressionFormat`) are detection logic: keep in `peek-detect`, output
  the io enum.

Result: `peek-io` codec layer has zero upward refs; the recursion lives entirely in `peek-detect`.

### Stays in the bin (`peek`)

Everything else: all of `src/types/<t>/` **except** the moved `format.rs`/`detect.rs`, plus `viewer/`,
`theme/`, `info/`, `extract/`, `output/`, `cli.rs`, `base64.rs`, `xml.rs`, `update.rs`, `main.rs`.

## Call-site churn mitigation — keep `crate::input` as a façade

The bin has **109** `crate::input::InputSource` sites and ~80 more `crate::input::detect::*` /
`crate::input::mime::*` sites. Do **not** rewrite them all. Keep a thin `src/input/mod.rs` in the bin
that re-exports the crates under the old paths:

```rust
// src/input/mod.rs  (bin-side façade — preserves crate::input::* paths)
pub use peek_io::{InputSource, LineSource, ByteStream, source, stream, stdin};
pub mod compression { pub use peek_io::compression::*; }
pub use peek_detect as detect;     // crate::input::detect::{FileType, Detected, detect, *Format}
pub use peek_detect::mime;         // crate::input::mime::MimeInfo
```
Adjust to match the exact symbols the grep inventory shows (`detect::detect`, `detect::Detected`,
`detect::FileType`, the per-type `*Format` re-exports, `mime::MimeInfo`, `compression::
resolve_transparent`, `source::ByteSource`, `source::BytesByteSource`). With the façade, **most bin
files compile unchanged.**

Reader-side imports of own format enums (`crate::types::<t>::format::<T>Format`) break once moved.
Two options, pick one and apply uniformly:
1. **Stub re-export** (lowest churn): leave `src/types/<t>/format.rs` as one line
   `pub use peek_detect::types::<t>::<T>Format;` Keeps reader imports byte-identical. Downside: 16
   stub files.
2. **Direct import** (cleaner end state): update reader files to `use peek_detect::types::<t>::…`.
   More edits, no stubs.

Recommendation: **(1) stubs first** to get green fast, then optionally collapse stubs in a follow-up
once the boundary is proven. The detect *façade* (`crate::input::detect`) is worth keeping
permanently — it is the historical public path and re-exporting is free.

## `peek-detect` public API surface (must re-export from its `lib.rs`)

Driven by the grep of what the bin consumes:
- `FileType`, `Detected`, `DecompressionContext`, `MimeInfo`
- `detect(&InputSource) -> Result<Detected>`, `detect_ignore_name`, `resolve_transparent`
- all per-type format enums: `ArchiveFormat AudioFormat CertFormat ComicFormat CsvFormat
  DiskImageFormat DocumentFormat EbookFormat EmailFormat PostScriptFormat FontFormat PdfFlavor
  SpreadsheetFormat SqliteFormat StructuredFormat VObjectFormat`
- `mime` module / `MimeInfo`
Re-export per-type `*Format` from the crate root so existing `detect::<T>Format` paths resolve through
the façade.

## Step order (each step compiles + `cargo test` green before next)

1. **Workspace skeleton.** Create empty `crates/peek-io` + `crates/peek-detect` with `lib.rs`
   stubs; wire `[workspace]` + path deps. Build (no-op crates).
2. **Move `peek-io` core** (source/lines/stream/stdin) + split out codec half of `compression.rs` +
   `CompressionFormat`. Fix `crate::input::` → `crate::` inside moved files. Bin still owns
   detect/mime/transparent for now via temporary path; get `peek-io` compiling standalone
   (`cargo build -p peek-io`).
3. **Move `peek-detect`**: detect orchestrator + mime + `transparent.rs` (resolve_transparent) +
   the 16 merged per-type modules + `pdf`/`objfile` special cases. Add the format-enum re-exports.
   `cargo build -p peek-detect`.
4. **Wire the façade.** Replace bin `src/input/` with the thin `mod.rs` re-export; add format-enum
   stubs (option 1). Delete the now-moved `src/input/*.rs` and `src/types/<t>/{detect,format}.rs`
   originals.
5. **Green the bin.** `cargo build && cargo test` at root. Fix residual import breaks (expect a
   handful the façade misses — `source::ByteSource`, `compression::resolve_transparent` path shape).
6. **`cargo fmt` + `cargo clippy`** across the workspace.

## Verification

- `cargo build -p peek-io` succeeds with **no** `peek-detect`/bin in its dep tree
  (`cargo tree -p peek-io` shows no `peek-detect`).
- `cargo build -p peek-detect` shows `peek-io` but **not** `peek` and **no** heavy reader crates
  (`calamine`/`rusqlite`/`object`/`symphonia`/`mail-parser`/`pdfium-render` absent from
  `cargo tree -p peek-detect`). This is the litmus that detection no longer pulls readers.
- `cargo test` (workspace) — full suite green; detection unit tests run under whichever crate they
  landed in.
- Smoke: run `peek` against one sample per major type + a `.tar.gz` (exercises the
  decompress→redetect path that crosses the io/detect seam) and a bare `.gz`.
- `cargo clippy --workspace` clean.

## Out of scope (do NOT bundle)

- Any detection *behaviour* change / hardening — that is the follow-up the split exists to enable.
  This branch is pure relocation; diff should be moves + path fixups + manifests only.
- Splitting `theme`/`viewer`/`info` into crates. Possible later; not now.
- Collapsing the per-type module count or renaming format enums.

## Follow-up backlog — detection hardening (NOT this branch)

The split exists to make these tractable: detection becomes a pure, reader-free, fuzzable surface.
Do them *after* the relocation lands, as separate changes. Ordered by value.

1. **Extension-vs-magic precedence (correctness, high).** `detect_file` runs `classify_by_name`
   before magic-mime, so a lying extension (`.txt` holding a PNG, `.csv` holding a zip) routes by
   name. The only correction — `detect_ignore_name` — fires *reactively*, on render failure
   (`detect.rs:264`), so silent mis-routes that don't fail render never self-correct. When name picks
   X but magic strongly says binary/container Y, prefer magic or proactively re-detect. Mind the
   deliberate `.ai`/`.pdf` ambiguity (both lead `%PDF`) — don't regress it.

2. **Unify the two detection paths (correctness/maintainability, high).** File path
   (`detect_file`) = name→magic→sniff→streaming whole-file UTF-8. In-memory path
   (`detect_bytes_named` → `detect_bytes`) = name→magic→head-only UTF-8→sniff. Different order *and*
   different UTF-8 rigor — a file valid-UTF-8 in the head but binary deep in the body classifies
   differently depending on entry path. Collapse to one core over a `Read`; parity-test both entry
   points.

3. **Bound the UTF-8 text/binary scan (perf, high).** `is_utf8_streaming` (`detect.rs:551`) reads the
   *entire* file to decide text-vs-binary when nothing else matched — a multi-GB extensionless log is
   fully scanned at detect time, before any viewer runs. Contradicts "stream don't load." Cap at
   first N MB: valid UTF-8 over the cap ⇒ treat as text.

4. **Truncated-head JSON sniff (correctness, medium).** `sniff_text_content` (`detect.rs:670`) runs
   `serde_json::from_str` on the head buffer only, so a large extensionless/stdin JSON fails to parse
   on the truncated head and falls through to plain text. Use a structural brace-sniff or
   valid-prefix acceptance instead of a full parse.

5. **Tighten loose heuristics (correctness, low).** YAML sniff (`detect.rs:708`) treats any `---\n`
   prefix as YAML — catches Markdown frontmatter and plain text with a `---` rule. Same family as
   #1: extension-routed binary types (`.zip`→Archive) and extension-only `.br` aren't magic-verified;
   mismatch is shown in info but not acted on.

6. **Fuzz / property-test the pure surface (the unlock, ongoing).** Once `peek-detect` has no reader
   crates, `detect_bytes` + every `sniff_*` / `format_from_*` is cheap to fuzz: never-panic,
   magic-byte corpus round-trips, two-path parity (#2). Today this would drag in
   calamine/rusqlite/pdfium build cost.

## Docs to update when landing

- `CLAUDE.md` architecture map — note the workspace + the two crates above `src/`.
- `docs/architecture-map.md` — `input/` now a bin-side façade over `peek-io` + `peek-detect`;
  per-type `format`/`detect` now live in `peek-detect`.
- `docs/architecture.md` — record the crate boundary + dependency rule as the enforcement mechanism
  for "detection must not depend on readers".
- Then delete or archive this plan per `docs/` hygiene rules.
