# Architecture map

Full file/module breakdown. Read when adding files, modifying a module, or unsure where logic lives.
CLAUDE.md keeps a condensed top-level version; this is the detailed reference.

Cargo workspace: the `peek` binary at the repo root plus two leaf crates under `crates/`. The split
makes the architectural rule "detection must not depend on the reader/viewer layer" a compile-time
guarantee (Cargo dependency edges) rather than convention.

```
crates/
  peek-io/             — input foundation crate. Depends on nothing in-tree.
    src/lib.rs         — re-exports InputSource, ByteSource, LineSource, ByteStream
    src/source.rs      — InputSource (File / Memory{Bytes} / FileRange{base,offset,len} / TempFile{Arc<NamedTempFile>}) + ByteSource trait + FileByteSource / BytesByteSource / RangeByteSource / TempFileByteSource (holds the Arc so reads outlive the source). read_bytes() returns bytes::Bytes; Memory arm is a refcount clone
    src/lines.rs       — LineSource: streaming, anchor-indexed line view over InputSource
    src/stream.rs      — ByteStream: io::Read / io::BufRead / io::Seek wrapper over any ByteSource so callers can use io::copy / read_until / lines (tar / cpio / etc. go through this seam); Seek is relative to the range start so a post-BOM range is a clean 0-based seekable stream. `ReadSeek` (Read + Seek) trait alias lets a reader carry either a ByteStream or a Cursor behind one Box (csv seekable reader)
    src/compression.rs — CompressionFormat (gz/bz2/xz/zst/lz4/br) + codec_label/suffix + decompress_bytes (brotli is extension-only, no magic) + stripped_name; MAX_DECOMPRESS_BYTES = 256 MiB. The transparent-decompression orchestration (resolve_transparent) lives in peek-detect because it re-runs detection
    src/stdin.rs       — read_stdin (read piped stdin into a Memory InputSource) + reopen fd 0 from /dev/tty after the pipe is consumed. The CLI-level "file vs stdin" decision (needs Args) stays in the binary
  peek-detect/         — file-type detection crate. Depends on peek-io only — never the readers.
    src/lib.rs         — re-exports the detection surface + resolve_transparent
    src/detect.rs      — detection orchestrator (magic-byte / extension / content-sniff priority) + FileType + Detected + DecompressionContext; CompressionFormat re-exported from peek-io; per-type format enums re-exported from `types::<x>`
    src/mime.rs        — MimeCategory + MimeInfo: RFC 6838 classification (Registered / Vendor / x-prefix / unknown) used by the Info screen MIME row
    src/transparent.rs — resolve_transparent (called at every (source, Detected) entry boundary so bare wrappers open straight to inner content): decompress via peek-io, then re-run detect on the inner bytes
    src/types/          — one module per file type: the format enum + the pure ext/MIME/content-sniff helpers the orchestrator calls (the former bin-side `types/<x>/{format,detect}.rs`, merged). No reader/viewer code. cert pulls x509-parser (DER content-verify); the rest are dependency-light. Readers re-export each format enum at their module root (`crate::types::<x>::<X>Format`).
      archive.rs       — ArchiveFormat enum + label; format_from_name + format_from_mime (handles double-extensions `.tar.gz` etc. before bare compression)
      audio.rs         — AudioFormat enum + label; format_from_ext + format_from_mime (audio container routing)
      cert.rs          — CertFormat enum (Pem / Der / Jwk; PKCS#12 planned); format_from_ext (`.pem` / `.csr` / `.crl` / `.key` / `.p7b` / `.p7c` / `.pub` → Pem, `.der` → Der, `.jwk` / `.jwks` → Jwk) + sniff_pem (`-----BEGIN ` header / `ssh-rsa…` content) + sniff_der (leading `0x30 0x82` SEQUENCE that fully parses as X509Certificate — parse, not just magic; the one heavy detection dep) + sniff_jwk (`kty` is a known type, or a `keys` array of such — looks_like_jwk inlined here). `.crt` / `.cer` left to content sniff (either encoding)
      comic.rs         — ComicFormat enum + label; format_from_ext: `.cbz` → Cbz
      csv.rs           — CsvFormat enum (Csv / Tsv) + default_delimiter; format_from_ext: `.csv` / `.tsv`
      disk_image.rs    — DiskImageFormat enum (Iso / Dmg / Raw) + label; format_from_ext + Raw → Iso upgrade (cheap 6-byte PVD probe at offset 32768)
      document.rs      — DocumentFormat enum (Docx / Odt / Rtf) + label; format_from_ext + format_from_mime (RTF magic-byte route)
      ebook.rs         — EbookFormat enum; format_from_ext: `.epub` → Epub
      email.rs         — EmailFormat { Eml, Mbox } + label; format_from_ext (eml → Eml, mbox → Mbox) + sniff_text (`From ` separator → Mbox; RFC822 header block with a recognised mail header → Eml; guards against arbitrary `key: value` files)
      eps.rs           — PostScriptFormat { Eps, Ps } + label + crop_to_bbox (EPS → gs -dEPSCrop); format_from_ext (eps/epsf/epsi → Eps, ps → Ps) + format_from_mime (application/postscript → Eps) + sniff_text (`%!PS…`, EPSF token → Eps else Ps) + is_dos_eps (inline 4-byte magic check; the full DOS-EPS header parse stays in the reader's `dos_eps`)
      font.rs          — FontFormat enum (TrueType / OpenType / Collection / Woff / Woff2) + label; format_from_ext (`.ttf` / `.otf` / `.ttc` / `.otc` / `.woff` / `.woff2`) + sniff_font_bytes (4-byte magic: `00 01 00 00` / `true` / `OTTO` / `ttcf` / `wOFF` / `wOF2`)
      objfile.rs       — is_bare_coff: full COFF-header validation (known machine + no optional header + sane section count + executable flag clear) so bare `.obj` routes here without misclaiming Wavefront 3D `.obj`. No format enum — `ObjectFile` is a unit `FileType` variant
      pdf.rs           — PdfFlavor { Pdf, Illustrator } + label: `.ai` is PDF-compatible Illustrator (same render path); flavour drives the Info label + extension-mismatch allow-list. No detect helpers — PDF is magic-routed in the orchestrator
      spreadsheet.rs   — SpreadsheetFormat { Xlsx, Xlsm, Ods } + label + is_ooxml (picks docProps/core.xml vs meta.xml + the calamine reader); format_from_ext + format_from_mime (magic is application/zip → extension-routed like docx)
      sqlite.rs        — SqliteFormat enum (single Sqlite variant today; SQLCipher / WAL flavours would slot in); format_from_ext (`.sqlite` / `.sqlite3` / `.db` / `.db3`) + format_from_mime (`application/vnd.sqlite3` / `application/x-sqlite3`)
      structured.rs    — StructuredFormat enum (JSON / JSONC / JSON5 / JSONL / YAML / TOML / XML); format_from_ext. (JSON / SVG / HTML / XML / YAML content sniff for unnamed sources lives in the orchestrator's detect_bytes)
      vobject.rs       — VObjectFormat { ICal, VCard } + label (iCalendar / vCard); format_from_ext (ics/ical/ifb → ICal, vcf/vcard → VCard) + sniff_text (leading `BEGIN:VCALENDAR` / `BEGIN:VCARD` marker, BOM/blank-line tolerant)
  peek-theme/          — theming leaf crate. Depends on nothing in-tree (parallel to peek-io). Aliased into the bin as `crate::theme` via `use peek_theme as theme` (façade like `src/input/`).
    src/lib.rs         — re-exports ThemeManager / PeekThemeName / load_embedded_theme / PeekTheme / lerp_color / ActiveStyle / Attr / Sgr / StyleMode / display_width / scan
    src/name.rs        — PeekThemeName + embedded .tmTheme data (include_str! over ../themes/) + load_embedded_theme + ValueEnum impl
    src/sgr.rs         — low-level SGR escape mechanics (color encoders, Attr, ActiveStyle, display_width/scan tokenizer)
    src/style_mode.rs  — StyleMode (truecolor/256/16/grayscale/plain) + RGB→palette conversion + ValueEnum impl
    src/peek_theme.rs  — PeekTheme semantic roles + paint helpers + lerp_color/blend + rgb↔hsl + search-match colors
    src/manager.rs     — ThemeManager: shared SyntaxSet/ThemeSet + active PeekTheme
    themes/            — Embedded .tmTheme files (idea-dark default + vscode-dark-modern / vscode-dark-2026 / vscode-monokai)
src/
  main.rs              — CLI entry point: dispatches inputs to viewers
  cli.rs               — Args struct (clap derive)
  base64.rs            — shared crate-wide base64 codec: decode (standard + URL alphabet, both tables in one) + decoded_len + encode_url (URL-safe, no padding — for the JWK thumbprint); hand-rolled, no crate dep; first consumer was the notebook image extractor
  update.rs            — `--update` flow: GitHub Releases check + pipe install.sh into sh
  xml.rs               — shared XML helper: `unescape_attr_value` decodes a quick-xml attribute's raw UTF-8 bytes + resolves escapes via `quick_xml::escape::unescape` (feature-flag-independent — calamine enables quick-xml's `encoding` feature, which removes `Attribute::unescape_value`). Used by docx / odt / epub / structured-xml / spreadsheet props
  input/               — thin façade over peek-io + peek-detect (keeps the historical `crate::input::*` paths) + the CLI-level source dispatch
    mod.rs             — re-exports peek-io (InputSource, ByteSource, LineSource, source, stream) and peek-detect (as `detect`, `mime`, and `compression::resolve_transparent`) under `crate::input::*`
    stdin.rs           — build_source(&Args): pick file vs stdin from the CLI args (delegates the actual stdin read + tty reopen to peek_io::stdin)
  extract/
    mod.rs             — Module declarations + re-exports (Extracted, ExtractOptions, ExtractError, extract, sanitize_entry_path)
    extract.rs         — Top-level dispatch (FileType → per-type extractor) + Extracted/Options/Error types + path sanitiser
    write.rs           — Output enum + write_extracted: streams to stdout or writes file at path
  output/
    mod.rs             — re-exports PrintOutput
    print.rs           — PrintOutput: write-once stdout for --print / pipes / --info
    help.rs            — CLI help and version screens
  info/
    mod.rs             — FileInfo + InfoExtras trait + Extras (`Box<dyn InfoExtras>` payload) + impl_info_extras! macro + test-only downcast_extras + shared permission helpers
    gather/            — FileInfo collection, split per general file type
      mod.rs           — Per-source dispatch (gather() entry point)
      tests.rs         — Fixture-based tests against test-images / test-data
    render/            — Themed terminal rendering of FileInfo, split per section
      mod.rs           — render() entry, RenderOptions, shared push_field/section_header/paint_count
      file.rs          — File section: name, path, size, MIME, timestamps, permissions
    time.rs            — UTC ISO / local-with-offset timestamp formatting (libc::localtime_r)
  theme                — alias for the `peek-theme` crate (see crates/ above); `use peek_theme as theme` keeps `crate::theme::*` paths working
  types/
    mod.rs             — Per-file-type modules (each owns reader + info + view-mode)
    info_impls.rs      — Central registry: one impl_info_extras! row per type binding its stats struct to the `info::InfoExtras` trait (replaces the old FileExtras enum + render match). Only `types → info` edge is the trait itself
    binary/
      mod.rs           — Module wiring
      info.rs          — BinaryInfo struct + gather_extras (friendly format label) + render_section (Format)
    text/
      mod.rs           — Module wiring
      info.rs          — TextStats / LineEndings / IndentStyle / Encoding (shared shape: markdown / sql / svg import from here)
      info_gather.rs   — gather_text_stats: streaming UTF-8/UTF-16 stats (lines/words/encoding/indent/shebang)
      info_render.rs   — render_section + push_text_stats: Content/Source section content
    markdown/
      mod.rs           — Module wiring + MarkdownRenderer re-export
      compose.rs       — Compose: RenderedTextMode (default, --raw inverts) + ContentMode source view
      renderer.rs      — MarkdownRenderer: TextRenderer impl that reads source and dispatches to render::render; theme_name comes via the render() arg so cached lines invalidate on theme cycle
      info.rs          — MarkdownInfo { text: TextStats, stats: MarkdownStats } + MarkdownStats + FrontmatterKind
      info_gather.rs   — Single-pass MD stats: headings by level, fenced blocks + langs, links/images/tables/lists, task progress, frontmatter, prose word count, reading time
      info_render.rs   — Render Markdown info section (Content + Markdown blocks)
      render/
        mod.rs         — Entry: render(text, width, theme, style_mode, tm, theme_name) → Vec<String>. Splits frontmatter, runs pulldown-cmark with GFM options, feeds the walker
        walker.rs      — Event-stream walker: container stack (List/Item/Blockquote) + leaf block (Paragraph/Heading/CodeBlock) + table builder. Inline styling for emph/strong/strike/code/links/images, task-list marker swap, footnote ref + def, frontmatter dim block, tight-vs-loose list spacing
        table.rs       — GFM tables → box-drawing. Column widths sized to widest cell then proportional shrink to fit available width; per-column alignment from header separator; cell wrap with SGR preserved
        wrap.rs        — wrap_with_prefix (rebuild prefix on each wrapped row) + display_width (counts text tokens, ignores SGR)
    notebook/
      mod.rs           — Module wiring + NotebookRenderer / NotebookInfo re-exports
      model.rs         — serde_json::Value walker → Notebook { nbformat, language, kernel, cells }; tolerant of nbformat 3 (worksheets/input) vs 4; collapses output mime bundles to one Output (Stream/Text/Image/Html/Error); strip_ansi for tracebacks
      renderer.rs      — NotebookRenderer: TextRenderer that translates the notebook to one Markdown doc (code cells → fenced blocks in kernel lang, outputs → fenced text / notes) then reuses markdown::render_markdown for highlight + wrap
      listing.rs       — Raw-JSON walk → flat Blocks TOC entries (code-N.<ext> / image-N.<ext>) + extract_block resolution (decodes base64 images via crate::base64, code/SVG verbatim). Off the model/render path so image bytes never load during render
      extract.rs       — Resolve a Blocks key to an in-memory source named after the block; outer re-detect renders it (code highlighted / image drawn), so descend needs no notebook-specific frame logic
      compose.rs       — Compose: RenderedTextMode (default, --raw inverts) + structured-JSON ContentMode source view + Blocks ListingMode (when code/images present)
      info.rs          — NotebookInfo (nbformat, kernel/language, cell + output tallies, max execution count) built from a parsed Notebook
      info_gather.rs   — Parse notebook → NotebookInfo (None falls back to text/binary gather)
      info_render.rs   — Render Notebook info section
    sql/
      mod.rs           — Module wiring
      info.rs          — SqlInfo { text: TextStats, stats: SqlStats } + SqlStats + SqlDialect
      info_gather.rs   — Statement scanner with string/comment/dollar-quote state; classifies DDL/DML/DQL/TCL, records created objects, guesses dialect
      info_render.rs   — Render SQL info section (Content + SQL blocks)
    css/
      mod.rs           — Module wiring
      info.rs          — CssInfo { text: TextStats, stats: CssStats } + CssStats + SelectorKindCounts / CssImport / ColorSwatch
      info_gather.rs   — cssparser rule/declaration-trait scanner: CssScanner drives StyleSheetParser + RuleBodyParser → rule/selector/at-rule counts, @import URLs, deduped colour palette. Colours scanned only inside declaration values (cssparser-color) so selectors / strings / comments never false-match; CSS nesting counted
      info_render.rs   — Render CSS info section (Content + CSS blocks + Colors swatch grid)
    cert/
      mod.rs           — Module wiring
      compose.rs       — compose(fmt): PEM → paired Source ContentMode (no syntax token); JWK → structured JSON content mode (pretty + highlight, reusing FileType::Structured(Json)); DER → no source view (binary), just the universal Info + hex tail. Info aux mode renders the cert sidecar in every case
      info.rs          — CertInfo { text: Option<TextStats> (None for binary DER), source_label: &'static str ("PEM" / "DER" / "JWK"), entries: Vec<CertEntry>, parse_errors: Vec<String> } + CertEntry variants (Certificate / CSR / CRL / PrivateKey / PublicKey / SshPublicKey / JsonWebKey / Unknown — heavy variants boxed) + per-entry shapes (incl. JwkEntry { kty / crv / alg / use / kid / key_ops / key_size_bits / thumbprint }) + KeyType (Rsa / Ec(curve) / Ed25519 / Dsa / Other)
      info_gather.rs   — gather(text): pem::parse_many → per-block dispatch by PEM label. gather_der(der): label-less DER recovered by structure (classify_der tries cert → CRL → CSR → PKCS#8 / SPKI key, first decode wins, else Unknown). gather_jwk(text): serde_json → jwk::parse. X.509 cert / CSR / CRL via x509-parser; SSH pubkey lines (outside any PEM fence) via ssh-key. Keys: hand-rolled ASN.1 TLV walker over PKCS#1 / SEC1 / PKCS#8 / SPKI envelopes recovers key type + bit size without a fourth crypto crate. SHA-1 + SHA-256 fingerprints over the cert DER (sha1 / sha2)
      jwk.rs           — JSON Web Key parse (RFC 7517/7518) + thumbprint (RFC 7638): parse(value) → Vec<JwkEntry> (single key or `keys` set); looks_like_jwk (detection helper); key_size_bits (RSA modulus bits / EC-OKP curve bits / oct secret bits); thumbprint (SHA-256 over canonical required-member JSON, base64url via crate::base64). serde_json + sha2 only
      info_render.rs   — Render cert info section (Content text-stats block only when `text` is Some; entries section headed by source_label, per-entry blocks: cert Subject / Issuer / Serial / validity / Public Key / SANs / fingerprints; JWK Type / Bits / Algorithm / Use / Key Ops / Key ID / Thumbprint). Days-Left ≤ 30 painted as warning; expired painted as warning with negative day count
    font/
      mod.rs           — Module wiring
      sfnt.rs          — decode(bytes, format) → Cow<[u8]>: single entry point handing every consumer raw sfnt. Bare TrueType/OpenType/Collection borrow through (no copy); Woff unwraps in-tree (woff.rs), Woff2 delegates to the `wuff` crate. Called first by both info_gather (font_gather) and compose
      woff.rs          — decode(): WOFF 1.0 → sfnt. Rebuilds the offset table + directory, inflates each table (zlib via in-tree flate2, or copies verbatim when stored uncompressed), 4-byte-aligns table data. Bounds-checked against hostile input; metadata/private blocks dropped. No new dep. (WOFF2's brotli + glyf/loca transform is handled by `wuff` in sfnt.rs, not here)
      compose.rs       — compose(): unwrap to sfnt (sfnt::decode) → rasterise specimen → SpecimenMode (primary view). Best-effort — a font fontdue can't parse (or a malformed WOFF) skips the specimen push and falls through to the Info + Hex tail
      info.rs          — FontInfo { format, face_count, faces, parse_errors } + FaceInfo (family / subfamily / postscript_name / version / OS/2 weight + width / italic / monospaced / glyph_count / units_per_em / codepoint_count / scripts / hinting / designer / vendor / copyright / license_url)
      info_gather.rs   — ttf-parser Face walk: read_name_table (UTF-16BE + Mac Roman decoders — Apple system fonts still ship Macintosh-platform records as canonical, so the full Mac Roman upper-half mapping is bundled here) + head_flags (hinting bit) + scan_cmap (Unicode codepoint count + 12-bucket script coverage from cmap ranges). Every face in a collection is gathered; face_count() exposed so the compose path can size SpecimenMode's cycle range without re-parsing
      info_render.rs   — Render Font info section (Format + Faces count for collections, then a per-face block per FaceInfo). Weight painted as `<class> (<name>)` for canonical OS/2 weights, bare number otherwise; empty name-table fields skip their row entirely
      specimen.rs      — rasterise(bytes, face_index, target_height_px) → DynamicImage. fontdue rasterises each glyph of a hard-coded sample (pangrams + digits + ASCII alphabet) at a derived font size, blits them into a white RGBA8 canvas with baseline alignment. Coverage values darken the destination per glyph; the existing image pipeline downsamples + composites
      specimen_mode.rs — SpecimenMode: parallel to image::ImageRenderMode but owns a pre-decoded DynamicImage instead of a source. Same ImageView wiring (cycle background / image-mode / fit, FitHeight horizontal pan, single-slot cache invalidated on resize / margin / bg / fit change). For collections, holds the original `Bytes` + face count + current face index; `n` / `p` (NextFace / PrevFace) re-rasterise the next face in place with wrap, status segment surfaces `Face N/M`. Pipe path uses capped_for_image_pipe so font specimens don't dominate piped output
    structured/
      mod.rs           — Module wiring
      info.rs          — StructuredInfo / StructuredStats / TopLevelKind + gather_extras (per-format stats) + render_section (Format)
      pretty.rs        — JSON / YAML / TOML / XML pretty-printers (used by ContentMode)
    csv/
      mod.rs           — Module wiring; re-exports CsvStats
      parse.rs         — Streaming, bounded-memory CSV reader over `csv::Reader<Box<dyn ReadSeek>>`. Two resident tiers: a retained `seed` (first 1000 records → widths + header heuristic + type sample + top-of-file rows) and a sliding `window` (WINDOW_SIZE records) for everything past it. Window refills seek the reader to the nearest sparse `anchors` entry (one `csv::Position` per ANCHOR_STRIDE=256 records) and re-parse forward. `total` is unknown until `ensure_all` runs a count pass that discards cells (O(1) mem); `loaded()` = total-or-frontier. malformed counted once per record (gated on first discovery). UTF-16 LE/BE transcoded eagerly to a fully-resident UTF-8 Cursor (window bound N/A there). Malformed guard: > 4 MiB per record OR > 10 000 physical lines OR csv-crate error → `<error>` row + counter; reader resyncs on next newline.
      compose.rs       — compose(): RowsTableMode (via build_csv_mode — wraps CsvData in a Box<dyn RowSource>) + paired Source ContentMode + infer_alignments helper (Int/Float seed-body inference, shared with the table-mode tests)
      info.rs          — CsvStats { format, delimiter, encoding, has_bom, header_detected, columns: Vec<ColumnStats>, loaded_records, total_records, malformed_count, sampled } + ColumnStats / ColumnType (Int/Float/Bool/Date/String/Mixed)
      info_gather.rs   — gather: per-column type inference + width / empty counts over the seed sample
      info_render.rs   — render_section (CSV + Columns blocks)
    sqlite/
      mod.rs           — Module wiring
      reader.rs        — SqliteReader: read-only `rusqlite::Connection`; spools Memory / FileRange / TempFile sources to a `NamedTempFile` held in an `Arc` for the connection's lifetime so piped DBs work too
      catalog.rs       — sqlite_master walker → SqliteCatalog { tables, views, indexes, triggers } with `Entity { name, tbl_name, sql, row_count }`; internal `sqlite_*` tables filtered out, COUNT(*) attached per table/view
      compose.rs       — compose(): builds the Entry tree (kind-grouped Dirs, `<name>.sql` for every entity, `<name>.csv` extra leaf for tables/views), pushes a ListingMode with a descend handler installed via `with_descend_handler`. Handler parses `<kind>/<name>.csv` rows and pushes a streaming SqliteTableMode + Info + About frame over the current DB; `.sql` / non-row-bearing rows return None → standard extract path. Holds `parse_contents_key` + `build_contents_frame`
      row_set.rs       — SqliteRowSet impl RowSource: 1000-row sliding window over `SELECT * FROM "<entity>" LIMIT/OFFSET`, synthetic header row at index 0 holding column names from PRAGMA table_info, cached COUNT(*) so total() is definite up front. NULL → None, BLOB → `<blob: N bytes>`. Alignment from declared type affinity (INT / REAL / NUMERIC right; CHAR / TEXT / DATE / BOOL left)
      table_mode.rs    — Thin constructor: opens SqliteRowSet for `<entity>`, derives alignments, wraps in RowsTableMode with has_header=true
      extract.rs       — Extract handler. `<kind>/<name>.sql` queries `sqlite_master.sql` and returns an in-memory `.sql` source with a `-- <name> from <db>` header so the outer re-detect routes it through the SQL syntax view. `<kind>/<name>.csv` streams `SELECT * FROM "<entity>"` through `csv::Writer` into a `NamedTempFile` (returned as `InputSource::TempFile`): NULL → empty, INTEGER / REAL / TEXT → display form, BLOB → SQL hex literal `X'…'` (lossless, round-trippable into INSERT). Indexes / triggers stay schema-only; the parser rejects `.csv` for them defensively
      info.rs          — SqliteInfo { stats: Option<SqliteStats>, error } + SqliteStats { page_size, page_count, encoding, schema_version, user_version, application_id, journal_mode, integrity_ok, table/view/index/trigger counts, total_rows, top_tables }
      info_gather.rs   — gather_extras: opens SqliteReader, scrapes `PRAGMA page_size/page_count/encoding/schema_version/user_version/application_id/journal_mode/integrity_check`, attaches the catalog and sorts the biggest tables for top_tables. Errors fold into `SqliteInfo::err` so the info panel always renders
      info_render.rs   — render_section: SQLite block (page / encoding / journal / integrity / counts / total rows) + Biggest tables block (omitted when there are no row-bearing tables)
    spreadsheet/         — `.xlsx` / `.xlsm` / `.ods` workbooks. Same shape as sqlite (sheets → listing → drill into a streaming table)
      mod.rs           — Module wiring; re-exports SpreadsheetInfo
      workbook.rs      — calamine wrapper: opens via a `Cursor<Bytes>` (whole container read in for random access; per-format `Reader::new` dispatch avoids the Clone-requiring auto-opener); `sheet_names()` cheap, `materialize(name)` parses one sheet's Range whole. `Sheet` impls RowSource (resident rows, total known); alignment from native Data variants (Int/Float → right), header via all-text row-0 heuristic. cell_string maps Data→Option<String> (Empty→None)
      compose.rs       — compose(): Sheets ListingMode (`<sheet>.csv` rows) + descend handler → RowsTableMode + Info + About frame per sheet; secondary ZIP-entry ListingMode via archive::reader::list_entries. SHEET_SUFFIX shared with extract
      extract.rs       — `<sheet>.csv` keys → materialize + stream the sheet through csv::Writer into a NamedTempFile; any other key is a raw container path → delegate to archive::extract (ArchiveFormat::Zip)
      xml_props.rs     — read core document properties from the zip: docProps/core.xml (OOXML) or meta.xml (ODS), parsed by one Dublin-Core reader keyed on prefixed element names covering both vocabularies → DocumentMetadata
      info.rs / info_gather.rs / info_render.rs — SpreadsheetInfo { format, sheets, metadata, error }; gather lists sheets + reads metadata; render shows Sheets count / Names / core props
    image/
      mod.rs           — Module wiring; re-exports ImageRenderMode, AnimationMode + the foundation `image_render` geometry modules (scroll/zoom/zoom_pan) at the old paths
      compose.rs       — compose(): push AnimationMode for animated GIF/WebP, ImageRenderMode for static raster
      info.rs          — ImageStats + AnimationStats + LoopCount (animation summary)
      info_gather.rs   — gather_extras (dimensions, color, ICC, HDR) + IMAGE_HEAD_SCAN/read_head
      info_render.rs   — render_section (Image, EXIF, XMP, Animation)
      extract.rs       — Animation frame extract (GIF/WebP): decode all frames, re-encode frame N as PNG (Memory-backed)
      exif.rs          — EXIF field extraction
      xmp.rs           — XMP packet scrape (Dublin Core / xmp tags)
      animation_stats.rs — GIF/WebP animation stats (frames, duration, loop)
      view.rs          — ImageView: shared image-grid scroll + zoom + cycleable config for every Mode that scrolls through a PreparedImage (ImageRenderMode + AnimationMode + SvgAnimationMode + SpecimenMode). Holds (config, scroll_x, scroll_y, zoom); exposes view_bounds (effective grid - viewport per axis), render_prepared (clamp pan + zoom dispatch + render), pipe_snapshot/restore (force-Contain + zoom=1 wrapper for `--print`), scroll, handle_config_cycle (b/m/f keys + pan reset on fit change), handle_zoom (+/-/0/1-9 with viewport-centre anchor), status_segments
      anim_frame.rs    — AnimFrameState: shared frame-playback state (current / playing / last_advance) for animated image Modes (AnimationMode + SvgAnimationMode). play_pause / step / tick / next_tick / status_segment / extract_target
      mode.rs          — ImageRenderMode: static raster + rasterized SVG view; embeds ImageView, owns InputSource + single-slot CachedFrame
      animation_mode.rs — AnimationMode: GIF/WebP playback (next_tick / tick driven); embeds ImageView + AnimFrameState, owns decoded frame list (no per-frame cache — frames change every tick)
      paged_render.rs  — render_image_window: shared single-bitmap decode→fit→window-crop→ASCII (zoom fast path + zoomed path) every PageRenderer (PDF/CBZ/EPS) defers to. Lives here (beside the engine it drives) not in foundation `viewer::paged`; takes PagedRender/RenderArgs back from the foundation. zoom/scroll/zoom_pan geometry + the render-config vocab (ImageMode/Background/FitMode/ImageConfig/TermSize) now live in `viewer::image_render`; re-exported at the old `image::{zoom,scroll,zoom_pan}` / `pipeline::*` paths
      pipeline/        — Rasterization → ASCII-art rendering core
        mod.rs         — Module wiring; re-exports the render-config vocab from `viewer::image_render`
        render.rs      — Image → glyph-matched ASCII art with true color (TermSize re-exported from `viewer::image_render`)
        animate.rs     — GIF/WebP frame decoding + frame counting + render_frame
        glyph_atlas.rs — Precomputed glyph bitmaps + atlas indexing
        glyph_atlas_data.rs — Generated companion to `glyph_atlas.rs`: the raw bitmap data table (kept in its own file so the API stays in `glyph_atlas.rs` and the codegen blob doesn't drown it)
        clustering.rs  — Two-color clustering for cell rendering
        contour.rs     — Sobel + Otsu edge detection for ImageMode::Contour
        svg.rs         — SVG rasterization (resvg): svg_dimensions / rasterize_svg
        svg_anim/      — CSS `@keyframes` SVG parser + per-frame rasterizer
          mod.rs       — Public API: try_parse / try_parse_bytes / render_frame
          scan.rs      — quick-xml walk: byte-span collection of animated elements + <style>
          spec.rs      — Inline-style `animation-*` parser → AnimSpec
          keyframes.rs — CSS @keyframes rule parser → KeyframeStop, TransformValue
          timeline.rs  — Merged frame timeline: build_frames, sample_target (steps + linear)
          marker.rs    — __PEEK_ANIM_*__ marker injection + per-frame substitution
          selectors.rs — Flat CSS selector parser (`Class` / `Id` / `Tag` / `TagClass`) for `@keyframes` rule targets; combinators / pseudo-classes / attribute selectors / `*` are detected and dropped (matcher API stays tiny)
          util.rs      — Shared helpers: skip_ws, find_substr/brace, parse_length, root_svg_dimensions
    html/
      mod.rs           — Module wiring; re-exports HtmlRenderer
      compose.rs       — compose(): push RenderedTextMode<HtmlRenderer> + paired HTML source ContentMode
      renderer.rs      — HtmlRenderer: TextRenderer impl reading source bytes through `render::render`; the generic RenderedTextMode owns caching / search / windowing
      render.rs        — Shared html2text driver: bytes → ANSI lines via StyleMode (also used by EPUB chapters). CSS via html2text `use_doc_css`; near-grayscale colours filtered to avoid fighting terminal foreground
    ebook/
      mod.rs           — Module wiring; re-exports EbookStats / Metadata
      compose.rs       — compose(): push EpubReadMode (chapters) + ZIP TOC ListingMode; OPF failure leaves listing-only
      info.rs          — Shared ebook info shape (universal across EPUB / MOBI / FB2): EbookStats { metadata: Metadata, chapter_count }
      epub/
        mod.rs         — Module wiring; re-exports EpubReadMode
        package.rs     — Parse EPUB ZIP: META-INF/container.xml → OPF rootfile → DC metadata (into shared Metadata) + manifest (id→href) + spine; resolve spine to absolute ZIP paths; ZIP entry reader
        read_mode.rs   — EpubReadMode: one chapter at a time via shared html `render`. Per-chapter render cache keyed by (idx, width); n / N step chapter (Action::NextChapter / PrevChapter). render_to_pipe walks the whole spine. Pre-processes `<img>` tags to inject `alt="image: <basename>"` for empty / missing alt so chapter image refs stay visible. Cover-style chapters (≤ 3 non-empty rendered lines + at least one `<img>`) render the first image as ASCII via the image pipeline
        info_gather.rs — Populate EbookStats (DC metadata + chapter count) from package::open
        info_render.rs — Render EPUB info section from EbookStats
    email/
      mod.rs           — Module wiring; re-exports EmailInfo
      compose.rs       — compose(fmt): `.eml` → [rendered Message (unless --plain) + raw Source + Attachments ListingMode]; `.mbox` → Messages ListingMode with descend handler (subrange one message → reuse the .eml stack) + raw Source. Source precedes the attachments listing so the print/pipe first-data-mode pick is the message, not the listing
      message.rs       — Single parse site over mail-parser: parse(bytes) → owned ParsedEmail { from/to/cc/subject/date/message_id, Body (Html preferred over Text), attachments }; attachment_base() (declared filename or `attachment-N.<ext>`) + dedupe_keys() (unique names pass through clean for CLI extract, collisions get a `-N` stem suffix) shared with the extractor; content_type() helper
      mbox.rs          — split(bytes) → Vec<MboxEntry { offset, len, subject, date_secs }> by scanning `From ` line-start separators; ranges point past the separator so each slice parses standalone; lightweight Subject + Date scan (Date via mail_parser::DateTime::parse_rfc822 → epoch) avoids a full parse per row
      renderer.rs      — EmailRenderer: TextRenderer rendering a themed header block + body (HTML via html::render::render, plain text word-wrapped); re-parses per render like HtmlRenderer (RenderedTextMode caches)
      extract.rs       — Resolve an attachment key to an in-memory source (re-parse, match part by recomputed key, copy decoded contents); NotFound on miss
      info.rs          — EmailInfo (header summary + attachment count/bytes for .eml, message count for .mbox) + gather_extras(source, fmt) (None falls back to text/binary gather)
      info_render.rs   — Render Email info section (header rows truncated; Messages count for mbox)
    vobject/
      mod.rs           — Module wiring; re-exports VObjectInfo. iCalendar (.ics) + vCard (.vcf): the two IETF vObject text formats, one shared content-line parser, format-specific renderers
      compose.rs       — compose(fmt): rendered read view (CalendarRenderer for ICal / ContactRenderer for VCard) unless --plain, then raw Source via text_content_mode. No inner items
      line.rs          — Shared hand-rolled content-line parser (no dependency): unfold (line folding + CRLF/LF), parse_line (NAME;PARAM=VAL:VALUE, quoted-param-aware colon/`;` splitting), parse_components (BEGIN/END → Component tree). ContentLine { name, params, value } + Component { name, props, children } with param/value/props_named accessors; unescape_text, split_structured (`;` fields), format_list (`,` list → ", "-joined)
      datetime.rs      — format_datetime (ISO-basic / dashed / UTC-Z / date-only → `YYYY-MM-DD HH:MM`, raw passthrough on no match) + date_key (sortable YYYY-MM-DD). Pure string reshaping, no date crate / no zone math
      calendar.rs      — CalendarRenderer: TextRenderer rendering a VCALENDAR agenda (name header + per-VEVENT/VTODO blocks: when-span, location, humanised RRULE, organizer/attendees, status, categories, description). humanize_rrule (FREQ/BYDAY/COUNT/UNTIL/INTERVAL) + format_when (same-day end-date collapse). summarize(text) → CalendarSummary (name/version/product, event+todo counts, date range) for Info
      contact.rs       — ContactRenderer: TextRenderer rendering one grouped card per VCARD (FN-or-N name, org/title, every email/phone/address with TYPE annotation, web/birthday/categories/note). v3 + v4 handled together. summarize(text) → ContactSummary (count + leading version)
      render.rs        — Shared rendered-view helpers: push_field (Label: value with hanging-indent wrap) + push_prose (wrapped multi-line block). Same visual grammar as the email header block
      info.rs          — gather + render in one (tiny type): VObjectInfo { format, Detail::{Calendar(CalendarSummary)|Contact(ContactSummary)} }; gather_extras(source, fmt) (64 MiB cap, None falls back to text/binary) + render_section
    eps/
      mod.rs           — Module wiring; re-exports EpsInfo; `postscript_text(bytes, header)` helper (PS section slice for DOS-EPS, whole file otherwise; lossy UTF-8)
      compose.rs       — compose(): [Preview: PagedImageMode<EpsImageRenderer> when a DOS-EPS TIFF preview exists] + [Render: PagedImageMode<EpsImageRenderer> when gs::find() succeeds, lazy] + Source (text_content_mode over the PS-section slice). `--plain` drops both image views. First-pushed = default, so Preview leads when present
      dos_eps.rs       — Binary DOS-EPS container parse: MAGIC `C5 D0 D3 C6`, 30-byte LE header → PostScript Section + optional preview (TIFF preferred over WMF); offsets clamped to file length
      dsc.rs           — DSC comment parser: line-prefix scan to `%%EndComments`/body → DscInfo { title, creator, creation_date, for_whom, bounding_box, language_level, pages }
      gs.rs            — Optional Ghostscript bridge (never bundled): find() probes gs/gswin64c/gswin32c on PATH via `--version`; render(exe, postscript, crop_to_bbox) pipes PS on stdin → png16m on stdout (-dSAFER, page 1, 150 DPI), decodes via image crate
      image_renderer.rs — EpsImageRenderer: PageRenderer (1 page) over EpsImageSource { Preview(Arc<DynamicImage>) decoded eagerly at compose (so an undecodable TIFF never becomes a dead tab) | Ghostscript { exe, postscript, crop_to_bbox } rendered lazily on first draw }; single-slot cache of the render *outcome* (Ok or Err) so a failing gs render runs exactly once, not per redraw; defers to render_image_window
      info.rs          — EpsInfo { format, dsc: DscInfo, preview: Option<PreviewMeta { kind, bytes, dimensions }>, gs_available }
      info_gather.rs   — Populate EpsInfo: parse DOS header → preview meta (+ best-effort TIFF dims), DSC from PS section, gs::find()
      info_render.rs   — Render section (header = format.label()): DSC fields + Preview (kind + dims / "none") + Render (Ghostscript / install hint)
    document/
      mod.rs           — Module wiring; re-exports DocumentStats / DocumentMetadata / DocRenderer
      compose.rs       — compose(): DOCX/ODT → RenderedTextMode<DocRenderer> + ZIP TOC ListingMode; RTF → RenderedTextMode<RtfRenderer> + inline-embed listing when any \pict groups parsed
      ast.rs           — Shared word-processing AST (Doc / Block::{Paragraph,Table} / Paragraph / Run + count_words + merge_paragraphs). Populated by both docx::package and odt::package; RTF stays separate because its on-the-wire shape is a flat painter-tagged text stream
      render.rs        — Shared render(&Doc, width, theme, style_mode) -> Vec<String>: width-aware word wrap, per-run SGR (bold/italic/underline/strike + custom fg color), heading bold + theme.heading colour, bullet prefix "• ", table rows joined " | ". Used by both DOCX and ODT
      renderer.rs      — DocRenderer: TextRenderer impl over the shared AST via render::render. Format-agnostic; per-format wiring only supplies the parsed Doc
      wrap.rs          — Shared word-wrap primitives used by both `render` (DOCX/ODT) and `rtf::render`: `split_words` (whitespace tokeniser), `visible_width` (unicode-width), `SgrStyle` (bold/italic/underline/strike + Option<Color>), `emit_styled` (open/close SGR bracketing). Wrap engines themselves stay branched — DOCX builds Vec<Vec<Run>> over a paragraph/run tree, RTF emits inline over a flat painter-tagged stream
      info.rs          — Shared document info shape (DOCX / ODT / RTF): DocumentStats { format, metadata, paragraph_count, word_count, image_count } + DocumentMetadata { title / creator / subject / description / keywords / created / modified }
      info_render.rs   — Render shared Document info section keyed off `format` label
      docx/
        mod.rs         — Module wiring
        package.rs     — Hand-rolled `quick_xml` event walk over `word/document.xml` (paragraph / pPr / pStyle / numPr / r / rPr / b / i / u / strike / color / t / br / tab / drawing-blip), `docProps/core.xml` (DC + cp metadata), and `word/_rels/document.xml.rels` (image rId → basename). Produces shared `ast::Doc`. Hand-walking instead of going through a full WordprocessingML deserializer (`docx-rust` / `docx-rs`) — both reject real-world Word files because numeric attributes routinely carry `"auto"` / `"none"` / `"true"` strings their strict integer types can't decode
        info_gather.rs — Populate DocumentStats via package::open (paragraph / word / image counts + metadata)
      odt/
        mod.rs         — Module wiring
        package.rs     — Hand-rolled `quick_xml` walk over `content.xml`: pre-scans `<office:automatic-styles>` (and `<office:styles>`) into a style-name → run-attrs table, then resolves `<text:span text:style-name=…>` references during the body walk. Heading level from `<text:h text:outline-level=N>`; falls back to deriving from "Heading_20_N" style names when authoring tools encode headings as styled `<text:p>`. `<draw:image xlink:href="Pictures/…">` → `[Image: <basename>]` placeholder run. `<text:list>` nesting depth drives indent. `meta.xml` Dublin Core + meta:* metadata; multi-valued `<meta:keyword>` entries comma-joined. Produces shared `ast::Doc`. styles.xml inheritance is intentionally not consulted in v1
        info_gather.rs — Populate DocumentStats via package::open
      rtf/
        mod.rs         — Module wiring; re-exports RtfRenderer. RTF stays outside the shared AST because its on-the-wire shape is a flat painter-tagged text stream, not a paragraph/run tree
        parse.rs       — Pre-process RTF (strip `{\info ...}` group, inject `\\\n` after each `\par` so rtf-parser's lexer emits CRLF) → RtfDocument::try_from → owned Vec<Block { painter, paragraph, text }> with painter resolved against \colortbl. Hand-scans `\info` group bytes for title / author / subject / keywords / creatim / revtim
        render.rs      — render(&Parsed, width, theme, style_mode) -> Vec<String>: wraps StyleBlock.text by width, emits SGR for painter bold/italic/underline/strike + colortbl color
        renderer.rs    — RtfRenderer: TextRenderer impl over the parsed RTF stream via render::render
        extract.rs     — Per-embed extract from parsed `\pict` / `\object` groups: key is the synthetic name from `parse::embeds_to_entries` (e.g. `image1.jpg`) → InputSource::Memory (image bytes re-detect through recursive peek)
        info_gather.rs — Populate DocumentStats via parse::open_source
    pdf/
      mod.rs           — Module wiring; re-exports PdfStats, PdfPageRenderer, PdfTextRenderer
      compose.rs       — compose(): PagedImageMode<PdfPageRenderer> (fit forced to FitWidth) + RenderedTextMode<PdfTextRenderer> (only when Doc::has_extractable_text — skipped for scans / outlined `.ai`) + /EmbeddedFiles ListingMode
      package.rs       — Lazy global Pdfium init (exe-dir → .pdfium/lib dev fallback → system); load_pdf_from_byte_vec → Arc-backed Doc with page_count / render_page (RGBA via image feature) / page_text / has_extractable_text (probes first 8 pages) / metadata / list_embeds / read_embed; list_embeds returns one tree under `attachments/<name>` (/EmbeddedFiles) plus `pages/page{N}/image{M}.{ext}` (inline image XObjects); read_embed dispatches by prefix and falls back to `get_raw_image` → PNG re-encode for codecs `get_raw_image_data` doesn't surface as a usable file. PDF date `D:YYYYMMDDHHMMSSZ` → `YYYY-MM-DD HH:MM:SS UTC` formatter
      page_renderer.rs — PdfPageRenderer: PageRenderer impl — rasterizes a page via Pdfium (single-slot bitmap cache, ~4096px cap) then defers to `viewer::paged::render_image_window`. Wrapped in the generic `viewer::paged::PagedImageMode`
      text_renderer.rs — PdfTextRenderer: TextRenderer impl over `Doc::page_text`; pages joined with muted `--- Page N ---` separator; greedy word-wrap with hard-break for over-width tokens. Per-page extract failures degrade to a placeholder line + warning
      extract.rs       — Extract `/EmbeddedFiles` attachment by name → InputSource::Memory; reuses `extract::sanitize_entry_path`
      info.rs          — PdfStats { flavor: PdfFlavor, metadata: DocumentMetadata, page_count, attachment_count (/EmbeddedFiles), image_count (per-page XObjects), encrypted, pdf_version, error: Option<String> }
      info_gather.rs   — Populate PdfStats via package::open_doc; takes the PdfFlavor (carried on both success + error paths); failures land as `error` field rendered as warning row
      info_render.rs   — Render info section, header from `flavor.label()` ("PDF" / "Adobe Illustrator") (Version / Title / Author / Subject / Keywords / Created / Modified / Pages / Attachments). On error, render only `Error: ...` and stop
    comic/
      mod.rs           — Module wiring; re-exports ComicStats / CbzPageRenderer
      compose.rs       — compose(): PagedImageMode<CbzPageRenderer> (paged images) + ZIP TOC ListingMode
      info.rs          — Shared comic-archive info shape (only CBZ ships today; the shape is sized for CBR / CB7 / CBT if they're ever added): ComicStats { format, page_count, total_image_bytes }
      cbz/
        mod.rs         — Module wiring; re-exports CbzPageRenderer
        package.rs     — list_pages: walk ZIP central directory, filter image entries by extension (png/jpg/jpeg/webp/gif/bmp/tif/tiff), skip __MACOSX/, sort by name; open_zip + read_page for body fetch
        page_renderer.rs — CbzPageRenderer: PageRenderer impl — decodes one ZIP image entry (per-page bitmap cache) then defers to `viewer::paged::render_image_window`. Wrapped in the generic `viewer::paged::PagedImageMode`
        info_gather.rs — Populate ComicStats (page count + uncompressed image bytes) from package::list_pages
        info_render.rs — Render comic info section from ComicStats
    svg/
      mod.rs           — Module wiring; re-exports SvgAnimationMode
      compose.rs       — compose(): SvgAnimationMode (CSS keyframes) or ImageRenderMode + paired XML source ContentMode
      info.rs          — SvgStats { text: TextStats, viewBox, element counts, security flags, animation } + SvgAnimationStats
      info_gather.rs   — gather_extras (viewBox, element counts, security flags, animation summary)
      info_render.rs   — render_section (SVG + Source sections)
      extract.rs       — SVG anim frame extract: render_frame → resvg rasterize at intrinsic size (sub-512px upscaled to 512 floor) → PNG
      animation_mode.rs — SvgAnimationMode: CSS `@keyframes` SVG playback (per-frame rasterize + bounded LRU cache); embeds ImageView + AnimFrameState, owns AnimatedSvg model + last_term for scroll clamp
    audio/
      mod.rs           — Module wiring; re-exports AudioStats
      compose.rs       — compose(): Info → optional Cover (ImageRenderMode) → optional Lyrics (ContentMode) → optional Embeds ListingMode
      info.rs          — Shared audio info shape: AudioStats { format, codec, duration_secs, sample_rate, channels, channel_layout, bits_per_sample, bitrate, metadata: AudioMetadata, has_lyrics, has_album_art, error } + AudioMetadata { title, artist, album, album_artist, track_number, disc_number, date, genre, composer, comment }
      package.rs       — Central symphonia probe. `probe(source, format)` → `Probed { codec/track params, AudioMetadata, visuals: Vec<EmbedVisual>, lyrics: Option<String> }`. Walks both `format.metadata().current()` (Vorbis on Ogg/FLAC) and `probed.metadata.get().current()` (ID3v2 sidecar on MP3/AIFF); embedded visuals carried as raw bytes + media_type + canonical `usage_root` (front_cover / back_cover / artist / …). Lyrics joined across USLT/SYLT/`LYRICS=` sources. `to_stats(&Probed)` projects onto AudioStats for InfoMode. `primary_cover(&Probed)` picks the FrontCover-tagged visual (fallback first) for the dedicated Cover tab; `visual_filename` builds its suggested name. `build_listing(&Probed)` synthesises `pictures/<usage>.<ext>` (with `_N` suffix on dup roots) + `lyrics/lyrics.txt`; empty when nothing embedded. `read_embed(&Probed, key)` returns `(Vec<u8>, suggested_name)` for extract. Re-probes per call (header + tag walk, ms-cheap)
      info_gather.rs   — Thin shim: calls `package::probe` + `package::to_stats`; failures land as `error` field
      info_render.rs   — Render Audio + Tags info sections. Tags section omitted when no tag fields populated
      extract.rs       — Per-key extract: `package::probe` → `package::read_embed` → `InputSource::Memory`. Image bytes re-detect as Image (route through ASCII pipeline on recursive peek); lyrics text re-detect as plain text
    archive/
      mod.rs           — Module wiring (no re-exports; consumers reach in via reader / info / extract)
      compose.rs       — compose(): list TOC entries → ListingMode under the format's label
      reader.rs        — list_entries dispatcher (returns Vec<Entry>) + ReadSeek helper + open_seekable (streams File/TempFile/Memory; RangeReadSeek windows a FileRange over its backing file — no slurp)
      info.rs          — ArchiveStats + gather_extras (TOC stats via Stats::from_root) + render_section (Archive info section); static_lib_summary adds a Static library section (object-member count + arch) when an `ar` archive's members are objects
      extract.rs       — Per-format entry extract via materialise(reader, declared_size, opts): entries ≥ SPOOL_THRESHOLD (16 MiB) or unknown size land in InputSource::TempFile ($TMPDIR/peek-*, RAII unlink via Arc<NamedTempFile>); smaller stay in Bytes. --no-tempfile forces Vec path and drops the 256 MiB MAX_EXTRACT_BYTES cap. zip/tar[gz/bz2/xz/zst/lz4/br]/7z/cpio[gz]/ar. Stored zip / uncompressed tar members → zero-copy InputSource::subrange view (no spool). tar/cpio/ar/7z stream the walk over open_seekable (walk_tar; compressed via backends::tar::decode_compressed; 7z via for_each_entries draining preceding solid-block entries) — never reads the whole archive into RAM, matched body streams to spool
      backends/
        mod.rs         — Backend module wiring
        zip.rs         — Zip TOC via central directory (no decompression); returns Vec<FlatEntry>
        tar.rs         — Tar TOC via header walk; decode_compressed (shared by listing + extract): gz/bz2/xz/zst/lz4/br all stream-decompress (xz via liblzma streaming reader, br via brotli-decompressor)
        sevenz.rs      — 7-Zip TOC via sevenz-rust2 (header-only)
        cpio.rs        — cpio TOC via hand-rolled newc (`070701`/`070702`) + ODC (`070707`) header walker. CpioReader state machine drives both list (skip bodies) and extract (read matched body). plain + gz wrappers; old-binary cpio not supported
        ar.rs          — ar(1) reader for `.deb`. ArReader header-chain state machine drives both list and extract (same split as CpioReader). Decodes BSD `#1/<len>` long names; GNU `//` string table unhandled (members shown lossily)
    directory/
      mod.rs           — Module wiring; re-exports DirectoryMode
      compose.rs       — compose(): DirectoryMode rooted at the source path; suppress `..` row at filesystem root
      read.rs          — One-level fs::read_dir → Vec<DirEntry>; sorts dirs-first then case-insensitive name; follows symlinks for kind/size/mtime, broken links surface as `?`
      mode.rs          — DirectoryMode: flat one-level listing. Selects every entry (files + dirs); prepends synthetic `..` row when canonical parent exists. Enter (Action::Descend) targets selected entry. Uses ModeId::Listing so Tab cycle / --list pickup keep working. ViewerState::push_extracted collapses dir→dir descent onto the current frame so there's no stack of directories. Row painting delegated to `viewer::listing::row` (perms/size/mtime/marker) so it stays visually identical to ListingMode by construction
      info.rs          — DirectoryStats + gather_extras + render_section
      extract.rs       — Resolve key (single-segment filename) against parent path → InputSource::File(child_path). `..` walks up via Path::canonicalize → parent. Rejects `/` and `.`
    disk_image/
      mod.rs           — Module wiring (ISO + DMG)
      compose.rs       — compose(): ISO → directory-tree ListingMode; DMG / Raw → InfoMode (no filesystem walker available)
      info.rs          — DiskImageInfo + DiskImageMeta { Iso | Dmg | Raw } + IsoVolumeMeta / IsoDateTime / DmgMeta / DmgPartition / DmgVariant / DmgChecksumKind / RawImageMeta / MbrTable / MbrPartition
      iso_pvd.rs       — Hand-rolled ISO 9660 Primary Volume Descriptor parser + Joliet / El Torito scan + root-extent locator
      iso_listing.rs   — ISO 9660 directory walker → Listing tree (Joliet preferred; depth/entry caps; no Rock Ridge) + lookup_file_range for extract
      dmg_trailer.rs   — Hand-rolled UDIF (Apple Disk Image) "koly" trailer parser (last 512 bytes); carries plist_offset for the partition-map read
      dmg_plist.rs     — quick-xml walk of the embedded plist → blkx entries (Name + decoded mish Data); not a general plist parser, pulls only the partition map
      mish.rs          — UDIF "mish" (BLKX) block-table parser: start sector + sector span + per-chunk (kind, stored length) → MishSummary (offset, logical size, stored bytes, chunk count, run-type histogram + codecs()); structure only, no payload decode
      mbr.rs           — MBR partition-table parser for raw `.img` / `.bin` / `.dd` images; reads the 512-byte boot sector and populates `MbrTable` / `MbrPartition` shown in the Info view
      extract.rs       — ISO entry extract: lookup_file_range → zero-copy FileRange (or Bytes::slice for stdin-piped); DMG returns Unsupported
      info_gather.rs   — gather_extras: ISO reads 16 KiB at offset 32768; DMG reads tail 512 bytes + (if present) the plist region → dmg_plist + mish → DmgPartition rows
      info_render.rs   — render_section (Disk Image info section, ISO + DMG blocks); DMG partition map split into per-filesystem detail blocks + one compact "Partition scheme" block (structural/free entries), classified by is_structural + friendly_type
    objfile/
      mod.rs           — Module wiring
      compose.rs       — compose(): InfoMode landing view + Sections / Symbols TableMode (no extract path)
      load.rs          — Fat-aware load: object::FileKind probe → universal Mach-O slice select (host arch, else first) → object::File::parse; carries the parsed slice bytes; FatSummary lists every slice
      links.rs         — linked_libraries: per-format dependency walk (ELF DT_NEEDED / Mach-O LC_LOAD_DYLIB family / PE import table) — the unified imports() reports symbols, not the soname list
      info.rs          — ObjectInfo { meta: Option<ObjectMeta>, error } + ObjectMeta (semantic `object` enums: BinaryFormat / Architecture / ObjectKind / Endianness — not pre-formatted) + BuildIdKind + linked libraries
      info_gather.rs   — gather_extras: load + capture header counts, build identity (build_id / mach_uuid / pdb_info), and linked libraries into ObjectMeta
      info_render.rs   — render_section (Object File section) + enum→label maps (format / kind / arch / endianness — the one place metadata becomes text); arch_label reused by the static-library summary
      tables.rs        — build(): Sections / Symbols as shared `viewer::table::Table` data (typed Cell + CellRole) via the `object` crate
    classfile/
      mod.rs           — Module wiring
      compose.rs       — compose(): InfoMode landing view + Fields / Methods TableMode + Bytecode disassembly view (no extract path)
      info.rs          — ClassfileInfo { meta: Option<ClassfileMeta>, error } + ClassfileMeta (keeps cafebabe's ClassAccessFlags semantic; render maps it)
      info_gather.rs   — gather_extras: cafebabe parse_class_with_options (bytecode parsing off) → ClassfileMeta
      info_render.rs   — render_section (Class File section) + version / access-flag → label maps
      descriptor.rs    — Render cafebabe descriptor types as syntax-highlighted spans (`(Ljava/lang/String;I)V` → coloured `(String, int) -> void`: primitives / class names / `[]` / punctuation each a CellRole)
      bytecode.rs      — build(): re-parse with bytecode on; Disassembly { methods: MethodAsm[] } of theme-free instruction data (offset / mnemonic / resolved operand) via cafebabe's decoded opcode stream
      bytecode_mode.rs — BytecodeMode: caller-scrolled `javap -c` view; themed lines cached per (width, style, theme) with per-method header anchors; `n`/`p` jump methods (YesScrollTo), `/` searches
      tables.rs        — build(): Fields / Methods as shared `viewer::table::Table` data
  viewer/
    mod.rs             — Registry (holds ComposeOpts), compose_modes (single-file dispatch table delegating to `types::<x>::compose::compose`), ComposeCtx (theme manager / name / plain mode — the `text_content_mode` bundle), ComposeOpts (the 12-field clap-free view of `cli::Args` the compose path reads; built by `Args::compose_opts()` in the bin), free `image_config`. Re-exports highlight_lines / LineStreamHighlighter from `highlight`
    highlight.rs       — Syntect highlighting: highlight_lines (whole-text), LineStreamHighlighter (forward-only streaming feeder used by ContentMode), syntax_token_for (FileType + filename → syntect syntax token, honors `--language`), fallback_syntax_token (extensions syntect doesn't natively support)
    cell_size.rs       — Terminal cell aspect-ratio detection: cell_aspect_h_over_w reads cell pixel dims from TIOCGWINSZ (cached on first call), falls back to 1:2 when the terminal can't report; set_override for an explicit user override. Used by the image pipeline to preserve source aspect across fonts
    image_render/      — Foundation render vocabulary shared by PagedImageMode + cell_size + the types/image engine (moved out of types/image so the shared mode doesn't depend on the reader). The only types/image → here edge is re-exporting these back.
      config.rs        — Background / FitMode / ImageConfig / TermSize value types (render config + terminal dims)
      image_mode.rs    — ImageMode enum (full/block/geo/ascii/contour palette selection)
      zoom.rs          — ZoomLevel: multiplicative zoom factor on top of fit-mode base grid (1.25× per step, 1×..16× clamp, preset/label)
      scroll.rs        — Shared scroll-action handler for image-grid modes: arrows / PgUp / PgDn / Home / End → (scroll_x, scroll_y) deltas with Bounds clamping
      zoom_pan.rs      — ViewBounds + ZoomPanState: zoom/pan state machine (anchor-preserving zoom, pan clamping) over ScrollBounds + ZoomLevel
    interactive.rs     — Unified event loop driving a Vec<Box<dyn Mode>> stack; routes raw keys to active prompt overlay when one is open
    search.rs          — Text-search primitives: smart_case_sensitive, find_matches (exact substring), overlay_matches (paint match backgrounds onto a styled line), SearchState (scan/step/line_overlay/status_segment — shared by every searchable mode), reveal_h_scroll (minimal-pan offset to bring a match on screen) + overlay_window
    wrap_scroll.rs     — WrapScroll: wrap-aware scroll position (logical line / visual sub-row / horizontal pan) + LineView enum (Raw(&LineSource) | Pretty(&[String])). ContentMode's scroll geometry — step / page / clamp / bottom-find over wrapped lines — lives here, branch-agnostic via LineView
    paged.rs           — Shared paged-render mechanism: PageCacheKey / CachedRender / render_cached / step_paged / pipe_rows. Image-config cycling: cycle_image_config handler + the CYCLE_BACKGROUND/IMAGE_MODE/FIT_HELP rows it dispatches, pinned together by a unit test so help and handling can't drift (shared by paged / image / animation / svg-anim / epub modes). Plus PagedImageMode<R> — generic one-page-at-a-time image Mode over the PageRenderer trait (page_count + render_page); ::new defaults the tab label to "Read", ::with_label overrides it (EPS uses "Preview"/"Render"). PDF / CBZ / EPS each supply a small PageRenderer impl. The shared decode→fit→window-crop→ASCII (`render_image_window`) lives in `types::image::paged_render` beside the engine it drives — only the type-side renderers call it; `image_placeholder` here is the shared failure line. Mirrors RenderedTextMode<R>. EPUB stays separate (adds chapter search + cover render)
    listing/
      mod.rs           — Re-exports: Entry, EntryMtime, FlatEntry, Stats, ListingMode, from_flat_paths, time_from_epoch_secs
      entry.rs         — Entry / EntryKind { File | Dir { children } } / EntryMtime + epoch helper
      stats.rs         — Stats: aggregate counts / sizes computed by tree walk
      build.rs         — FlatEntry + from_flat_paths(): build hierarchical tree from path-keyed entries (synthesizes implicit dirs)
      row.rs           — Shared row-painting primitives for every listing-style view (ListingMode + DirectoryMode): SIZE_COL_WIDTH / PERMS_COL_WIDTH / MTIME_HIDE_BELOW_COLS constants, format_perms / paint_perms, SizeCell + format_size + paint_size + size_color, format_mtime_epoch, paint_selected_marker + ROW_GUTTER + with_marker, compose_row (single source of truth for column layout), paint_mtime + mtime_column_width
      mode.rs          — ListingMode: generic tree-style TOC view (perms, size, mtime, path) + file-selection cursor (used by archive / comic / ebook / document / pdf / audio / disk_image). Leaf-name `/` search via shared `SearchState` (`viewer::search`) scanning each row's last path segment; n/p navigates with wrap; file matches update selection, directory matches only scroll into view via viewport.scroll_to_row
      viewport.rs      — ListingViewport: scroll + selection state + sticky-chain math. `select_row` pins a file selection; `scroll_to_row` brings any row (file or dir) into the content slot without moving the selection cursor
    modes/
      mod.rs           — Mode trait, ModeId, RenderCtx, ExtractTarget (extract_target hook: EntryPath / FrameIndex)
      content.rs           — ContentMode: streamed text / syntax / structured / SVG XML source (LineSource-backed); wrap/scroll geometry delegated to `viewer::wrap_scroll`, pretty branch to `pretty_view`, active branch exposed to the geometry as a `LineView` borrow built by the `active_view` free fn
      content_rendering.rs — Active-output state for ContentMode: `Rendering` enum (`RawOnly` vs `Either { showing, pretty }`) replacing the old pretty-Option / use_pretty-bool / forced-flag triple; owns the `PrettyView` when one exists
      content_pipe.rs      — Pipe-mode rendering for ContentMode: pretty whole-text write, raw stream with highlighter, raw stream without highlighter; shared gutter prefix builder for the two raw paths
      content_tests.rs     — ContentMode tests; loaded as a child of `content` via `#[path]` to reach private fields
      pretty_view.rs   — PrettyView: the lazy structured pretty-print branch — one-shot parse (size-capped at PRETTY_MAX_BYTES), size-cap / parse-error fallback state, theme-keyed rendered-line cache. ContentMode keeps the raw-vs-pretty view state + windowing
      gutter.rs        — Gutter: ContentMode's line-number gutter — on/off state + digit-width sizing, per-visual-row `prefix` (interactive), whole-Vec `apply` (pipe)
      hex.rs           — HexMode: byte-offset-scrolled hex dump (interactive + pipe stream)
      info.rs          — InfoMode: file metadata view; each rendered field word-wrapped to the content width (a long warning / path can't soft-wrap a row the ScreenBuffer miscounts), re-renders on resize
      help.rs          — HelpMode: keyboard-shortcut listing
      about.rs         — AboutMode: logo, version, palette swatches, tips
      rendered_text.rs — RenderedTextMode<R>: generic whole-document read mode (caching per (width, style_mode), search, windowing) over a TextRenderer R. Used by DOCX/ODT, RTF, HTML, PDF text — each supplies a small TextRenderer impl
    table/
      mod.rs           — Two aligned-table flavours under one roof. (1) Materialised path used by objfile / classfile: Table / Column / Cell / CellRole / Align data + cell / cell_spans (multi-colour token cell) / fit_columns. (2) Streaming path used by CSV + SQLite contents: RowsTableMode + RowSource trait (lazy row pulls, cell-scoped search, monotonic auto-widen). Visual shape is identical, data model differs
      mode.rs          — TableMode (materialised flavour): sticky-header table over materialised rows, live-theme cell repaint, vertical scroll + Left/Right pan + `/` search (minimal reveal_h_scroll pan)
      row_source.rs    — RowSource trait: lazy index-addressable row stream (ensure_row / row / loaded / total / column_count / malformed_count / row_is_malformed). Cells are `Option<String>` so NULL stays distinct from empty string for SQLite. Three impls: CsvData (`types/csv/parse.rs`, seed + seek-anchored window) and SqliteRowSet (`types/sqlite/row_set.rs`, LIMIT/OFFSET window) are windowed/bounded; spreadsheet `Sheet` (`types/spreadsheet/workbook.rs`) is fully materialised (calamine has no streaming sheet API — bounded by one sheet). `WINDOW_SIZE` shared from `viewer::table`. Full-file cell search walks every row via repeated `ensure_row`, so windowed sources slide rather than materialise the whole table
      rows_mode.rs     — RowsTableMode: streaming flavour over `Box<dyn RowSource>`. Aligned table with sticky header, monotonic auto-widen (grows widths as wider cells scroll into view; sticky header repaints on every change), `Shift+R` reflow widths from viewport (opt-in shrink), `Shift+H` toggle header, Left/Right column-step horizontal pan. Per-column Alignment + has_header decided at construction by the source's compose path (CSV: classify_cell on seed body; SQLite: column-type affinity). Embedded `\n` collapses to a muted `↵` glyph; `\t` → space, `\r` dropped. Cell-scoped `/` search: scans every cell's display-form bytes, matches stay inside one cell (never cross delimiters); `n`/`p` step matches and pan h_col + scroll top_record. Print mode uses seed widths only — single-row overflow pushes following columns of that row past the terminal edge
    ui/
      mod.rs           — with_alternate_screen, status line composer, terminal-size helpers
      state.rs         — ViewerState: mode stack, active index, scroll, lazy line cache, extract dispatch + prompt overlay slot + status flash. `ensure_active_rendered` recovers from render failures: retry_frame_detection (magic-byte re-detect for misnamed files) then degrade_active_to_hex (drop broken mode → Hex view + decode-cause warning) before propagating
      prompt.rs        — Modal text-input Prompt overlay (readline-style nav) consuming raw key events; replaces status line while open
      screen.rs        — ScreenBuffer: per-row diff against prev frame, no-flash redraw
      keys.rs          — Action enum (centralized keybindings), Outcome
      help.rs          — Keyboard-shortcut help screen renderer
    hex.rs             — Hex layout primitives + format_row (used by HexMode)
docs/                  — Builder / agent reference (architecture, conventions, planning)
  architecture.md      — Design, data flow, key abstractions, extension guide
  architecture-map.md  — This file: full file/module breakdown
  features.md          — Currently shipped features (✅ implemented + ◐ partial)
  planned.md           — Planned features and ideas (☐ planned + ❓ open)
  conventions.md       — Coding conventions
  release.md           — Release pipeline, install.sh, recovery from failed runs
  theme-conversion.md  — How to port VS Code / IDEA themes to peek .tmTheme
  svg-anim-perf.md     — SVG animation memory profile + optimization options
  image-rendering.md   — Image rendering pipeline reference (glyph atlas, fast 2-colour clustering, contour mode)
  checkup-findings.md  — Temporary findings list from the current `/checkup` round (IDs retire as items ship; gaps in numbering are deliberate)
manual/                — User-facing manual (mdbook). `mdbook serve manual` to browse
  book.toml            — mdbook config
  src/                 — Chapter sources (SUMMARY.md + per-topic .md files)
.github/workflows/
  ci.yml               — Build + test on push to main / PRs (cross-platform matrix); fails on warnings
  release.yml          — Manual-dispatch release workflow (5-target build matrix)
  manual.yml           — Build + deploy mdbook manual to GitHub Pages on manual/** changes
install.sh             — POSIX installer for curl | sh on macOS/Linux
```
