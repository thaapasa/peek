# peek — Planned Features & Ideas

Status legend: ☐ planned · ❓ idea / open

For implemented (✅) and partial (◐) features, see [features.md](features.md).

## File Types

### Markup / Documentation ◐

| Format   | Extensions                                              | Status |
|----------|---------------------------------------------------------|--------|
| Markdown | `.md`, `.markdown`, `.mdown`, `.mkd`, `.mkdn`, `.mdwn`  | ✅      |
| SQL      | `.sql`, `.ddl`, `.dml`, `.psql`                         | ◐      |

Markdown rendered read mode (CommonMark + GFM, syntect-highlighted fenced code, box-drawing
tables, task lists, footnotes, frontmatter strip) shipped — see
[features.md → Markdown](features.md#markdown-). SQL highlighted source + format-aware Info
section also ship today. Still planned:

- SQL: pretty-print / formatter, statement-outline aux mode, distinct PL/pgSQL grammar dispatch
  inside `$$ … $$` bodies.
- Outline aux mode shared between Markdown headings and SQL statements (mode + key binding TBD).

### CSS ◐

Syntax-highlighted source + CSS-aware Info section (stylesheet stats, `@import` list,
colour-palette swatches) ship today — see
[features.md → CSS](features.md#css-). Two items still planned:

#### Selector specificity inline-annotation

Annotate each rule's selector list with its specificity tuple (`a,b,c`) in the highlighted CSS
source view. The "killer feature" for debugging "why isn't my style winning". Biggest cost is the
`ContentMode` integration: a separate code path (gutter or trailing-comment rendering) and a
parsed-selector cache plumbed through `RenderCtx`. Specificity itself is cheap to compute.

#### svg_anim keyframe-parser rewrite

Replace the hand-rolled `types/image/pipeline/svg_anim` CSS parsing (`keyframes.rs` /
`spec.rs` / transform parsing, ~250 LOC) with a typed parser. Separate concern with real
SVG-animation regression risk, and the one piece that would actually justify `lightningcss`'s
typed `Transform` / `Animation` values over the current `cssparser` + `cssparser-color` pair
(picked for the Info view because it's ~120–200 KB vs ~400–700 KB and covers everything the Info
view needs). Revisit as its own task; if picked up, weigh swapping the CSS dep then.

### Structured Data Additions ☐

CSV / TSV shipped — see [features.md](features.md#structured-data--config-files).

JSONL streaming for multi-GB logs is still pending — the current implementation reads the
whole file into memory, which is fine for most logs but breaks down for very large ones.

### Document Files ☐

DOCX, RTF, and PDF are shipped — see [features.md](features.md). What's still planned:

| Format        | Extensions |
|---------------|------------|
| Excel (OOXML) | `.xlsx`    |

Modern XML-based Office formats only — legacy `.doc` / `.xls` not planned.

`.xlsx` should support text extraction (sheet → tab-separated rows or rendered table) and the
existing OOXML-as-ZIP TOC browsing. `calamine` handles parsing without bringing a full
spreadsheet engine.

#### PDFium Distribution

PDF support ships with `pdfium-render` (dynamically loads `libpdfium.dylib` / `.so` / `.dll`).
The release tarball already bundles the matching Pdfium build next to the binary — see
`release.yml`'s `Bundle Pdfium` step. Still-pending packaging work:

- **install.sh**: detect an already-installed system Pdfium (homebrew etc.) and skip the bundled
  copy when present. (Version pinning per release is already handled — the workflow reads the
  bundled `BUILD` from `.pdfium/VERSION`.)
- **`cargo install`**: build-time path search via `PDFIUM_DYNAMIC_LIB_PATH` only finds the lib
  if the user has set it; document install steps in the README.
- **Feature flag**: optional Cargo feature `pdf` so a no-PDF build keeps binary size down for
  embedded targets.

### Vector / PostScript Files ☐

| Format                  | Extensions |
|-------------------------|------------|
| Adobe Illustrator       | `.ai`      |
| Encapsulated PostScript | `.eps`     |
| PostScript              | `.ps`      |

Modern `.ai` files (CS2 / 2005 onwards) are PDF 1.x internally — Illustrator saves a
PDF-compatible stream by default. Detect the `%PDF-` magic in the first bytes and route to
`pdfium-render` (already planned for PDF). Free win: the same library covers modern AI.

Legacy AI (pre-CS2) is pure PostScript and follows the EPS path below.

**EPS modes (cyclable with Tab):**

- **Embedded preview** (default when present) — DOS EPS Binary files start with `C5 D0 D3 C6`
  followed by a 30-byte header that points to an embedded TIFF, JPEG, or WMF preview baked in by
  the designer. Extract the preview and render through the existing image pipeline. Quality is
  whatever was baked — fine for "is this the right file?" peeks.
- **Source view** — syntax-highlighted PostScript (DSC-style comments + PS body).
- **Info** — parsed DSC comments (`%%Title`, `%%Creator`, `%%CreationDate`, `%%BoundingBox`,
  `%%For`, `%%LanguageLevel`) plus preview format / dimensions when present.

Plain `.eps` / `.ps` without an embedded preview falls back to source view by default.

**Optional Ghostscript path.** True PostScript rendering needs Ghostscript — AGPL/GPL, large C
dependency. Don't bundle. Detect `gs` on PATH at runtime; if available, offer a "render via gs"
mode for files without an embedded preview. Document the optional dependency in the README.
Pure-Rust PostScript rendering does not exist at usable quality.

#### Implementation Libraries

| Concern             | Crate / approach                                                                          |
|---------------------|-------------------------------------------------------------------------------------------|
| Modern AI rendering | `pdfium-render` (already planned for PDF — same dep covers AI).                           |
| EPS binary header   | Hand-parsed (30-byte struct, `C5 D0 D3 C6` magic, offset/length to PostScript + preview). |
| Embedded preview    | `image` crate (TIFF / JPEG already supported).                                            |
| DSC comment parse   | Hand-rolled (~50 lines; line-prefix scan up to `%%EndComments`).                          |
| Real PS rendering   | `gs` subprocess (optional). No bundled dep.                                               |

### Video Files ❓

Render video as ASCII art in real-time — decode frames and run through the image pipeline. Stretch
goal; may not be practical due to decode performance and terminal refresh-rate limits. Would need an
ffmpeg binding.

In print mode: file metadata (duration, resolution, codec, bitrate), possibly a single frame.

### Archive Files ◐

| Format      | Extensions                     | Status |
|-------------|--------------------------------|--------|
| ZIP         | `.zip`, `.jar`, `.war`, `.apk` | ✅      |
| Tar         | `.tar`                         | ✅      |
| Tar + gzip  | `.tar.gz`, `.tgz`              | ✅      |
| Tar + bzip2 | `.tar.bz2`, `.tbz2`            | ✅      |
| Tar + xz    | `.tar.xz`, `.txz`              | ✅      |
| Tar + zstd  | `.tar.zst`, `.tzst`            | ✅      |
| 7-Zip       | `.7z`                          | ✅      |
| RAR         | `.rar`                         | ☐      |

Listing-only mode — primary view is a file tree with per-entry size, mode, and mtime. No
extraction, no content preview of inner files. Reuses the existing permissions/size painting from
the file info screen. Tab cycles tree view ↔ file info; hex (`x`) still works on the raw archive
bytes.

Reads the table of contents only — no payload decompression — so even multi-GB archives list
instantly via streaming through the existing `ByteSource`.

#### Implementation Libraries

| Format        | Crate         | Notes                                                                                                  |
|---------------|---------------|--------------------------------------------------------------------------------------------------------|
| `.zip`        | `zip`         | Pure Rust, mature. Central directory = ready-made TOC.                                                 |
| `.tar` family | `tar`         | Pure Rust. Streaming entry iterator.                                                                   |
| `.gz`         | `flate2`      | Pure Rust (miniz_oxide backend).                                                                       |
| `.bz2`        | `bzip2-rs`    | Pure Rust.                                                                                             |
| `.xz`         | `lzma-rs`     | Pure Rust. (`xz2` C-binding alternative if needed.)                                                    |
| `.zst`        | `zstd`        | C bindings, well-maintained.                                                                           |
| `.7z`         | `sevenz-rust` | Pure Rust.                                                                                             |
| `.rar`        | `unrar`       | Wraps proprietary unrar C lib. License caveats — listing only is fine, but distribution adds friction. |

RAR is the awkward one — closed format, library wrap. Defer behind a Cargo feature flag (`rar`),
off by default. Everything else is pure Rust or low-friction C bindings.

#### Extract enhancements

Extract from archive entries ships today via `--extract <KEY>` and `e` in the viewer (see
[features.md → Extraction](features.md#extraction-)). Entries ≥ 16 MiB now spool to a
`NamedTempFile` (`InputSource::TempFile`), lifting the in-memory 256 MiB cap for the common
case. Still planned:

- **Stored zip / uncompressed tar → `FileRange`** — when the entry is stored as-is in the
  archive, expose the entry as a zero-copy offset+limit view into the backing file rather than
  spooling. Same path that ISO extracts already use. Saves the spool write for "tar of large
  uncompressed binaries".
- **Stream-walk tar/cpio off a `TempFile` source** — recursive descent into an archive entry
  that itself contains a tar/cpio re-buffers the outer entry via `source.read_bytes()`. Switch
  to a streaming walk over `open_byte_source()` so nested big-on-big stays disk-only.
- **RAR extract** — once RAR listing lands, extract reuses the unrar wrapper; same listing-only
  caveats apply.

### Disk Images ◐

| Format | Extensions | Status                                                |
|--------|------------|-------------------------------------------------------|
| ISO    | `.iso`     | ✅ PVD metadata + recursive directory listing (Joliet) |
| DMG    | `.dmg`     | ✅ UDIF trailer-only (no partition map walk)           |

ISO ships with a TOC view backed by `viewer::listing` (same render path as archive containers).
DMG remains metadata-only. See [features.md → Disk Images](features.md#disk-images-) for what's
surfaced today.

Still planned:

- **ISO Rock Ridge detection** — needs a SUSP scan inside the root directory record; one extra
  read pass. Would surface real Unix permissions in the perms column.
- **DMG partition map** — parse the embedded XML plist for the blkx tables (partition list with
  per-partition name, type, size). Adds a `plist` crate dependency.
- **DMG nested filesystem metadata** — HFS+ / APFS volume names inside the partition payload.
  Significant work; probably never worth it for peek.
- **DMG entry extract** — currently returns `Unsupported`. Needs UDIF block decompression
  (zlib / bzip2 / lzfse chunks) before any meaningful filesystem walk could expose individual
  files. Significant work, deferred indefinitely.

#### Implementation Libraries

| Format          | Crate       | Notes                                                                       |
|-----------------|-------------|-----------------------------------------------------------------------------|
| ISO (PVD + TOC) | hand-rolled | Current implementation — directory walker is ~250 LOC, no crate dependency. |
| DMG (trailer)   | hand-rolled | Current implementation — ~80 lines, no crate dependency.                    |
| DMG (partition) | `plist`     | For decoding the embedded XML partition map.                                |

UDF (DVD / Blu-ray ISOs) deferred — more complex format, niche use case for peek.

### Notebooks ☐

| Format           | Extensions |
|------------------|------------|
| Jupyter Notebook | `.ipynb`   |

`.ipynb` is JSON under the hood. Default content view should render the notebook as a sequence of
cells (markdown text + syntax-highlighted source + outputs) rather than dumping raw JSON. Raw JSON
view stays available via `r` (raw toggle).

File info: kernel/language, cell count (markdown vs code), output count, notebook metadata.

### Config Files ☐

| Format     | Extensions              |
|------------|-------------------------|
| INI / CFG  | `.ini`, `.cfg`, `.conf` |
| .env       | `.env`                  |
| Java Props | `.properties`           |
| HCL        | `.hcl`, `.tf`           |
| Dhall      | `.dhall`                |
| CUE        | `.cue`                  |

Most route through syntect for highlighting (already present for some). INI/properties have native
parsers if structured pretty-print + section folding ever wanted. HCL/Dhall/CUE: highlighting only
unless the parser ecosystems mature.

Crates (where pretty-print is wanted): `rust-ini`, `java-properties`, `hcl-rs`.

### Email ☐

| Format | Extensions |
|--------|------------|
| RFC822 | `.eml`     |
| Mbox   | `.mbox`    |

Default view shows headers (From, To, Cc, Subject, Date, Message-ID) followed by body. Multipart
messages list parts with content-type and size; HTML parts render via the same `html2text` path as
the HTML viewer. Attachments listed with filename, type, size — not extracted.

For `.mbox` (multiple messages concatenated): show a list view first, drill into a single message.

File info: header summary, part count, total size, MIME walk.

Crate: `mail-parser` (pure Rust).

### Calendar / Contacts ☐

| Format | Extensions |
|--------|------------|
| iCal   | `.ics`     |
| vCard  | `.vcf`     |

Pretty list of events / contacts: human-readable date/time formatting, grouped fields, normalised
property names. Raw view shows the original text.

File info: event/contact count, date range (calendars), version.

Crates: `ical` (covers both iCalendar and vCard, pure Rust).

### Audio Files — Stretch ☐

Tags + technical properties + embedded cover-art view + embedded lyrics view + embeds
listing shipped (see [features.md](features.md) "Audio Files"). Open ideas:

- **Audiobook chapters** for `.m4b` containers — MP4 chapter atoms / `chpl` boxes drive a
  `NextChapter` / `PrevChapter` flow like EPUB. Defer until a real m4b ships up.
- **Multi-picture Cover tab.** Today only the primary (FrontCover or first) visual gets the
  Cover tab; back / artist / leaflet pictures only live in the Embeds listing. Could cycle
  through all visuals in one Cover view with `n` / `p`.
- **Synced lyrics timeline.** `SYLT` ID3v2 frames carry per-line timestamps; currently flattened
  to plain text. A timeline view that highlights the current line during (future) playback
  would be the next step.
- **ASCII waveform or spectrum preview.** Decoding adds cost — decide later. Tags alone are
  cheap and useful.

### Font Files ◐

TTF / OTF / TTC metadata + specimen render ship — see
[features.md → Fonts](features.md#fonts-). Still planned:

| Format    | Extensions        |
|-----------|-------------------|
| Web fonts | `.woff`, `.woff2` |

WOFF / WOFF2 need separate decompressors (zlib / brotli) — deferred until the case for shipping
their dep weight is concrete.

Stretch:

- Multi-script sample sentences keyed on cmap coverage (Cyrillic / Greek / Arabic / CJK
  samplers when the font carries the glyphs). The current sampler is a hard-coded ASCII
  pangram.
- True recursive peek into a single face of a `.ttc` collection — would need to synthesise
  a standalone SFNT from the TTC table directory (copy referenced tables, recompute offsets
  and checksums). The current shape — face-cycle keys (`n` / `p`) on the SpecimenMode and
  every face's metadata in Info — covers the visible use case at a fraction of the cost.

### Single-File Compressed ◐

`.gz` / `.bz2` / `.xz` / `.zst` / `.lz4` single-stream wrappers ship — see
[features.md → Single-stream Compression](features.md#single-stream-compression-). Still planned:

| Format | Extensions |
|--------|------------|
| brotli | `.br`      |

Brotli would slot into the same transparent-decompress pipeline (`brotli` crate).

### Databases ☐

| Format | Extensions                   |
|--------|------------------------------|
| SQLite | `.db`, `.sqlite`, `.sqlite3` |

Read-only schema-first viewer: list tables/views/indices/triggers with row counts, column
definitions (name, type, NOT NULL, default, PK), foreign keys. Per-table preview (first N rows) as
a Tab subview is a stretch; schema dump is the primary value.

File info: SQLite version, page size/count, encoding, user_version, application_id.

Crate: `rusqlite` (opens read-only via `OpenFlags::SQLITE_OPEN_READ_ONLY`).

### Certificates and Keys — DER / PKCS#12 / JWK ☐

PEM ships (see [features.md](features.md#certificates-and-keys-) — X.509 cert / CSR / CRL /
private + public keys / SSH pubkey, fingerprints, SANs, key usage, validity, days remaining).
Still open:

| Format        | Extensions       | Notes                                                                                                                                                                                                  |
|---------------|------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| X.509 DER     | `.der`           | Today `.crt` / `.cer` carrying raw DER fall through to the hex viewer. Wire a magic-byte / leading `0x30 0x82` sniff and decode through the same `x509-parser` path the PEM viewer uses                |
| PKCS#12 / PFX | `.p12`, `.pfx`   | Encrypted bag. First cut: show bag types + embedded cert/key labels without prompting for the password. A password prompt is a separate UX surface (interactive only; pipe mode would skip the parse) |
| JWK / JWKS    | `.jwk`, `.jwks`  | JSON form — the structured viewer already pretty-prints these. A cert sidecar would add key thumbprint (RFC 7638) and a normalised key-type / bits / curve row                                          |

Crates: `der` / `cms` (DER + PKCS#7), `pkcs12` (encrypted bags). JWK can ride the existing
`serde_json` dependency.

### Object Files — deeper inspection ☐

The base object-file viewer ships — ELF / Mach-O / PE/COFF detection, header Info, Sections and
Symbols tables, universal-binary unwrapping (see [features.md](features.md)). Still open:

- **Linked libraries** — `DT_NEEDED` (ELF), load commands (Mach-O), import table (PE).
- **Notes / build metadata** — build ID, compiler / toolchain hints, code-signature presence.
- **Mach-O fat slices** — switch the viewed slice interactively. Today the host-arch slice is
  auto-picked and the rest only listed in the Info view.
- **WebAssembly `.wasm`** and **static libraries `.a` / `.lib`** — `object` parses both; neither
  detection routing nor a tailored view is wired.
- **Bare COFF `.obj`** — no magic signature, so not auto-detected.

## Image Features

## Viewer Features

### Text Search ◐

Exact-substring search with smart-case shipped for every text-rendering view — `ContentMode`,
the rendered HTML view, the EPUB / DOCX / ODT / RTF read views, the PDF text view, the
CSV / TSV table view (single-cell scope), and listings (leaf-name scope) — all on a shared
`SearchState`. See [features.md → Text Search](features.md#text-search-). Still planned:

- **Regex matching** — the "desirable" from the original spec. Plain substring is the shipped
  minimum.
- **Incremental search** — re-scan + re-highlight on every keystroke instead of confirm-on-Enter.
- **Wider reach** — file-info view and the hex dump. Those don't participate yet.
- **Lazy / bounded scan** — the current scan is one full pass over the active view, capped at
  100,000 matches; a multi-GB file pays that pass up front. A lazy "search from here" would
  scale better.

### Large File Safeguards ☐

For large files: viewer mode defaults to the file info screen instead of loading full contents.
Display a size warning. Keyboard shortcut to opt in to loading. File info (size, type) obtainable
without reading the whole file.

## Memory / Streaming ☐

North star #2 from CLAUDE.md: *stream, don't load*. Sites where view-mode caches grow
unboundedly with scroll, or whole-file slurps lack a cap. Audit snapshot (2026-05-17) in
[archived/memory-audit-2026-05.md](archived/memory-audit-2026-05.md) — file paths in that
snapshot have drifted; treat its categorization as the source-of-truth shape, the specific
file:line citations as starting points to re-find.

| Priority | Site                                 | Fix                                                                                                                                        |
|----------|--------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------|
| High     | CSV `records` Vec                    | Sliding window / ring around viewport; drop records far from current scroll. Alternative: hard ceiling + "showing first N records" notice. |
| High     | DOCX / ODT / HTML / RTF render cache | Cap analogous to `PRETTY_MAX_BYTES`; above cap → "too large for rendered view, raw source only".                                           |
| Medium   | EPUB + PDF + CBZ paged cache         | LRU cap (last N renders) keyed by viewport.                                                                                                |
| Medium   | Audio visuals                        | Per-visual byte cap; reject oversized cover art early.                                                                                     |
| Low      | Pretty-print double-buffer           | Share raw vec between pretty and highlighter to halve footprint.                                                                           |
| Low      | Stdin slurp                          | Document the limit; consider spill-to-tempfile for huge stdin streams (mirror the archive extract path).                                   |

The tar/cpio re-buffer on `TempFile` sources is already tracked under
[Archive Files → Extract enhancements](#extract-enhancements).

## Future / Optional Features

### Block Collapsing / Folding ❓

Collapse blocks (objects, arrays, nested structures) in the interactive viewer. Mainly for
structured data (JSON, YAML, TOML, XML) but could extend to code (folding functions, blocks).

**Challenges:** the current pipeline produces a flat `Vec<String>` of ANSI-escaped lines with no
structural metadata. Folding would require:

- Line metadata layer (fold level, block boundaries, visibility state) replacing bare `String` lines
- Virtual line mapping so scroll offsets work with collapsed regions
- Preserving fold state across re-renders (theme toggle, raw/pretty toggle)
- For structured data: retaining parsed structure or using indentation heuristics
- For code: language-aware block detection via syntect scopes (significantly harder,
  language-dependent)

Indentation-based folding for structured data (JSON/YAML) would be the most practical starting
point — pretty-printed output has reliable indentation levels.

### Type-support plugin trait ❓

Follow-up to the types-colocation refactor — see
[archived/refactor-types-colocation-plan.md](archived/refactor-types-colocation-plan.md) for the
underlying restructuring and the rejected-trait-dispatch rationale.

Once every file type owns its `format.rs`, `detect.rs`, `info.rs`, and `compose.rs`, the central
dispatch sites (`Registry::compose_modes` match, `input/detect.rs::DETECTORS` list, `info::render`
match) could collapse into trait-dispatch loops:

```rust
trait TypeSupport {
    fn matches(&self, detected: &Detected) -> bool;
    fn compose(&self, ctx: &ComposeCtx, modes: &mut Vec<Box<dyn Mode>>) -> Result<()>;
    fn detect_by_extension(&self, ext: &str) -> Option<FileType>;
    fn detect_by_magic(&self, head: &[u8]) -> Option<FileType>;
    fn render_info(&self, extras: &FileExtras, theme: &PeekTheme, opts: RenderOptions) -> Vec<String>;
}

fn all_types() -> Vec<Box<dyn TypeSupport>> { /* one entry per type */ }
```

Adding a new type becomes one new directory plus one line in `all_types()`.

**Trade-off:** loses the single-file dispatch overview. Today, opening `viewer/mod.rs` shows every
file type's compose strategy at a glance; with trait dispatch, the reader follows a `Vec` to an
implementation. IDEs handle the jump fine, but losing the "scan the whole match in one screen"
property is real.

**Recommendation:** revisit only if the number of file types grows past the point where the
central matches stop fitting on one screen, or if external plugins (loading a `TypeSupport` from a
dynamic library) become a goal. Until then, the hard-coded matches established by the colocation
refactor are easier to read and modify.
