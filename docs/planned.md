# peek — Planned Features & Ideas

Status legend: ☐ planned · ❓ idea / open

For implemented (✅) and partial (◐) features, see [features.md](features.md).

## File Types

### SQL ◐

- Pretty-print / formatter, statement-outline aux mode, distinct PL/pgSQL grammar dispatch
  inside `$$ … $$` bodies.
- Outline aux mode shared between Markdown headings and SQL statements (mode + key binding TBD).

### CSS ◐

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

### PDFium Distribution ◐

- **install.sh**: detect an already-installed system Pdfium (homebrew etc.) and skip the bundled
  copy when present. (Version pinning per release is already handled — the workflow reads the
  bundled `BUILD` from `.pdfium/VERSION`.)
- **`cargo install`**: build-time path search via `PDFIUM_DYNAMIC_LIB_PATH` only finds the lib
  if the user has set it; document install steps in the README.
- **Feature flag**: optional Cargo feature `pdf` so a no-PDF build keeps binary size down for
  embedded targets.

### Vector / PostScript Files ◐

- **Legacy AI (pre-CS2)** — pure PostScript, no PDF wrapper. Routes through the EPS/PostScript
  path today (source + DSC info + `gs` render when available), but isn't specifically detected
  or labelled as Illustrator. Low priority — such files are rare now.
- **Multi-page `.ps`** — the Ghostscript render view shows page 1 only. A paged render (gs
  page-count probe + per-page rasterise, `n` / `p` to step) would cover multi-page PostScript
  documents.
- **WMF previews** — DOS-EPS files can carry a Windows Metafile preview instead of TIFF. The
  Info view names it but there's no pure-Rust WMF rasteriser, so it's not rendered. `gs` covers
  these files anyway when present.
- **EPSI inline previews** — the rare hex-ASCII preview block in plain `%!PS` EPS isn't parsed
  (only binary DOS-EPS TIFF previews are). `gs` covers these too.

### Video Files ❓

Render video as ASCII art in real-time — decode frames and run through the image pipeline. Stretch
goal; may not be practical due to decode performance and terminal refresh-rate limits. Would need an
ffmpeg binding.

In print mode: file metadata (duration, resolution, codec, bitrate), possibly a single frame.

### Archive Files ◐

| Format | Extensions | Status |
|--------|------------|--------|
| RAR    | `.rar`     | ☐      |

RAR is the awkward one — closed format, library wrap via `unrar` (wraps the proprietary unrar C
lib). License caveats: listing only is fine, but distribution adds friction, so defer behind a
Cargo feature flag (`rar`), off by default.

- **RAR listing** — table-of-contents view through the existing `ByteSource` / `ListingMode` path.
- **RAR extract** — once RAR listing lands, extract reuses the unrar wrapper; same listing-only
  caveats apply.

### Disk Images ◐

- **ISO Rock Ridge detection** — needs a SUSP scan inside the root directory record; one extra
  read pass. Would surface real Unix permissions in the perms column.
- **DMG nested filesystem metadata** — HFS+ / APFS volume names inside the partition payload.
  The partition map (names / types / sizes / compression) already shows — see
  [features.md → Disk Images](features.md#disk-images-); this is the next layer down, walking the
  reconstructed partition's filesystem. Significant work; probably never worth it for peek.
- **DMG entry extract** — currently returns `Unsupported`. Needs UDIF block decompression
  (zlib / bzip2 / lzfse chunks) before any meaningful filesystem walk could expose individual
  files. Significant work, deferred indefinitely.

UDF (DVD / Blu-ray ISOs) deferred — more complex format, niche use case for peek.

### Config Files ◐

- **Structured pretty-print + section folding** for INI / properties via native parsers
  (`rust-ini`, `java-properties`) — would need the folding infrastructure (see
  [Block Collapsing](#block-collapsing--folding-)), so deferred with it.
- **HCL / Dhall / CUE** beyond highlighting — typed parsing only if those ecosystems mature.

### Email ◐

- **Attachment content-type column** in the listing (today rows show name + size only; the
  listing primitive has no type column).
- **Inline images** rendered in the body (cid: references resolved against inline parts).

### Audio Files ☐

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

Bare sfnt (`.ttf` / `.otf` / `.ttc` / `.otc`) and both web wrappers (`.woff` / `.woff2`) ship.
Stretch:

- Multi-script sample sentences keyed on cmap coverage (Cyrillic / Greek / Arabic / CJK
  samplers when the font carries the glyphs). The current sampler is a hard-coded ASCII
  pangram.
- True recursive peek into a single face of a `.ttc` collection — would need to synthesise
  a standalone SFNT from the TTC table directory (copy referenced tables, recompute offsets
  and checksums). The current shape — face-cycle keys (`n` / `p`) on the SpecimenMode and
  every face's metadata in Info — covers the visible use case at a fraction of the cost.

### Certificates and Keys — DER / PKCS#12 / JWK ☐

| Format        | Extensions      | Notes                                                                                                                                                                                                 |
|---------------|-----------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| X.509 DER     | `.der`          | Today `.crt` / `.cer` carrying raw DER fall through to the hex viewer. Wire a magic-byte / leading `0x30 0x82` sniff and decode through the same `x509-parser` path the PEM viewer uses               |
| PKCS#12 / PFX | `.p12`, `.pfx`  | Encrypted bag. First cut: show bag types + embedded cert/key labels without prompting for the password. A password prompt is a separate UX surface (interactive only; pipe mode would skip the parse) |
| JWK / JWKS    | `.jwk`, `.jwks` | JSON form — the structured viewer already pretty-prints these. A cert sidecar would add key thumbprint (RFC 7638) and a normalised key-type / bits / curve row                                        |

Crates: `der` / `cms` (DER + PKCS#7), `pkcs12` (encrypted bags). JWK can ride the existing
`serde_json` dependency.

### Object Files — deeper inspection ☐

- **Linked libraries** — `DT_NEEDED` (ELF), load commands (Mach-O), import table (PE).
- **Notes / build metadata** — build ID, compiler / toolchain hints, code-signature presence.
- **Mach-O fat slices** — switch the viewed slice interactively. Today the host-arch slice is
  auto-picked and the rest only listed in the Info view.
- **WebAssembly `.wasm`** and **static libraries `.a` / `.lib`** — `object` parses both; neither
  detection routing nor a tailored view is wired.
- **Bare COFF `.obj`** — no magic signature, so not auto-detected.

## Viewer Features

### Text Search ◐

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

| Priority | Site                                 | Fix                                                                                                      |
|----------|--------------------------------------|----------------------------------------------------------------------------------------------------------|
| High     | DOCX / ODT / HTML / RTF render cache | Cap analogous to `PRETTY_MAX_BYTES`; above cap → "too large for rendered view, raw source only".         |
| Medium   | EPUB + PDF + CBZ paged cache         | LRU cap (last N renders) keyed by viewport.                                                              |
| Medium   | Audio visuals                        | Per-visual byte cap; reject oversized cover art early.                                                   |
| Low      | Pretty-print double-buffer           | Share raw vec between pretty and highlighter to halve footprint.                                         |
| Low      | Stdin slurp                          | Document the limit; consider spill-to-tempfile for huge stdin streams (mirror the archive extract path). |

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
