# peek — Planned Features & Ideas

Status legend: ☐ planned · ❓ idea / open

For implemented (✅) and partial (◐) features, see [features.md](features.md).

This doc is ordered as a **release plan**: nearest-term hardening first, then the
1.0 bar, then post-1.0 deepening, then the open-idea graveyard. The roadmap below is
the index; the detailed per-feature notes follow under the same milestone headings.

## Roadmap

Feature *breadth* is already 1.0-level — the gap is robustness against the project's own
two north stars (*stream, don't load*; *multi-GB first-class*). The plan closes the
remaining streaming leaks, then deepens.

### 0.4 — Hardening (make the tagline true) ☐

The "stream, don't load" / "multi-GB first-class" promises still have leaks. Not
features — the marketing claim not yet fully holding.

- **Memory / Streaming audit** ◐ — only the stdin slurp remains (documented, spill
  deferred). See [§ Memory / Streaming](#memory--streaming-).
- **Large File Safeguards** ◐ — one optional idea (generalized info-default landing)
  open. See [§ Large File Safeguards](#large-file-safeguards-).
- **Install / supply-chain hardening** ☐ — from the 2026-06-21 security review: no
  release signatures, unverified Pdfium fetch, `install.sh` pulled from `main`. See
  [§ Install / supply-chain hardening](#install--supply-chain-hardening-).
- **Parser fuzzing backlog** ☐ — fuzzing reaches only `detect()` today; the per-type
  binary/text parsers are untested by coverage-guided fuzzing. See
  [§ Parser fuzzing backlog](#parser-fuzzing-backlog-).

### 1.0 — the last user-facing must-haves ✅

No remaining blockers. Feature breadth, the streaming guards, and robustness are at the 1.0 bar;
the shipped Text Search scope is accepted as the 1.0 feature. Its further refinements moved to
post-1.0 deepening below.

### Post-1.0 / 1.x — deepening ☐

- **Text Search refinements** — incremental re-scan, wider reach (info view + hex dump), and a
  lazy/bounded scan. Quality-of-life on an already-shipped feature. See
  [§ Text Search](#text-search).

- **Block Collapsing / Folding** — the biggest single new capability and the natural
  1.1 headline; big lift (line-metadata layer). See
  [§ Block Collapsing / Folding](#block-collapsing--folding-).
- **File-type deepening** — enhancements to already-shipped types (SQL formatter, CSS
  specificity, multi-page `.ps`, PKCS#12, RAR, m4b chapters, …). None block 1.0; pick
  by demand. See [§ File Types](#file-types).

### Open ideas / deferred indefinitely ❓

- **Video → ASCII** — stretch; ffmpeg dep, decode-perf wall. See
  [§ Video Files](#video-files-).
- **HEIC / PSD / JPEG-XL / camera RAW** — detected-but-not-rendered: needs a new
  decoder dep the `image` crate doesn't provide. See
  [§ Image-decode gap](#image-decode-gap-).
- **Legacy OLE office** (`.doc` / `.xls` / `.ppt`) + **binary plist** — old compound-
  file / `bplist00` formats not handled today. See [§ Legacy & Apple binary
  formats](#legacy--apple-binary-formats-).
- **Parquet** — columnar data-eng standard; schema + row-group metadata. See
  [§ Parquet](#parquet-).
- **Type-support plugin trait** — analysed and parked; revisit only if type count
  outgrows the central matches or external plugins become a goal. See
  [§ Type-support plugin trait](#type-support-plugin-trait-).
- **DMG nested-filesystem metadata / entry extract** — significant work, "probably
  never worth it." See [§ Disk Images](#disk-images-).
- **Unified `Timestamp` type** — fold the remaining String / decomposed / freeform date
  sites onto one granularity enum behind `Value::Timestamp`. Cleanup, demand-driven. See
  [§ Unified Timestamp type](#unified-timestamp-type-).

---

## 0.4 — Hardening

### Large File Safeguards ◐ (one optional idea open)

The "multi-GB first-class" guards shipped — see [features.md](features.md) and the
strategy doc [memory-streaming.md](memory-streaming.md). One optional item remains:

- ◐ **Generalize the info-default landing** beyond decompress / TOC — a
  `Mode::loads_whole_file()` signal that lands *any* over-threshold transform view on
  Info. Fold in only if a non-decompress view ever needs it.

### Detection hardening ◐

The `peek-detect` crate split (see the archived
[crate-split plan](archived/crate-split-plan.md)) made these tractable: detection is now
a small, reader-free, fuzzable surface. Full rationale + file/line references live in
that plan's "Follow-up backlog" section. Remaining:

- ☐ **[1.x] Tighten loose heuristics.** YAML `---` prefix over-matches; extension-
  routed binary types and `.br` aren't magic-verified.

### Parser fuzzing backlog ☐

Today the only fuzz coverage is `detect()` — `fuzz/fuzz_targets/detect_bytes.rs`
(libFuzzer) plus the stable proptest floor `crates/peek-detect/tests/fuzz_detect.rs`.
Every **per-type** parser in `crates/peek-types` is untested by coverage-guided fuzzing;
they have unit tests (ds_store notably has hostile-input tests) but no fuzzer. These
parse the most attacker-controlled binary structure in the project, so this is the
highest-value defense-in-depth follow-up from the 2026-06-21 security review (finding
L4). The parser layer reviewed *clean* — no reachable panic was found by hand — so this
is about keeping it that way as the parsers change, not chasing a known bug.

**Infra blocker (do first):** the fuzz crate is workspace-excluded (libfuzzer-sys needs
nightly) and depends **only on `peek-detect` + `peek-io`**. To fuzz any `peek-types`
parser you must first add `peek-types = { path = "../crates/peek-types" }` (and
`peek-foundation` for the base64/xml helpers) to `fuzz/Cargo.toml`. Once that's in,
adding a target is a `[[bin]]` block + a one-line `fuzz_target!(|data: &[u8]| { … })`,
run via `just fuzz target=<name> secs=<n>`. There is **no** `#[cfg(fuzzing)]`
infrastructure today and most candidates below are already `pub`, so no harness
plumbing is needed — except the few `pub(super)` fns (e.g. `mbr::parse`), which would
need a `pub` bump or a `testing`-style feature to expose.

One target needs **no new deps** and can land immediately: `peek_io::LineSource` (drive
`open` + `window` over `InputSource::memory(data, "f")`) — exercises the anchor-index
scan and the UTF-8 + terminal-sanitize decode in `crates/peek-io/src/lines.rs`.

**Tier 1 — trivial wiring, high value** (hand-written, take `&[u8]`/`&str` directly; one-line targets once `peek-types` is a dep):

- `ds_store::reader::parse(&[u8])` — `ds_store/reader.rs` — Bud1 buddy-allocator: offset
  table + block-address math, UTF-16 name lengths, recursive B-tree walk (already
  depth+visited guarded). The richest bespoke binary parser in the tree.
- `disk_image::mish::…::parse(&[u8])` — `disk_image/mish.rs` — BLKX chunk table; declared
  count clamped to available, fixed-offset `read_u32`/`read_u64` with no internal bounds
  check (relies on the caller clamp — exactly what a fuzzer should probe).
- `disk_image::iso_pvd::parse` / `parse_root_extents(&[u8])` — `disk_image/iso_pvd.rs` —
  ISO9660 PVD both-endian reads, datetime/ascii-uint fields, returns extents later used
  to seek.
- `disk_image::dmg_trailer::parse(&[u8])` and `mbr::parse(&[u8])` (the latter is
  `pub(super)`) — fixed-offset koly trailer / partition-table reads.
- `eps::dos_eps::parse(&[u8])` — `eps/dos_eps.rs` — 30-byte DOS-EPS header, section
  offset/len clamping; result is sliced downstream.
- `structured::info::json5_nesting_too_deep(&str)` **paired with**
  `structured::pretty::pretty_print(&str, fmt)` — `structured/{info.rs,pretty.rs}` — the
  hand-written depth-bomb guard that fronts `json5` (whose recursive descent has no depth
  limit). A gap here re-exposes an **uncatchable SIGABRT** stack overflow, so this pair
  is the single most consequential text target.
- `image::exif::exif_fields_from_bytes(&[u8])` / `image::xmp::xmp_fields_from_bytes(&[u8])`
  — `image/{exif.rs,xmp.rs}` — TIFF/IFD parse + hand-written XMP substring scraper, run
  on head bytes.
- `sql::info_gather::gather(&str)` and `css::info_gather::gather(&str)` — bespoke
  statement/dollar-quote/comment scanners (SQL) and custom cssparser visitor math (CSS).
- `peek_foundation::base64::{decode, decoded_len}(&str)` — differential target: assert
  `decoded_len(s) == decode(s).map(|v| v.len())`; the listing layer trusts `decoded_len`
  as a size.

**Tier 2 — easy wiring** (build `InputSource::memory` or `&str` in 1–2 lines):

- `peek_io::LineSource::open` (no new deps — land first; see above).
- `csv::parse::…::open(InputSource, fmt)` — encoding/UTF-16/delimiter sniff + cell
  classify over a sliding window; drive `window`/`ensure_all`.
- `vobject::line::parse_components(&str)`, `vobject::datetime::format_datetime(&str)`,
  `vobject::{calendar,contact}::summarize(&str)` — vCard/iCal line unfolding, quoted-param
  scans, fixed-range datetime substring slicing, RRULE humanizer. All byte-index string
  math, cheap, high panic-surface.
- `disk_image::iso_listing::list_iso(InputSource)` — recursive directory-tree walk from
  PVD extents; good for offset-loop / record-bounds bugs.
- `classfile::info_gather::gather_extras(InputSource)` — input is just
  `InputSource::memory`, but the value is in peek's own `descriptor`/`bytecode`
  formatters running over attacker-controlled (cafebabe-parsed) descriptors.

**Tier 3 — harder / container** (need a valid-enough container or a `Read`/`ReadSeek` wrapper):

- `archive::backends::cpio` header walk (`list_from_read` / `find_entry` /
  `next_header`) — bespoke newc/odc/old-binary header parsers: hex/octal field decode,
  4-byte alignment, name-length caps. Classic header-confusion + integer-length surface.
- `archive::backends::ar::ArReader` (`next` / `list`) — 60-byte ASCII header, decimal/
  octal decode, BSD `#1/<len>` long-name length; `parse_static_lib` sits on top.

**Suggested order:** (1) add the deps + land the no-dep `LineSource` target; (2) the
Tier-1 binary `&[u8]` parsers; (3) the `json5_nesting_too_deep` + `pretty_print` pair;
(4) the vobject/sql/csv text targets; (5) Tier 3 archive headers if time allows. Seed
corpora from the existing fixtures under `test-data/` for each type.

### Memory / Streaming ◐

North star #2 from CLAUDE.md: *stream, don't load*. Sites where view-mode caches grow
unboundedly with scroll, or whole-file slurps lack a cap. Audit snapshot (2026-05-17)
in [archived/memory-audit-2026-05.md](archived/memory-audit-2026-05.md) — file paths
in that snapshot have drifted; treat its categorization as the source-of-truth shape,
the specific file:line citations as starting points to re-find. The scroll-cache /
whole-file-slurp leaks it flagged are closed; one remains:

- ◐ **Stdin slurp** — documented (`stdin.rs` module doc) as an inherent unbounded read:
  a pipe is non-seekable, so the whole stream materialises before viewing. Spill-to-
  tempfile past a threshold (mirroring the archive-extract / decompress spill paths)
  stays a future option if huge-stdin pressure ever shows up; for now, route huge inputs
  through a file path.

- ◐ **ODS sparse-dimension allocation (upstream-bounded).** A spreadsheet sheet
  materialises into calamine's *dense* `Range` — a `width × height` buffer over the
  bounding rectangle of populated cells. Two cells at opposite corners force a
  full-grid allocation from a tiny file. **XLSX is fully guarded**
  (`spreadsheet/workbook.rs` `bounded_xlsx_range` streams cells via
  `worksheet_cells_reader` and bails before the dense build once the box crosses
  `MAX_SHEET_CELLS` = 16M). **ODS is only partly guarded:** calamine 0.35 has no
  streaming reader for ODS (`worksheet_range_ref` is `unimplemented!()`), so the dense
  `vec![Data::default(); len]` runs *inside* `worksheet_range` before peek sees the
  `Range`. The post-read `get_size()` cap in `materialize` catches every ODS bomb
  calamine can allocate without aborting (e.g. ~1M×100 ≈ 2.4 GB → rejected before
  peek mirrors it); the extreme case (ODS caps at 1M×16K ≈ 16e9 cells ≈ ~400 GB)
  aborts the process inside calamine — a clean-ish DoS, but an alloc-failure abort skips
  `Drop`, so `TerminalGuard` would not restore the terminal. Found in the 2026-06-21
  security review (finding H2). Options, in order of preference:
  - **Upstream a max-cell cap into calamine's ODS reader** — cleanest, helps everyone.
  - **Port an ODS extent pre-scan into peek** — ~50–80 lines, ODS-only. Awkward because
    the extent is implicit in `number-rows-repeated`/`number-columns-repeated` and
    calamine *trims trailing empty repeats*; a naive repeat-sum over-counts the padded
    grids real ODS files carry and would reject legitimate files. Correctness requires
    mirroring calamine's trimming.
  - **Accept it** — moderate ODS bombs already rejected; the extreme one is a crash.

### Install / supply-chain hardening ☐

From the 2026-06-21 security review. The download paths are otherwise well-built — all
HTTPS with cert validation, `install.sh` verifies a SHA-256 before running, `gs` is
invoked via an arg vector under `-dSAFER`, no shell interpolation, secure `tempfile`
temps. The residual gaps all reduce to *"trust GitHub Releases of two repos"* — closing
them means adding cryptographic provenance, not fixing a leak.

- ☐ **Sign releases.** `install.sh` verifies a `.sha256`, but the checksum ships from
  the *same* release as the artifact — it catches transit corruption, not a malicious or
  compromised release. Anyone with release-publish rights (or a GitHub account takeover)
  can swap both binary and checksum together. Add a minisign / cosign signature, embed
  the public key in `install.sh` and the binary, and verify the signature rather than a
  self-consistent hash. *(Review ID: M1.)*

- ☐ **Verify the Pdfium fetch.** `scripts/fetch-pdfium.sh` and `release.yml` download the
  dylib from `bblanchon/pdfium-binaries` over HTTPS with **no** checksum or signature
  check (`.pdfium/VERSION` is display-only, not an integrity control). A compromise there
  trojans every peek release. Pin and verify a known-good SHA-256 of each Pdfium asset.
  Weakest link in the chain. *(Review ID: M3.)*

- ☐ **Verify the Pdfium dylib at load (or refuse a world-writable dir).** peek loads
  Pdfium from the executable's own directory first. peek's own lookup isn't
  cwd-hijackable (full-path bind, no `$LD_LIBRARY_PATH` use), but a dylib planted next to
  the binary in a writable install dir loads with no integrity check at load time.
  Consider a checksum at load, or refusing to load from a world-writable directory.
  *(Review ID: M2.)*

- ☐ **Pin `install.sh` to the release tag.** `--update` fetches `install.sh` from the
  mutable `main` branch while installing the binary from a resolved release tag — script
  and binary come from decoupled refs, and a bad `main` script reaches every existing
  user running `--update`. Fetch the script from the same immutable tag. *(Review ID:
  L1.)*

Same trust root throughout (repo release-write access), so none introduces a *new*
attacker class — but signing (M1) + pinned Pdfium (M3) move provenance from "trust the
platform" to "verify cryptographically," and are the meaningful follow-ups.

- ☐ **Drop the `cargo audit` ignores when upstream catches up.** `just audit` is clean
  except 3 *unmaintained* advisories (no known vulnerability), all transitive and
  upstream-owned, suppressed in `.cargo/audit.toml`: `bincode` + `yaml-rust` via
  `syntect`, `paste` (build-time proc-macro) via `image`→`ravif`→`rav1e`. Re-check on
  each dep bump; remove an ignore once the chain stops pulling it in.

---

## Post-1.0 / 1.x

### Text Search

The shipped scope (literal + regex, smart-case, `n`/`p`, cross-view reach, budgeted scan) is the
accepted 1.0 feature. Refinements, none blocking:

- **Incremental search** — re-scan + re-highlight on every keystroke instead of
  confirm-on-Enter.
- **Wider reach** — file-info view and the hex dump. Those don't participate yet.
- **Lazy / bounded scan** — the current scan is one full pass over the active view,
  capped at 100,000 matches; a multi-GB file pays that pass up front. A lazy "search
  from here" would scale better.

### Styled-text IR (structured spans) ❓

Replace the in-band-ANSI pipeline with a typed span tree — `Line(Vec<Span>)`, `Span { text,
style }` (think HTML spans, or ratatui's `Span`/`Line`/`Style`). Content is **always** plain,
sanitized text; SGR escapes get generated once, at the leaf, when `peek_theme` flushes a line to
the terminal under the active `StyleMode`.

**Why it earns its place:**

- **Kills the escape-leak bug class by construction.** Today style is carried as in-band escapes
  in `String`s, and `render_themed_status_line` / listings / prompt sanitize segment *text* before
  paint — so any caller that pre-embeds SGR gets it neutralized into visible `␛[…m` (the
  2026-06-22 status-bar warning-marker bug). With a span IR, text and style never share a string
  until render, so this can't happen. The current `(String, Color)` segment type can't enforce
  this; an `escapes-can't-exist-here` newtype is the smaller version of the same idea.
- **Removes the structure → escapes → re-parse round-trip.** syntect already hands us
  `Vec<(Style, &str)>` spans; we flatten to escapes early, then `viewer/ui/styled.rs` (the whole
  "SGR-aware string family") re-derives structure via `scan`/`Sgr`/`ActiveStyle` to count width,
  wrap, slice, and truncate — re-arming styles across every cut. Operating on spans makes
  width/wrap/slice trivial and drops the re-parse machinery.
- **Centralizes style-mode degradation.** truecolor→256→16→plain mapping lives in one leaf
  renderer instead of being baked into strings at construction time.
- **Unblocks folding.** [Block Collapsing](#block-collapsing--folding-) already wants a line
  metadata layer "replacing bare `String` lines" — a `Line` type is where fold level / block
  boundaries / visibility naturally hang.

**Cost / scope:** large, cross-cutting — every mode's render produces spans, `styled.rs` and
`ScreenBuffer` (`prev_lines: Vec<String>`) get rebuilt against the IR. Keep an escape-hatch
variant (`Raw`) for the half-block image renderer, which emits dense per-cell SGR and shouldn't be
forced through. Viewport-bounded rendering means per-line span allocation is cheap (only visible
lines), and dropping the repeated escape re-parse on every scroll/wrap is likely a net win.

Prerequisite-shaped: do this before, or together with, folding rather than after.

### Block Collapsing / Folding ❓

Collapse blocks (objects, arrays, nested structures) in the interactive viewer. Mainly
for structured data (JSON, YAML, TOML, XML) but could extend to code (folding
functions, blocks). The natural 1.1 headline capability.

**Challenges:** the current pipeline produces a flat `Vec<String>` of ANSI-escaped
lines with no structural metadata. Folding would require:

- Line metadata layer (fold level, block boundaries, visibility state) replacing bare
  `String` lines — see [§ Styled-text IR](#styled-text-ir-structured-spans-), the natural
  home for this
- Virtual line mapping so scroll offsets work with collapsed regions
- Preserving fold state across re-renders (theme toggle, raw/pretty toggle)
- For structured data: retaining parsed structure or using indentation heuristics
- For code: language-aware block detection via syntect scopes (significantly harder,
  language-dependent)

Indentation-based folding for structured data (JSON/YAML) would be the most practical
starting point — pretty-printed output has reliable indentation levels.

Also unblocks INI / properties section-folding — see
[§ Config Files](#config-files-).

## File Types

Deepening of already-shipped types (none block 1.0; pick by demand).

### Presentations ◐

The `presentation` type ships (pptx/pptm/ppsx + odp slide-by-slide text, Keynote
preview). Remaining deepening:

- **`.ppt`** (legacy OLE/CFB compound binary) — deferred with the other OLE formats.
- **ODP run styling** — ODP slide runs render plain; bold/italic/colour need the
  `automatic-styles` resolution the `document` ODT parser already does.
- **Keynote slide text** — modern `.key` stores text as undocumented snappy-protobuf
  (IWA). Today only the embedded `preview.jpg` + build metadata surface. Full IWA
  decode is a large, version-fragile reverse-engineering effort — its own task.

### SQL ◐

- Pretty-print / formatter, statement-outline aux mode, distinct PL/pgSQL grammar
  dispatch inside `$$ … $$` bodies.
- Outline aux mode shared between Markdown headings and SQL statements (mode + key
  binding TBD).

### CSS ◐

#### Selector specificity inline-annotation

Annotate each rule's selector list with its specificity tuple (`a,b,c`) in the
highlighted CSS source view. The "killer feature" for debugging "why isn't my style
winning". Biggest cost is the `ContentMode` integration: a separate code path (gutter
or trailing-comment rendering) and a parsed-selector cache plumbed through
`RenderCtx`. Specificity itself is cheap to compute.

#### svg_anim keyframe-parser rewrite

Replace the hand-rolled `crates/peek-types/src/types/image/pipeline/svg_anim` CSS
parsing (`keyframes.rs` / `spec.rs` + transform/selector parsing across `selectors.rs`
/ `scan.rs`, ~600 LOC) with a typed parser. Separate concern with real SVG-animation
regression risk, and the one piece that would actually justify `lightningcss`'s typed
`Transform` / `Animation` values over the current `cssparser` + `cssparser-color` pair
(picked for the Info view because it's ~120–200 KB vs ~400–700 KB and covers
everything the Info view needs). Revisit as its own task; if picked up, weigh swapping
the CSS dep then.

The separate SVG-animation **render perf** queue (measurements + proposed optimizations,
not a shipped-behaviour description) lives in [svg-anim-perf.md](svg-anim-perf.md). Folded
to this roadmap as a future-work pointer; not active right now.

### PDFium Distribution ◐

- **install.sh**: detect an already-installed system Pdfium (homebrew etc.) and skip
  the bundled copy when present. (Version pinning per release is already handled — the
  workflow reads the bundled `BUILD` from `.pdfium/VERSION`.)
- **`cargo install`**: build-time path search via `PDFIUM_DYNAMIC_LIB_PATH` only finds
  the lib if the user has set it; document install steps in the README.
- **Feature flag**: optional Cargo feature `pdf` so a no-PDF build keeps binary size
  down for embedded targets.

### Vector / PostScript Files ◐

- **Legacy AI (pre-CS2)** — pure PostScript, no PDF wrapper. Routes through the
  EPS/PostScript path today (source + DSC info + `gs` render when available), but isn't
  specifically detected or labelled as Illustrator. Low priority — such files are rare
  now.
- **Multi-page `.ps`** — the Ghostscript render view shows page 1 only. A paged render
  (gs page-count probe + per-page rasterise, `n` / `p` to step) would cover multi-page
  PostScript documents.
- **WMF previews** — DOS-EPS files can carry a Windows Metafile preview instead of
  TIFF. The Info view names it but there's no pure-Rust WMF rasteriser, so it's not
  rendered. `gs` covers these files anyway when present.
- **EPSI inline previews** — the rare hex-ASCII preview block in plain `%!PS` EPS isn't
  parsed (only binary DOS-EPS TIFF previews are). `gs` covers these too.

### Archive Files ◐

| Format | Extensions | Status |
|--------|------------|--------|
| RAR    | `.rar`     | ☐      |

RAR is the awkward one — closed format, library wrap via `unrar` (wraps the
proprietary unrar C lib). License caveats: listing only is fine, but distribution adds
friction, so defer behind a Cargo feature flag (`rar`), off by default.

- **RAR listing** — table-of-contents view through the existing `ByteSource` /
  `ListingMode` path.
- **RAR extract** — once RAR listing lands, extract reuses the unrar wrapper; same
  listing-only caveats apply.

### Disk Images ◐

- **ISO Rock Ridge detection** — needs a SUSP scan inside the root directory record;
  one extra read pass. Would surface real Unix permissions in the perms column.
- **DMG nested filesystem metadata** — HFS+ / APFS volume names inside the partition
  payload. The partition map (names / types / sizes / compression) already shows — see
  [features.md → Disk Images](features.md#disk-images-); this is the next layer down,
  walking the reconstructed partition's filesystem. Significant work; probably never
  worth it for peek.
- **DMG entry extract** — currently returns `Unsupported`. Needs UDIF block
  decompression (zlib / bzip2 / lzfse chunks) before any meaningful filesystem walk
  could expose individual files. Significant work, deferred indefinitely.

UDF (DVD / Blu-ray ISOs) deferred — more complex format, niche use case for peek.

### Config Files ◐

- **Structured pretty-print + section folding** for INI / properties via native
  parsers (`rust-ini`, `java-properties`) — would need the folding infrastructure (see
  [Block Collapsing](#block-collapsing--folding-)), so deferred with it.
- **HCL / Dhall / CUE** beyond highlighting — typed parsing only if those ecosystems
  mature.

### Email ◐

- **Inline images** rendered in the body (cid: references resolved against inline
  parts).

### Audio Files ☐

- **Audiobook chapters** for `.m4b` containers — MP4 chapter atoms / `chpl` boxes drive
  a `Next` / `Prev` flow like EPUB. Defer until a real m4b ships up.
- **Multi-picture Cover tab.** Today only the primary (FrontCover or first) visual gets
  the Cover tab; back / artist / leaflet pictures only live in the Embeds listing.
  Could cycle through all visuals in one Cover view with `n` / `p`.
- **Synced lyrics timeline.** `SYLT` ID3v2 frames carry per-line timestamps; currently
  flattened to plain text. A timeline view that highlights the current line during
  (future) playback would be the next step.
- **ASCII waveform or spectrum preview.** Decoding adds cost — decide later. Tags alone
  are cheap and useful.

### Font Files ◐

Bare sfnt (`.ttf` / `.otf` / `.ttc` / `.otc`) and both web wrappers (`.woff` /
`.woff2`) ship. Stretch:

- Multi-script sample sentences keyed on cmap coverage (Cyrillic / Greek / Arabic /
  CJK samplers when the font carries the glyphs). The current sampler is a hard-coded
  ASCII pangram.
- True recursive peek into a single face of a `.ttc` collection — would need to
  synthesise a standalone SFNT from the TTC table directory (copy referenced tables,
  recompute offsets and checksums). The current shape — face-cycle keys (`n` / `p`) on
  the SpecimenMode and every face's metadata in Info — covers the visible use case at a
  fraction of the cost.

### Certificates and Keys — PKCS#12 ☐

X.509 DER (`.der`, and DER-encoded `.crt` / `.cer`) and JWK / JWKS (`.jwk` / `.jwks`)
ship — see [features.md → Certificates and Keys](features.md#certificates-and-keys-).

| Format        | Extensions     | Notes                                                                                                                                                                                                 |
|---------------|----------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| PKCS#12 / PFX | `.p12`, `.pfx` | Encrypted bag. First cut: show bag types + embedded cert/key labels without prompting for the password. A password prompt is a separate UX surface (interactive only; pipe mode would skip the parse) |

Crates: `pkcs12` (encrypted bags).

### Object Files — deeper inspection ☐

- **Notes / build metadata** — compiler / toolchain hints, code-signature presence.
  (Build ID / Mach-O UUID / PE PDB GUID already surface in Info.)
- **Mach-O fat slices** — switch the viewed slice interactively. Today the host-arch
  slice is auto-picked and the rest only listed in the Info view.

---

## Open ideas / deferred indefinitely

### Video Files ❓

Render video as ASCII art in real-time — decode frames and run through the image
pipeline. Stretch goal; may not be practical due to decode performance and terminal
refresh-rate limits. Would need an ffmpeg binding.

In print mode: file metadata (duration, resolution, codec, bitrate), possibly a single
frame.

### Image-decode gap ❓

Some common raster formats are *detected* (via `infer`) and routed to `FileType::Image`
but can't be decoded by the `image` 0.25 crate, so the render fails:

- **HEIC / HEIF** — iPhone's default photo format. No decoder in `image`; needs
  `libheif-rs` (C lib dep, distribution friction like Pdfium) or a pure-Rust decoder
  when one matures.
- **PSD** (Photoshop) — would need the `psd` crate (flattened composite only).
- **JPEG-XL** — needs `jxl-oxide` (pure Rust) or `libjxl`.
- **Camera RAW** (DNG / CR2 / NEF / ARW) — partial; some decodable, embedded JPEG
  preview is the pragmatic path.

Each adds a decode dependency for a single format. Deferred until one is worth the
weight — HEIC is the strongest candidate given how common it now is. Until then these
should at least fail with a clear "format detected, no decoder" message rather than a
generic render error.

### Legacy & Apple binary formats ❓

- **Legacy OLE / CFB office** (`.doc` / `.xls` / `.ppt`, pre-2007) — magic
  `D0CF11E0`. The Microsoft Compound File Binary container; today these fall through to
  the binary view. A listing of the OLE storage/stream tree (via the `cfb` crate) would
  at least expose structure; full text extraction is much more work. Rare and shrinking,
  so low priority.
- **Binary plist** (`bplist00`) — macOS-ubiquitous (prefs, provisioning profiles).
  Currently the `plist` extension routes to the XML parser, so *binary* plists
  mis-parse and fail (XML plists render fine). A magic sniff (`bplist00`) + the `plist`
  crate to re-emit as a structured tree would fix it. Arguably a mis-route bug more than
  a missing feature; small.

### Parquet ❓

Columnar data-engineering standard; today routes to the binary view. Fits the
info-screen model well *without* decoding all the data: the footer carries schema, row
groups, row/column counts, compression codec, and per-column statistics. First cut =
metadata-only Info section (via the `parquet` crate's file metadata reader, no full
column scan); a streaming row-preview table would be a later layer. Niche but
increasingly common; demand-driven.

### Type-support plugin trait ❓

Follow-up to the types-colocation refactor — see
[archived/refactor-types-colocation-plan.md](archived/refactor-types-colocation-plan.md)
for the underlying restructuring and the rejected-trait-dispatch rationale.

Each file type already owns its detection module (`crates/peek-detect/src/types/<x>.rs`)
and its reader modules (`info.rs`, `compose.rs`). The **info-render axis already
collapsed** into trait dispatch (2026-06-05): `info::render` calls
`InfoExtras::render_section` with per-type impls in `types/info_impls.rs` — so this
parked item is now narrower, covering the **compose + detection** axes only. The
remaining central dispatch sites (`Registry::compose_modes` match, the `peek-detect`
orchestrator's per-type calls) could similarly collapse into trait-dispatch loops:

```rust
trait TypeSupport {
    fn matches(&self, detected: &Detected) -> bool;
    fn compose(&self, ctx: &ComposeCtx, modes: &mut Vec<Box<dyn Mode>>) -> Result<()>;
    fn detect_by_extension(&self, ext: &str) -> Option<FileType>;
    fn detect_by_magic(&self, head: &[u8]) -> Option<FileType>;
    // info rendering already lives on the `InfoExtras` trait (see info_impls.rs).
}

fn all_types() -> Vec<Box<dyn TypeSupport>> { /* one entry per type */ }
```

Adding a new type becomes one new directory plus one line in `all_types()`.

**Trade-off:** loses the single-file dispatch overview. Today, opening `src/compose.rs`
shows every file type's compose strategy at a glance; with trait dispatch, the reader
follows a `Vec` to an implementation. IDEs handle the jump fine, but losing the "scan
the whole match in one screen" property is real.

**Recommendation:** revisit only if the number of file types grows past the point where
the central matches stop fitting on one screen, or if external plugins (loading a
`TypeSupport` from a dynamic library) become a goal. Until then, the hard-coded matches
established by the colocation refactor are easier to read and modify.

### Unified Timestamp type ❓

Domain-data dates are *partly* unified. Precise instants now share one representation —
`SystemTime` wrapped by `Value::Timestamp`, serialized ISO-8601 UTC, rendered muted
(landed 2026-06-18: PDF, document, spreadsheet, presentation, email; the parse +
`days_from_civil` primitives live in `peek-foundation` `info/time.rs`). Several sites
still store dates their own way:

| site | stored as |
|---|---|
| **cert** (`not_before` / `not_after` / `this_update` / `next_update`) | pre-formatted ISO-8601 `String` — has the epoch (`ASN1Time::timestamp()`), formats early; left as-is because its JSON is already correct and a typed render would regress its print from absolute UTC to viewer-local |
| **disk_image** `IsoDateTime` | decomposed struct (y/m/d/h/m/s + offset), never assembled into an instant |
| **iCalendar** `date_range` | `(String, String)` of `YYYY-MM-DD` — date-only, and a *range* |
| **audio / ebook** `date` | freeform `String`, often year-only (`2011`) |
| **EPS** `creation_date`, **image** EXIF/XMP | freeform `String`, may not be a date |

The consolidation worth doing is **not** a `SystemTime` vs UTC enum — those carry the
same information (a `SystemTime` *is* an absolute instant = a UTC civil time), so the
split is meaningless. The variation that actually exists is **granularity**:

```rust
enum Timestamp {
    Instant(SystemTime),            // precise: cert, iso, the converted five
    Date { y: i32, m: u8, d: u8 },  // date-only: iCalendar, audio year
    Raw(String),                    // un-parseable original, preserved verbatim: EXIF, EPS
}
```

It would `Value::Timestamp(Timestamp)` — one `Serialize` (instant → `…Z`, date →
`YYYY-MM-DD`, raw → string), one `InfoValue` render, one parse entry point — so the
messy-case policy lives in one place instead of scattered per-type String formatting.

**Pays off** at cert (String → `Instant`, kills its duplicate `gregorian`), iCalendar
(`Date` ×2), disk_image (`IsoDateTime` → `Instant`). **Stays `Raw`**: EXIF, EPS — no
false precision. **Judgment call**: audio/ebook year-only as `Date` vs `Raw`.

**Recommendation:** earns its place once 3–4 sites adopt it; if only cert ever moves, a
plain `SystemTime` field is simpler. Demand-driven cleanup, not a blocker.
