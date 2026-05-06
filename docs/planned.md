# peek — Planned Features & Ideas

Status legend: ☐ planned · ❓ idea / open

For implemented (✅) and partial (◐) features, see [features.md](features.md).

## File Types

### Markup / Documentation ◐

| Format   | Extensions                            | Status |
|----------|---------------------------------------|--------|
| Markdown | `.md`, `.markdown`, `.mdown`, `.mkd`  | ◐      |
| SQL      | `.sql`, `.ddl`, `.dml`, `.psql`       | ◐      |

Highlighted source + format-aware Info section ship today (see
[features.md → Markdown / SQL](features.md#markdown-)). Still planned:

- Markdown: rendered "read mode" (styled headings, bold, lists, tables, blockquotes, per-language
  dispatch into syntect inside fenced code blocks). Cyclable with Tab against the highlighted
  source.
- SQL: pretty-print / formatter, statement-outline aux mode, distinct PL/pgSQL grammar dispatch
  inside `$$ … $$` bodies.
- Outline aux mode shared between Markdown headings and SQL statements (mode + key binding TBD).

### Structured Data Additions ☐

Pending entries from the Structured Data table in [features.md](features.md):

- **HTML rendered text view** — `.html` source highlighting works today; a rendered text mode is
  planned (see Document Files → Implementation Libraries below for the library choice).
- **CSV / TSV** (`.csv`, `.tsv`) — render as a formatted table with column alignment.

JSONL streaming for multi-GB logs is also still pending — the current implementation reads the
whole file into memory, which is fine for most logs but breaks down for very large ones.

### Document Files ☐

| Format        | Extensions |
|---------------|------------|
| PDF           | `.pdf`     |
| Word (OOXML)  | `.docx`    |
| Excel (OOXML) | `.xlsx`    |

Modern XML-based Office formats only — legacy `.doc` / `.xls` not planned.

Document files should support multiple modes (cyclable with Tab):

- **Text extraction** — primary mode; show what's in the document as plain text.
- **Source browsing** — for OOXML (which is ZIP + XML), browse the internal XML files. Useful for
  debugging or inspecting structure.
- **Rendered preview** (PDF only) — render PDF pages to images, convert to ASCII art. Mostly
  novelty, but useful for seeing page layout at a glance.

File info screen should show document-specific metadata: page count, word count, author, creation
date.

#### Implementation Libraries

- **HTML render** — `html2text` crate (pure Rust, lynx/links-style HTML → terminal text). Source
  view stays via existing syntax highlighter. Full browser-engine render (Blitz / Servo) deferred —
  too heavy, not yet stable.
- **PDF** — `pdfium-render` (bindings to Google PDFium). Used for both page rasterization (preview
  mode → image renderer pipeline) and text extraction. V8/JS-disabled PDFium build (~10–13 MB
  per-platform shared lib). Pure-Rust alternatives (`pdf-extract`, `lopdf`) too fragile on
  real-world PDFs.
- **Office (OOXML)** — `zip` + `quick-xml` for the container and document XML; `calamine` for
  `.xlsx`. Text extract is pure Rust. Source/XML browsing reuses existing structured viewer.
  Visual render of Word/Excel pages out of scope (would require LibreOffice subprocess).

#### PDFium Distribution

Bundle the PDFium shared library (`.dylib` / `.so` / `.dll`) alongside the peek binary in release
artifacts. Adds ~10 MB per platform; acceptable.

`install.sh` should detect an already-installed system PDFium (e.g. via `brew`, package manager, or
a previous peek install) and skip the bundled copy in that case. If no PDFium present, download and
install the matching prebuilt next to the peek binary. Pin the PDFium version per peek release so
ABI/feature drift is bounded.

`cargo install` users compile against whatever PDFium is on their system — document install steps
in the README. Optionally gate the PDF feature behind a Cargo feature flag (`pdf`) so a no-PDF
build stays slim.

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

### Disk Images ☐

| Format | Extensions |
|--------|------------|
| ISO    | `.iso`     |
| DMG    | `.dmg`     |

Same shape as Archive Files — file info shows container metadata; primary content view is a tree
listing of the contained filesystem. Streams via `ByteSource`, no extraction.

**ISO 9660 metadata** (from the Primary Volume Descriptor at sector 16 / offset 32768):

- Volume label, volume set ID
- Publisher, data preparer, application
- Creation, modification, expiration, effective timestamps
- Volume size (sector count × 2048)
- Filesystem extensions present: Joliet (Unicode names), Rock Ridge (POSIX perms/symlinks),
  El Torito (boot record)
- Bootable flag + boot loader description (El Torito)
- Root directory tree → reuses the archive listing primitive

PVD is a fixed-layout 2048-byte block — trivial to parse by hand if only header metadata is needed.
Full directory walk + extensions wants a crate.

**DMG metadata** — UDIF trailer ("koly" block) at the end of file: format version, payload
checksum, partition map offsets, embedded property list. Compressed/encrypted DMGs are harder;
flat read-only images are feasible. Bonus: nested HFS+/APFS volume metadata if a parser is
available.

#### Implementation Libraries

| Format | Crate       | Notes                                                               |
|--------|-------------|---------------------------------------------------------------------|
| ISO    | `cdfs`      | Pure Rust ISO 9660 + Joliet + Rock Ridge reader. Current.           |
| ISO    | hand-rolled | PVD-only metadata — ~50 lines, no crate needed if listing deferred. |
| DMG    | `dmgwiz`    | Pure Rust UDIF reader. Read-only flat DMGs.                         |

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

### Audio Files ☐

| Format | Extensions     |
|--------|----------------|
| MP3    | `.mp3`         |
| FLAC   | `.flac`        |
| WAV    | `.wav`         |
| Ogg    | `.ogg`, `.oga` |
| Opus   | `.opus`        |
| AAC    | `.m4a`, `.aac` |

Metadata-first viewer: tags (title, artist, album, track, year, genre, comment), technical
properties (duration, bitrate, sample rate, channels, codec), embedded album art (route through
the image pipeline) when present.

Stretch: ASCII waveform or spectrum preview. Decoding adds cost — decide later if worth it. Tags
alone are cheap and useful.

Crates: `lofty` (tags + properties, pure Rust, broad format coverage). `symphonia` only if
waveform/spectrum is pursued.

### Font Files ☐

| Format          | Extensions        |
|-----------------|-------------------|
| TrueType        | `.ttf`            |
| OpenType        | `.otf`            |
| Web fonts       | `.woff`, `.woff2` |
| Font collection | `.ttc`, `.otc`    |

Metadata: family name, subfamily, full name, version, copyright, license URL, designer, vendor;
units-per-em, glyph count, supported scripts/codepoints, OS/2 weight/width, monospace flag, hinting
present.

Stretch: specimen render — ASCII-art rasterise a sample string ("The quick brown fox…") at a
chosen size through the existing image pipeline. Glyph rasterization needs a separate crate
(`fontdue` or `ab_glyph`).

Crates: `ttf-parser` or `skrifa` (read-fonts) for metadata. `fontdue` for specimen rasterization.

### Single-File Compressed ☐

| Format | Extensions |
|--------|------------|
| gzip   | `.gz`      |
| bzip2  | `.bz2`     |
| xz     | `.xz`      |
| zstd   | `.zst`     |

Distinct from tar archives — these wrap a single file. Treat transparently: streaming-decompress
through `ByteSource`, run the regular file-type detection on the inner content, render with the
matching viewer. The outer container is invisible to the viewer except for a header line in the
file info screen ("Compressed: gzip, original 12.4 MB → 3.1 MB on disk").

Reuses the same compression crates as the Archive Files plan (`flate2`, `bzip2-rs`, `lzma-rs`,
`zstd`).

### Databases ☐

| Format | Extensions                   |
|--------|------------------------------|
| SQLite | `.db`, `.sqlite`, `.sqlite3` |

Read-only schema-first viewer: list tables/views/indices/triggers with row counts, column
definitions (name, type, NOT NULL, default, PK), foreign keys. Per-table preview (first N rows) as
a Tab subview is a stretch; schema dump is the primary value.

File info: SQLite version, page size/count, encoding, user_version, application_id.

Crate: `rusqlite` (opens read-only via `OpenFlags::SQLITE_OPEN_READ_ONLY`).

### Certificates and Keys ☐

| Format         | Extensions                          |
|----------------|-------------------------------------|
| X.509 PEM      | `.pem`, `.crt`, `.cer`              |
| X.509 DER      | `.der`                              |
| CSR            | `.csr`                              |
| PKCS#12        | `.p12`, `.pfx`                      |
| SSH public key | `.pub` (and lines starting `ssh-…`) |

Decode and pretty-print:

- **Certificates** — subject, issuer, serial, validity (NotBefore/NotAfter, days remaining), SAN
  list, key type/size, signature algorithm, fingerprints (SHA-1, SHA-256), key usage, extended key
  usage, basic constraints.
- **CSR** — subject, requested SAN, key type, signature algorithm.
- **PKCS#12** — bag types, embedded cert/key summaries.
- **SSH public keys** — type, bits, fingerprint, comment.

Raw view (`r`) shows the PEM/text source. Encrypted PKCS#12 prompts skipped — show structural info
that's readable without the password.

Crates: `x509-parser`, `pkcs8`, `ssh-key`.

### Executables and Object Files ☐

| Format      | Extensions                              |
|-------------|-----------------------------------------|
| ELF         | `.elf`, `.so`, often no extension       |
| Mach-O      | `.dylib`, `.bundle`, often no extension |
| PE / COFF   | `.exe`, `.dll`, `.sys`                  |
| WebAssembly | `.wasm`                                 |
| Static libs | `.a`, `.lib`                            |

Default view is a structured metadata report:

- **Header** — format, architecture(s), endianness, file type (executable/library/object), entry
  point, machine flags.
- **Sections / segments** — name, address, size, flags.
- **Symbols** — exported, imported (truncated with a count for huge tables; full list opt-in).
- **Linked libraries** — `DT_NEEDED` (ELF), load commands (Mach-O), import table (PE).
- **Notes / build metadata** — build ID, compiler/toolchain hints, code signature presence.
- **Mach-O fat binaries** — list each slice.

`goblin` covers ELF / Mach-O / PE / archive (`.a`) under one API. `wasmparser` for `.wasm` (module
imports/exports/memory/table summary).

## Image Features

### Zoom ☐

`+`/`-` to scale up/down from the current sizing baseline. Height overflow uses viewer scrolling.
Width overflow uses the existing horizontal-pan mechanism — `Left`/`Right` already pans image
views under `FitHeight`; zoom can reuse that path so an over-wide zoomed image scrolls horizontally
instead of wrapping or truncating.

Implementation sketch: zoom multiplies the rendered grid size from the active sizing baseline. When
the resulting width exceeds the terminal, treat it like `FitHeight` — render full width, scroll
horizontally; render full height, scroll vertically. Effectively a free-zoom mode where both axes
scroll. A position indicator (`[3,2]/[5,4]`) can show viewport location. Print mode wouldn't
support zoom (interactive-only).

Capping zoom at terminal width is a simpler fallback if the dual-axis scroll proves clunky, but
horizontal pan is already in the viewer, so the marginal cost is small.

## Viewer Features

### Text Search ☐

`/` opens search prompt; type pattern, Enter searches. `n` / `N` jump to next / previous. Matches
highlighted in content. Regex is desirable; plain text is the minimum. Applies to all text-based
views (source, structured, document text, file info).

### Large File Safeguards ☐

For large files: viewer mode defaults to the file info screen instead of loading full contents.
Display a size warning. Keyboard shortcut to opt in to loading. File info (size, type) obtainable
without reading the whole file.

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
