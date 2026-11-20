# peek: planned features and ideas

Status legend: ☐ planned · ❓ idea or open question

For implemented (✅) and partial (◐) features, see [features.md](features.md).

This doc is ordered as a **release plan**. It starts with the nearest-term hardening, then the 1.0
bar, then the post-1.0 deepening, and ends with the graveyard of open ideas. The roadmap below is
the index. The detailed notes for each feature follow under the same milestone headings.

## Roadmap

Feature _breadth_ is already at the 1.0 level. The gap is robustness against the project's own two
north stars: _stream, don't load_ and _multi-GB first-class_. The plan closes the remaining
streaming leaks first and then deepens the existing features.

### Release 0.4: hardening to make the tagline true ☐

The "stream, don't load" and "multi-GB first-class" promises still have leaks. These items aren't
features. They are places where the marketing claim doesn't fully hold yet.

- **Memory and streaming audit** ◐: only the stdin slurp remains. It's documented, and the spill to
  disk is deferred. See [§ Memory / streaming](#memory--streaming-).
- **Large file safeguards** ◐: one optional idea, a generalized info-default landing, is still open.
  See [§ Large file safeguards](#large-file-safeguards-).
- **Install and supply chain hardening** ☐: from the 2026-06-21 security review. Releases have no
  signatures, the Pdfium fetch isn't verified, and `install.sh` is pulled from `main`. See
  [§ Install and supply chain hardening](#install-and-supply-chain-hardening-).
- **Parser fuzzing backlog** ☐: fuzzing reaches only `detect()` today. Coverage-guided fuzzing
  doesn't test the binary and text parsers of each type. See
  [§ Parser fuzzing backlog](#parser-fuzzing-backlog-).

### Release 1.0: the last user-facing must-haves ✅

No blockers remain. Feature breadth, the streaming guards, and robustness are at the 1.0 bar. The
shipped Text Search scope is accepted as the 1.0 feature. Its further refinements moved to the
post-1.0 deepening below.

### After 1.0: deepening in 1.x ☐

- **Text Search refinements**: incremental re-scan, wider reach (the info view and the hex dump),
  and a lazy, bounded scan. These improve the quality of life of a feature that already shipped. See
  [§ Text Search](#text-search).
- **Block collapsing and folding**: the biggest single new capability and the natural 1.1 headline.
  It's a big lift because it needs a line-metadata layer. See
  [§ Block collapsing and folding](#block-collapsing-and-folding-).
- **Built-in manual follow-ups**: following links between chapters, and the manual as a mode in the
  Tab cycle of every file. See [§ Built-in manual](#built-in-manual-).
- **File-type deepening**: enhancements to types that already ship, such as a SQL formatter, CSS
  specificity, multi-page `.ps`, PKCS#12, RAR, and m4b chapters. None of them blocks 1.0. Pick them
  by demand. See [§ File types](#file-types).

### Open ideas, deferred indefinitely ❓

- **Video to ASCII**: a stretch goal. It needs an ffmpeg dependency and hits a decode-performance
  wall. See [§ Video files](#video-files-).
- **HEIC, PSD, JPEG XL, and camera RAW**: peek detects these formats but doesn't render them. Each
  needs a new decoder dependency that the `image` crate doesn't provide. See
  [§ Image-decode gap](#image-decode-gap-).
- **Legacy OLE office files** (`.doc`, `.xls`, and `.ppt`) and **binary plist**: peek doesn't handle
  the old compound-file and `bplist00` formats today. See
  [§ Legacy and Apple binary formats](#legacy-and-apple-binary-formats-).
- **Parquet**: the columnar standard of data engineering. peek would show the schema and the
  row-group metadata. See [§ Parquet](#parquet-).
- **Type-support plugin trait**: analyzed and parked. Revisit it only if the type count outgrows the
  central matches or if external plugins become a goal. See
  [§ Type-support plugin trait](#type-support-plugin-trait-).
- **DMG nested-filesystem metadata and entry extract**: significant work, and "probably never worth
  it." See [§ Disk images](#disk-images-).
- **Unified `Timestamp` type**: fold the remaining date sites that use a `String`, a decomposed
  struct, or a freeform value onto one granularity enum behind `Value::Timestamp`. It's a cleanup,
  driven by demand. See [§ Unified timestamp type](#unified-timestamp-type-).

---

## Release 0.4: hardening

### Large file safeguards ◐

The "multi-GB first-class" guards shipped. See [features.md](features.md) and the strategy doc
[memory-streaming.md](memory-streaming.md). One optional item remains:

- ◐ **Generalize the info-default landing** beyond decompression and table of contents (TOC) views.
  A `Mode::loads_whole_file()` signal would land _any_ transform view over the threshold on Info.
  Add it only if a view other than decompression ever needs it.

### Detection hardening ◐

The `peek-detect` crate split made these items tractable. See the archived
[crate-split plan](archived/crate-split-plan.md). Detection is now a small surface that has no
readers and that you can fuzz. The "Follow-up backlog" section of that plan holds the full rationale
and the file and line references. One item remains:

- ☐ **[1.x] Tighten loose heuristics.** The YAML `---` prefix matches too much. Binary types routed
  by extension, and `.br`, aren't verified by magic bytes.

### Parser fuzzing backlog ☐

Today the only fuzz coverage is for `detect()`. It consists of `fuzz/fuzz_targets/detect_bytes.rs`
(libFuzzer) and the stable proptest floor `crates/peek-detect/tests/fuzz_detect.rs`. Coverage-guided
fuzzing doesn't test any of the **per-type** parsers in `crates/peek-types`. They have unit tests,
and `ds_store` has tests with hostile input, but they have no fuzzer. These parsers handle the most
attacker-controlled binary structure in the project. That makes fuzzing them the highest-value
defense-in-depth follow-up from the 2026-06-21 security review (finding L4). The parser layer came
out of the review _clean_, and the hand review found no reachable panic. The goal is to keep it that
way as the parsers change, not to chase a known bug.

#### Infrastructure blocker

Do this first. The fuzz crate is excluded from the workspace because `libfuzzer-sys` needs nightly,
and it depends **only on `peek-detect` and `peek-io`**. To fuzz any `peek-types` parser, first add
`peek-types = { path = "../crates/peek-types" }` to `fuzz/Cargo.toml`. Also add `peek-foundation`
for the base64 and XML helpers. After that, a new target is a `[[bin]]` block and a one-line
`fuzz_target!(|data: &[u8]| { … })`. You run it with `mise run fuzz <name> <n>`. There is **no**
`#[cfg(fuzzing)]` infrastructure today, and most candidates below are already `pub`, so the targets
need no harness plumbing. The exceptions are the few `pub(super)` functions, such as `mbr::parse`.
To expose those, make them `pub` or add a feature like `testing`.

One target needs **no new dependencies** and can land right away: `peek_io::LineSource`. Drive
`open` and `window` over `InputSource::memory(data, "f")`. The target exercises the anchor-index
scan and the UTF-8 and terminal-sanitize decode in `crates/peek-io/src/lines.rs`.

#### Tier 1: trivial wiring, high value

These parsers are hand-written and take `&[u8]` or `&str` directly. Each target is one line once
`peek-types` is a dependency.

- `ds_store::reader::parse(&[u8])` in `ds_store/reader.rs`: the Bud1 buddy allocator. It covers the
  offset table, block-address math, UTF-16 name lengths, and a recursive B-tree walk, which already
  has depth and visited guards. It's the richest bespoke binary parser in the tree.
- `disk_image::mish::…::parse(&[u8])` in `disk_image/mish.rs`: the BLKX chunk table. The declared
  count is clamped to what's available. The fixed-offset `read_u32` and `read_u64` calls have no
  internal bounds check and rely on the caller's clamp, which is exactly what a fuzzer should probe.
- `disk_image::iso_pvd::parse` and `parse_root_extents(&[u8])` in `disk_image/iso_pvd.rs`: reads of
  both-endian fields, date-time fields, and ASCII integer fields in the ISO 9660 Primary Volume
  Descriptor (PVD). It returns extents that later code uses to seek.
- `disk_image::dmg_trailer::parse(&[u8])` and `mbr::parse(&[u8])`, where the latter is `pub(super)`:
  fixed-offset reads of the koly trailer and the partition table.
- `eps::dos_eps::parse(&[u8])` in `eps/dos_eps.rs`: the 30-byte DOS EPS header. It clamps section
  offsets and lengths. Downstream code slices the result.
- `structured::info::json5_nesting_too_deep(&str)` **paired with**
  `structured::pretty::pretty_print(&str, fmt)` in `structured/{info.rs,pretty.rs}`: the
  hand-written depth-bomb guard in front of `json5`, whose recursive descent has no depth limit. A
  gap here exposes again a SIGABRT stack overflow that **can't be caught**. That makes this pair the
  single most consequential text target.
- `image::exif::exif_fields_from_bytes(&[u8])` and `image::xmp::xmp_fields_from_bytes(&[u8])` in
  `image/{exif.rs,xmp.rs}`: the TIFF and IFD parse and a hand-written XMP substring scraper. Both
  run on the head bytes.
- `sql::info_gather::gather(&str)` and `css::info_gather::gather(&str)`: bespoke scanners for
  statements, dollar quotes, and comments in SQL, and custom `cssparser` visitor math in CSS.
- `peek_foundation::base64::{decode, decoded_len}(&str)`: a differential target. Assert
  `decoded_len(s) == decode(s).map(|v| v.len())`, because the listing layer trusts `decoded_len` as
  a size.

#### Tier 2: easy wiring

These targets build an `InputSource::memory` or a `&str` in one or two lines.

- `peek_io::LineSource::open`. It needs no new dependencies, so land it first, as described earlier.
- `csv::parse::…::open(InputSource, fmt)`: encoding, UTF-16, and delimiter sniffing, plus cell
  classification over a sliding window. Drive `window` and `ensure_all`.
- `vobject::line::parse_components(&str)`, `vobject::datetime::format_datetime(&str)`, and
  `vobject::{calendar,contact}::summarize(&str)`: vCard and iCalendar line unfolding, scans of
  quoted parameters, fixed-range date-time substring slicing, and the RRULE humanizer. All of it is
  byte-index string math, which is cheap to fuzz and has a large panic surface.
- `disk_image::iso_listing::list_iso(InputSource)`: a recursive walk of the directory tree from the
  PVD extents. It's a good place to find bugs in offset loops and record bounds.
- `classfile::info_gather::gather_extras(InputSource)`: the input is only an `InputSource::memory`.
  The value is in peek's own `descriptor` and `bytecode` formatters, which run over
  attacker-controlled descriptors that `cafebabe` parsed.

#### Tier 3: harder, container formats

These targets need a container that is valid enough, or a `Read` or `ReadSeek` wrapper.

- The header walk in `archive::backends::cpio` (`list_from_read`, `find_entry`, and `next_header`):
  bespoke parsers for the `newc`, `odc`, and old binary headers. They decode hex and octal fields,
  apply 4-byte alignment, and cap name lengths. It's a classic surface for header confusion and
  integer-length bugs.
- `archive::backends::ar::ArReader` (`next` and `list`): the 60-byte ASCII header, decoding of
  decimal and octal fields, and the BSD `#1/<len>` long-name length. `parse_static_lib` sits on top
  of it.

#### Suggested order

1. Add the dependencies and land the `LineSource` target, which needs none.
2. Add the Tier 1 binary `&[u8]` parsers.
3. Add the `json5_nesting_too_deep` and `pretty_print` pair.
4. Add the `vobject`, `sql`, and `csv` text targets.
5. If time allows, add the Tier 3 archive headers.

Seed the corpus for each type from the existing fixtures under `test-data/`.

### Memory / streaming ◐

North star #2 in CLAUDE.md is _stream, don't load_. This section tracks the sites where caches in
view modes grow without bound as you scroll, and whole-file slurps that have no cap. The audit
snapshot of 2026-05-17 is in [archived/memory-audit-2026-05.md](archived/memory-audit-2026-05.md).
The file paths in that snapshot have drifted. Treat its categorization as the source of truth for
the shape of the problem, and its `file:line` citations as starting points to find the code again.
The scroll-cache and whole-file-slurp leaks it flagged are closed. These items remain:

- ◐ **Stdin slurp.** The module doc of `stdin.rs` documents it as an inherent unbounded read. A pipe
  isn't seekable, so the whole stream is read into memory before viewing starts. A spill to a temp
  file past a threshold stays a future option if pressure from huge stdin input ever shows up. It
  would mirror the spill paths of archive extract and decompression. For now, pass huge inputs as a
  path.

- ◐ **ODS sparse-dimension allocation (bounded upstream).** A spreadsheet sheet is read into
  calamine's _dense_ `Range`. That's a `width × height` buffer over the bounding rectangle of the
  populated cells, so two cells at opposite corners force a full-grid allocation from a tiny file.
  - **XLSX is fully guarded.** `bounded_xlsx_range` in `spreadsheet/workbook.rs` streams cells with
    `worksheet_cells_reader` and stops before the dense build once the box crosses
    `MAX_SHEET_CELLS`, which is 16M.
  - **ODS is only partly guarded.** calamine 0.35 has no streaming reader for ODS, and its
    `worksheet_range_ref` is `unimplemented!()`. So the dense `vec![Data::default(); len]` runs
    _inside_ `worksheet_range` before peek sees the `Range`. The `get_size()` cap that `materialize`
    applies after the read catches every ODS bomb that calamine can allocate without aborting. For
    example, about 1M×100 cells is about 2.4 GB, which peek rejects before mirroring it. ODS caps at
    1M×16K cells, which is about 16e9 cells or about 400 GB. That extreme case aborts the process
    inside calamine. It's a fairly clean denial of service, but the crash on allocation failure
    skips `Drop`, so `TerminalGuard` doesn't restore the terminal.

  The 2026-06-21 security review found this (finding H2). The options, in order of preference:
  1. **Upstream a max-cell cap into calamine's ODS reader.** This is the cleanest option and helps
     everyone.
  2. **Port an ODS extent pre-scan into peek.** It's about 50–80 lines and affects only ODS. It's
     awkward because the extent is implicit in `number-rows-repeated` and `number-columns-repeated`,
     and calamine _trims trailing empty repeats_. A naive sum of the repeats over-counts the padded
     grids that real ODS files carry, and it would reject legitimate files. To be correct, the
     pre-scan must mirror calamine's trimming.
  3. **Accept it.** peek already rejects moderate ODS bombs. The extreme one is a crash.

### Install and supply chain hardening ☐

These items come from the 2026-06-21 security review. Otherwise, the download paths are well built:

- They all use HTTPS with certificate validation.
- `install.sh` verifies a SHA-256 checksum before it runs the binary.
- peek invokes `gs` with an argument vector under `-dSAFER`, with no shell interpolation.
- Temp files go through the secure `tempfile` crate.

The remaining gaps all come down to _"trust the GitHub Releases of two repos."_ Closing them means
adding cryptographic provenance, not fixing a leak.

- ☐ **Sign releases.** `install.sh` verifies a `.sha256` file, but the checksum ships in the _same_
  release as the artifact. It catches corruption in transit. It doesn't catch a malicious or
  compromised release. Anyone with the right to publish releases, or anyone who takes over the
  GitHub account, can swap both the binary and the checksum together. Add a minisign or cosign
  signature, embed the public key in `install.sh` and in the binary, and verify the signature
  instead of a hash that only agrees with itself. _(Review finding M1.)_

- ☐ **Verify the Pdfium fetch.** `scripts/fetch-pdfium.sh` and `release.yml` download the dylib from
  `bblanchon/pdfium-binaries` over HTTPS with **no** checksum or signature check. `.pdfium/VERSION`
  is for display only and isn't an integrity control. A compromise there puts malicious code into
  every peek release. Pin and verify a known-good SHA-256 checksum of each Pdfium asset. This is the
  weakest link in the chain. _(Review finding M3.)_

- ☐ **Verify the Pdfium dylib at load, or refuse a world-writable directory.** peek loads Pdfium
  from the directory of its own executable first. An attacker can't hijack peek's own lookup through
  the current directory, because peek binds by full path and doesn't use `$LD_LIBRARY_PATH`. But if
  someone plants a dylib next to the binary in a writable install directory, it loads with no
  integrity check. Consider a checksum at load time, or refuse to load from a world-writable
  directory. _(Review finding M2.)_

- ☐ **Pin `install.sh` to the release tag.** `--update` fetches `install.sh` from the mutable `main`
  branch, but it installs the binary from a resolved release tag. The script and the binary come
  from decoupled refs, and a bad script on `main` reaches every existing user who runs `--update`.
  Fetch the script from the same immutable tag. _(Review finding L1.)_

The trust root is the same throughout: write access to the repo's releases. So none of these gaps
introduces a _new_ class of attacker. Still, signing (M1) and pinning Pdfium (M3) move provenance
from "trust the platform" to "verify cryptographically," and they're the follow-ups that matter.

- ☐ **Drop the `cargo audit` ignores when upstream catches up.** `mise run audit` is clean except
  for 3 _unmaintained_ advisories that have no known vulnerability. All are transitive and owned
  upstream, and `.cargo/audit.toml` suppresses them:
  - `bincode` and `yaml-rust` come in through `syntect`.
  - `paste`, a build-time proc macro, comes in through `image`→`exr`→`pulp` and
    `avif-decode`→`rav1d`.

  Check them again on each dependency bump. Remove an ignore once the chain stops pulling it in.

- ☐ **Report the rav1d `cdef` overlap, then turn its debug assertions back on.** The dev profile in
  the root `Cargo.toml` turns off `debug-assertions` for `rav1d`. The reason is that its
  `DisjointMut` overlap check aborted the AVIF tests in `peek-types` (`types::image::avif`) on
  aarch64. Release builds aren't affected, because the check exists only under
  `cfg(debug_assertions)` in rav1d's `src/disjoint_mut.rs`.
  - **Symptom.** A `rav1d-worker` thread panics with `overlapping DisjointMut`: a shared borrow at
    `src/cdef.rs` (`padding()`) against a mutable borrow at `src/cdef_apply.rs` (`backup2lines`).
    The panic is in a function that can't unwind, so the test binary aborts with SIGABRT and the
    harness prints no test name.
  - **Cause.** In rav1d 1.1.0, `padding()` takes the `top` slice from `x = -2` even when `HAVE_LEFT`
    is unset, which happens for a block on the left edge of the frame. It then reads only from
    `x_start = 2`. The 2 unread pixels are the tail of the previous line-buffer row, which another
    worker writes at the same time. No pixel is read during the write, so in practice the overlap is
    harmless. x86 builds use the assembly `cdef` and never run this code. Builds on other
    architectures get the Rust fallback, because `avif-decode` turns off rav1d's `asm` feature
    there. The upstream fix would start the slice at `x_start`.
  - **Reproduce.** Remove the override and build the test binary with
    `cargo test -p peek-types --lib --no-run`. Then run `<binary> --quiet image::avif` 200 times in
    a loop. On a 10-core aarch64 machine, about 9 of 200 runs aborted. Each decode test alone, or
    all of them with `--test-threads=1`, gave 0 aborts in 200 runs. The overlap needs several
    decodes that run at once.
  - **Rejected: single-threaded decode.** `avif-decode` 3 sets rav1d's thread count from
    `available_parallelism()` and has no setting for it. A single-threaded decode would need a fork
    or direct calls to rav1d.
  - **Upstream state on 2026-11-10.** The latest version on crates.io is 1.1.0. Upstream main has an
    unpublished version bump to 1.1.1, but no commit after 1.1.0 changes this logic, and no upstream
    issue reports it. Check again on each `avif-decode` or `rav1d` bump. Once a fixed release lands,
    remove the override and run the loop described earlier.

---

## After 1.0: the 1.x releases

### Text search

The shipped scope is the accepted 1.0 feature. It covers literal and regular expression search,
smart case, `n` and `p`, reach across views, and a budgeted scan. These refinements block nothing:

- **Incremental search**: re-scan and re-highlight on every keystroke instead of on Enter.
- **Wider reach**: the file-info view and the hex dump don't take part in search yet.
- **Lazy, bounded scan**: the current scan is one full pass over the active view, capped at 100,000
  matches. A multi-GB file pays for that pass up front. A lazy "search from here" would scale
  better.

### Styled-text IR (structured spans) ❓

Replace the pipeline of in-band ANSI escapes with a typed span tree, an intermediate representation
(IR): `Line(Vec<Span>)` and `Span { text, style }`. Think of HTML spans, or ratatui's `Span`,
`Line`, and `Style`. Content is **always** plain, sanitized text. The Select Graphic Rendition (SGR)
escapes are generated once, at the leaf, when `peek_theme` flushes a line to the terminal under the
active `StyleMode`.

#### Why it earns its place

- **It removes the escape-leak bug class by construction.** Today, style travels as in-band escapes
  inside `String` values. `render_themed_status_line`, the listings, and the prompt sanitize the
  segment _text_ before they paint it. So if a caller embeds SGR in advance, the sanitizer turns it
  into a visible `␛[…m`. That was the 2026-06-22 bug with the warning marker in the status bar. With
  a span IR, text and style never share a string until render, so this can't happen. The current
  `(String, Color)` segment type can't enforce this. A wrapper type whose contents can't hold
  escapes is the smaller version of the same idea.
- **It removes the round trip from structure to escapes and back.** syntect already hands peek
  `Vec<(Style, &str)>` spans. peek flattens them to escapes early. Then `viewer/ui/styled.rs`, the
  whole family of SGR-aware string functions, rebuilds the structure with `scan`, `Sgr`, and
  `ActiveStyle` to count width, wrap, slice, and truncate. It re-arms the styles across every cut.
  With spans, width, wrap, and slice become trivial, and the re-parse machinery goes away.
- **It centralizes style-mode degradation.** The mapping from truecolor to 256 colors to 16 colors
  to plain lives in one leaf renderer. Strings no longer bake it in when they're built.
- **It unblocks folding.** [Block collapsing](#block-collapsing-and-folding-) already wants a
  line-metadata layer "replacing bare `String` lines." A `Line` type is the natural place for the
  fold level, the block boundaries, and the visibility.

#### Cost and scope

The change is large and cuts across the code base. The render of every mode produces spans, and
`styled.rs` and `ScreenBuffer` (`prev_lines: Vec<String>`) get rebuilt against the IR. Keep an
escape-hatch variant, `Raw`, for the half-block image renderer. It emits dense SGR for each cell and
shouldn't be forced through the IR. Rendering is bounded to the viewport, so allocating spans for
each line is cheap because only visible lines get them. Dropping the repeated escape re-parse on
every scroll and wrap is likely a net win.

Treat this as a prerequisite: do it before folding, or together with it, rather than after.

### Block collapsing and folding ❓

Collapse blocks, such as objects, arrays, and nested structures, in the interactive viewer. It's
mainly for structured data (JSON, YAML, TOML, and XML), but it could extend to code, such as folding
functions and blocks. It's the natural headline capability for 1.1.

#### Challenges

The current pipeline produces a flat `Vec<String>` of ANSI-escaped lines with no structural
metadata. Folding would need:

- A line-metadata layer for the fold level, block boundaries, and visibility state, replacing bare
  `String` lines. [§ Styled-text IR](#styled-text-ir-structured-spans-) is the natural home for it.
- A virtual line mapping, so that scroll offsets work with collapsed regions.
- Fold state that survives re-renders, such as a theme toggle or a toggle between raw and pretty.
- For structured data, either the parsed structure kept around or indentation heuristics.
- For code, language-aware block detection through syntect scopes. That's much harder and depends on
  the language.

Indentation-based folding for structured data such as JSON and YAML would be the most practical
starting point, because pretty-printed output has reliable indentation levels.

Folding also unblocks section folding for INI and properties files. See
[§ Config files](#config-files-).

### Built-in manual ❓

`peek --manual` shipped with a TOC and chapter extract. The design is in
[archived/embedded-manual-plan.md](archived/embedded-manual-plan.md). Next steps:

- **Following links.** A key follows the link under the cursor in the rendered Markdown view. It
  resolves relative `.md` paths and `#fragment` anchors against the chapter table. This needs the
  link spans recorded in the rendered output, and a jump hook in the session layer. The build step
  then keeps relative link targets as link metadata instead of stripping them.
- **Manual in the Tab cycle.** Every file gets a manual mode next to Info and Help. It opens on the
  chapter for the current file type, so a CSV file lands on "CSV / TSV." This needs a map from
  `FileType` to chapter path in `peek-manual`, with a test that every `FileType` maps to an existing
  chapter.
- **Search across chapters** from the TOC.

## File types

These items deepen types that already ship. None of them blocks 1.0. Pick them by demand.

### Presentations ◐

The `presentation` type ships. It shows the text of `.pptx`, `.pptm`, `.ppsx`, and `.odp` files
slide by slide, and the preview of Keynote files. Remaining deepening:

- **`.ppt`**, the legacy Object Linking and Embedding (OLE) or Compound File Binary (CFB) format:
  deferred with the other OLE formats.
- **ODP run styling**: slide runs in OpenDocument Presentation (ODP) files render plain. Bold,
  italic, and color need the `automatic-styles` resolution that the ODT parser in `document` already
  does.
- **Keynote slide text**: modern `.key` files store text as undocumented snappy-compressed protobuf
  (IWA). Today, only the embedded `preview.jpg` and the build metadata show. A full IWA decode is a
  large reverse-engineering effort that breaks easily between versions. It's a task of its own.

### SQL ◐

- A pretty-printer or formatter, an auxiliary mode with a statement outline, and dispatch to a
  distinct PL/pgSQL grammar inside `$$ … $$` bodies.
- An auxiliary outline mode shared between Markdown headings and SQL statements. The mode and its
  key binding are still to be decided.

### CSS ◐

#### Inline annotation of selector specificity

Annotate the selector list of each rule with its specificity tuple (`a,b,c`) in the highlighted CSS
source view. It's the "killer feature" for debugging "why isn't the style winning?" The biggest cost
is the `ContentMode` integration. It needs a separate code path, either gutter or trailing-comment
rendering, and a cache of parsed selectors plumbed through `RenderCtx`. Computing the specificity
itself is cheap.

#### Rewrite of the `svg_anim` animation parser

Replace the hand-rolled CSS parsing in `crates/peek-types/src/types/image/pipeline/svg_anim` with a
typed parser. That code is about 600 lines in `keyframes.rs` and `spec.rs`, and the transform and
selector parsing across `selectors.rs` and `scan.rs`. It's a separate concern with real regression
risk for SVG animation. It's also the one piece that would justify the typed `Transform` and
`Animation` values of `lightningcss` over the current `cssparser` and `cssparser-color` pair. That
pair was picked for the Info view, because it's about 120 to 200 KB against about 400 to 700 KB and
covers everything the Info view needs. Revisit this as its own task. If you pick it up, weigh
swapping the CSS dependency at the same time.

The separate queue for SVG animation **render performance** lives in
[svg-anim-perf.md](svg-anim-perf.md). It holds measurements and proposed optimizations, not a
description of shipped behavior. This roadmap points to it as future work. It's not active right
now.

### Pdfium distribution ◐

- **`install.sh`**: detect a Pdfium that the system already has, for example from Homebrew, and skip
  the bundled copy if one is present. Version pinning for each release is already handled: the
  workflow reads the bundled `BUILD` from `.pdfium/VERSION`.
- **`cargo install`**: the build-time path search through `PDFIUM_DYNAMIC_LIB_PATH` finds the
  library only if the user has set that variable. Document the install steps in the README.
- **Feature flag**: an optional Cargo feature `pdf`, so that a build without PDF support keeps the
  binary small for embedded targets.

### Vector and PostScript files ◐

- **Legacy Adobe Illustrator files from before CS2**: these are pure PostScript, with no PDF
  wrapper. Today they go through the EPS and PostScript path, which shows the source, the Document
  Structuring Conventions (DSC) info, and a `gs` render when Ghostscript is available. peek doesn't
  detect or label them as Illustrator files. Low priority, because such files are rare now.
- **Multi-page `.ps`**: the Ghostscript render view shows page 1 only. A paged render would cover
  multi-page PostScript documents. It would probe the page count with `gs`, rasterize each page, and
  step with `n` and `p`.
- **WMF previews**: a DOS EPS file can carry a Windows Metafile (WMF) preview instead of a TIFF one.
  The Info view names it, but no pure-Rust WMF rasterizer exists, so peek doesn't render it. `gs`
  covers these files anyway when it's present.
- **EPSI inline previews**: peek doesn't parse the rare hex-ASCII preview block in a plain `%!PS`
  EPS file. It parses only the binary TIFF previews of DOS EPS files. `gs` covers these too.

### Archive files ◐

| Format | Extensions | Status |
| ------ | ---------- | ------ |
| RAR    | `.rar`     | ☐      |

RAR is the awkward one. It's a closed format, and support means wrapping a library through the
`unrar` crate, which wraps the proprietary unrar C library. The license is fine for listing only,
but distribution adds friction. So put RAR behind a Cargo feature flag, `rar`, that's off by
default.

- **RAR listing**: a table-of-contents view through the existing `ByteSource` and `ListingMode`
  path.
- **RAR extract**: once RAR listing lands, extract reuses the unrar wrapper. The same caveats about
  listing only apply.

### Disk images ◐

- **ISO Rock Ridge detection**: needs a scan for the System Use Sharing Protocol (SUSP) inside the
  root directory record, which is one extra read pass. It would show real Unix permissions in the
  permissions column.
- **DMG nested filesystem metadata**: the HFS+ and APFS volume names inside the partition payload.
  The partition map, with names, types, sizes, and compression, already shows. See
  [features.md → Disk Images](features.md#disk-images-). This item is the next layer down: walking
  the filesystem of the reconstructed partition. It's significant work and probably never worth it
  for peek.
- **DMG entry extract**: this returns `Unsupported` today. It needs decompression of the UDIF blocks
  (zlib, bzip2, and lzfse chunks) before any filesystem walk could expose individual files. It's
  significant work and deferred indefinitely.

UDF, used by DVD and Blu-ray ISOs, is deferred. It's a more complex format, and the use case is
niche for peek.

### Config files ◐

- **Structured pretty-print and section folding** for INI and properties files through native
  parsers (`rust-ini` and `java-properties`). This needs the folding infrastructure, so it's
  deferred along with it. See [Block collapsing](#block-collapsing-and-folding-).
- **HCL, Dhall, and CUE** beyond highlighting: add typed parsing only if those ecosystems mature.

### Email ◐

- **Inline images** rendered in the body, with `cid:` references resolved against the inline parts.

### Audio files ☐

- **Audiobook chapters** for `.m4b` containers. MP4 chapter atoms or `chpl` boxes drive a `Next` and
  `Prev` flow like the one for EPUB. Defer this until a real m4b file turns up.
- **Cover tab with several pictures**: today only the primary visual, FrontCover or the first one,
  gets the Cover tab. Back, artist, and leaflet pictures live only in the Embeds listing. One Cover
  view could cycle through all visuals with `n` and `p`.
- **Synced lyrics timeline**: `SYLT` ID3v2 frames carry a timestamp for each line. peek flattens
  them to plain text today. The next step would be a timeline view that highlights the current line
  during playback, which is also future work.
- **ASCII waveform or spectrum preview**: decoding adds cost, so decide later. Tags alone are cheap
  and useful.

### Font files ◐

Bare sfnt files (`.ttf`, `.otf`, `.ttc`, and `.otc`) and both web wrappers (`.woff` and `.woff2`)
ship. Stretch goals:

- Sample sentences in several scripts, keyed on cmap coverage. For example, show Cyrillic, Greek,
  Arabic, or CJK samples when the font carries those glyphs. The current sampler is a hard-coded
  ASCII pangram.
- A true recursive peek into a single face of a `.ttc` collection. It would need to build a
  standalone sfnt from the TTC table directory: copy the referenced tables, and recompute the
  offsets and checksums. The current design covers the visible use case at a fraction of the cost.
  It has face-cycle keys (`n` and `p`) on the SpecimenMode and shows the metadata of every face in
  Info.

### PKCS#12 certificates and keys ☐

X.509 DER files (`.der`, and DER-encoded `.crt` and `.cer`) and JSON Web Key (JWK) files and key
sets (`.jwk` and `.jwks`) ship. See
[features.md → Certificates and Keys](features.md#certificates-and-keys-).

| Format        | Extensions     | Notes                                                                                                                                                                                                         |
| ------------- | -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| PKCS#12 / PFX | `.p12`, `.pfx` | Encrypted bag. First cut: show the bag types and embedded certificate and key labels without asking for the password. A password prompt is a separate UX surface: interactive only, pipe mode skips the parse |

Crates: `pkcs12` for the encrypted bags.

### Object files: deeper inspection ☐

- **Notes and build metadata**: compiler and toolchain hints, and whether a code signature is
  present. The build ID, the Mach-O UUID, and the PE PDB GUID already show in Info.
- **Mach-O fat slices**: switch the viewed slice interactively. Today peek picks the slice for the
  host architecture automatically and only lists the rest in the Info view.

---

## Open ideas, deferred indefinitely

### Video files ❓

Render video as ASCII art in real time: decode the frames and run them through the image pipeline.
It's a stretch goal and may not be practical, because of decode performance and the refresh-rate
limits of terminals. It would need an ffmpeg binding.

In print mode, peek would show the file metadata (duration, resolution, codec, and bitrate), and
possibly a single frame.

### Image-decode gap ❓

peek _detects_ some common raster formats through `infer` and routes them to `FileType::Image`. The
`image` 0.25 crate can't decode them, so the render fails:

- **HEIC and HEIF**: the default photo format of the iPhone. `image` has no decoder for it. It needs
  `libheif-rs`, a C library dependency with distribution friction like Pdfium, or a pure-Rust
  decoder once one matures.
- **PSD** (Photoshop): would need the `psd` crate, which gives the flattened composite only.
- **JPEG XL**: needs `jxl-oxide` (pure Rust) or `libjxl`.
- **Camera RAW** (DNG, CR2, NEF, and ARW): partial. Some files decode, and the embedded JPEG preview
  is the pragmatic path.

Each of these adds a decode dependency for a single format. They're deferred until one is worth the
weight. HEIC is the strongest candidate, given how common it is now. Until then, these formats
should at least fail with a clear "format detected, no decoder" message instead of a generic render
error.

### Legacy and Apple binary formats ❓

- **Legacy OLE and CFB office files** (`.doc`, `.xls`, and `.ppt`, from before 2007), with the magic
  bytes `D0CF11E0`: this is the Microsoft Compound File Binary container. Today these files fall
  through to the binary view. A listing of the OLE storage and stream tree, through the `cfb` crate,
  would at least expose the structure. Full text extraction is much more work. These files are rare
  and getting rarer, so this is low priority.
- **Binary plist** (`bplist00`): found all over macOS, for example in preferences and provisioning
  profiles. Today the `plist` extension routes to the XML parser, so _binary_ plists fail to parse.
  XML plists render fine. A magic-byte sniff for `bplist00` and the `plist` crate, used to emit it
  again as a structured tree, would fix it. It's arguably more a routing bug than a missing feature,
  and it's small.

### Parquet ❓

Parquet is the columnar standard of data engineering. Today it routes to the binary view. It fits
the info-screen model well _without_ decoding all the data, because the footer carries the schema,
the row groups, the row and column counts, the compression codec, and statistics for each column.
The first cut would be an Info section with the metadata only, read through the file metadata reader
of the `parquet` crate with no full column scan. A streaming table with a row preview could come
later. Parquet is niche but increasingly common, so this is driven by demand.

### Type-support plugin trait ❓

This is a follow-up to the refactor that moved the code of each type into one place. See
[archived/refactor-types-colocation-plan.md](archived/refactor-types-colocation-plan.md) for the
underlying restructuring and the reasons trait dispatch was rejected.

Each file type already owns its detection module (`crates/peek-detect/src/types/<x>.rs`) and its
reader modules (`info.rs` and `compose.rs`). The **info-render axis already collapsed** into trait
dispatch on 2026-06-05. `info::render` calls `InfoExtras::render_section`, with an implementation
for each type in `types/info_impls.rs`. So this parked item is narrower now and covers only the
**compose and detection** axes. The remaining central dispatch sites could also collapse into
trait-dispatch loops. These are the `Registry::compose_modes` match and the per-type calls in the
`peek-detect` orchestrator:

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

With this, adding a new type means one new directory and one line in `all_types()`.

**Trade-off:** you lose the overview of the dispatch in one file. Today, when you open
`src/compose.rs`, you see the compose strategy of every file type at a glance. With trait dispatch,
you follow a `Vec` to an implementation. IDEs handle the jump fine, but losing the ability to scan
the whole match on one screen is a real cost.

**Recommendation:** revisit this only in one of two cases. Either the number of file types grows
until the central matches no longer fit on one screen, or external plugins, which load a
`TypeSupport` from a dynamic library, become a goal. Until then, the hard-coded matches from that
refactor are easier to read and change.

### Unified timestamp type ❓

Domain-data dates are _partly_ unified. Precise instants now share one representation: `SystemTime`
wrapped by `Value::Timestamp`, serialized as ISO 8601 UTC, and rendered muted. This landed on
2026-06-18 for PDF, document, spreadsheet, presentation, and email. The parse and `days_from_civil`
primitives live in `info/time.rs` in `peek-foundation`. Several sites still store dates their own
way:

| Site                                                                  | Stored as                                                                                                                                                                                                                       |
| --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **cert** (`not_before` / `not_after` / `this_update` / `next_update`) | Pre-formatted ISO 8601 `String`. It has the epoch (`ASN1Time::timestamp()`) but formats early. Left as is, because its JSON is already correct and a typed render would change its print from absolute UTC to viewer-local time |
| **`disk_image`** `IsoDateTime`                                        | Decomposed struct (y/m/d/h/m/s and offset), never assembled into an instant                                                                                                                                                     |
| **iCalendar** `date_range`                                            | `(String, String)` of `YYYY-MM-DD`: date only, and a _range_                                                                                                                                                                    |
| **audio / ebook** `date`                                              | Freeform `String`, often the year only (`2011`)                                                                                                                                                                                 |
| **EPS** `creation_date`, **image** EXIF/XMP                           | Freeform `String`, may not be a date                                                                                                                                                                                            |

The consolidation worth doing is **not** an enum of `SystemTime` and UTC. Those carry the same
information, because a `SystemTime` _is_ an absolute instant, which equals a UTC civil time. So that
split means nothing. The variation that actually exists is **granularity**:

```rust
enum Timestamp {
    Instant(SystemTime),            // precise: cert, iso, the converted five
    Date { y: i32, m: u8, d: u8 },  // date-only: iCalendar, audio year
    Raw(String),                    // un-parseable original, preserved verbatim: EXIF, EPS
}
```

`Value::Timestamp` would hold a `Timestamp`. There would be one `Serialize` (an instant becomes
`…Z`, a date becomes `YYYY-MM-DD`, and raw becomes a string), one `InfoValue` render, and one parse
entry point. The policy for messy cases then lives in one place instead of in scattered `String`
formatting for each type.

- **Pays off** at cert (`String` becomes `Instant`, which removes its duplicate `gregorian`),
  iCalendar (two `Date` values), and `disk_image` (`IsoDateTime` becomes `Instant`).
- **Stays `Raw`** for EXIF and EPS, to avoid false precision.
- **Judgment call** for audio and ebook: a year only as `Date` or as `Raw`.

**Recommendation:** this earns its place once 3 or 4 sites adopt it. If only cert ever moves, a
plain `SystemTime` field is simpler. It's a cleanup driven by demand, not a blocker.
