# Architecture map

Full file/module breakdown. Read when adding files, modifying a module, or unsure where logic lives.
CLAUDE.md keeps a condensed top-level version; this is the detailed reference.

```
src/
  main.rs              — CLI entry point: dispatches inputs to viewers
  cli.rs               — Args struct (clap derive)
  update.rs            — `--update` flow: GitHub Releases check + pipe install.sh into sh
  input/
    mod.rs             — re-exports InputSource, ByteSource, LineSource, ByteStream
    source.rs          — InputSource (File / Memory{Bytes} / FileRange{base,offset,len} / TempFile{Arc<NamedTempFile>}) + ByteSource trait + FileByteSource / BytesByteSource / RangeByteSource / TempFileByteSource (holds the Arc so reads outlive the source). read_bytes() returns bytes::Bytes; Memory arm is a refcount clone
    lines.rs           — LineSource: streaming, anchor-indexed line view over InputSource
    detect.rs          — File-type detection orchestrator (magic-byte / extension / content-sniff priority + Detected / FileType / CompressionFormat); per-type format enums + detection helpers live alongside their types under `types/<x>/{format,detect}.rs` and are re-exported here
    mime.rs            — MimeCategory + MimeInfo: RFC 6838 classification (Registered / Vendor / x-prefix / unknown) used by the Info screen MIME row
    stream.rs          — ByteStream: io::Read / io::BufRead wrapper over any ByteSource so callers can use io::copy / read_until / lines (tar / cpio / etc. go through this seam)
    compression.rs     — decompress_bytes (5 codecs: gz/bz2/xz/zst/lz4) + stripped_name + resolve_transparent (called at every (source, Detected) entry boundary so bare wrappers open straight to inner content); MAX_DECOMPRESS_BYTES = 256 MiB
    stdin.rs           — Build the input source from CLI args, reopen fd 0 from /dev/tty after pipe
  extract/
    mod.rs             — Module declarations + re-exports (Extracted, ExtractOptions, ExtractError, extract, sanitize_entry_path)
    extract.rs         — Top-level dispatch (FileType → per-type extractor) + Extracted/Options/Error types + path sanitiser
    write.rs           — Output enum + write_extracted: streams to stdout or writes file at path
  output/
    mod.rs             — re-exports PrintOutput
    print.rs           — PrintOutput: write-once stdout for --print / pipes / --info
    help.rs            — CLI help and version screens
  info/
    mod.rs             — FileInfo + FileExtras enum (single-field wrappers around per-type stats from `types/<x>/info.rs`) + shared permission helpers
    gather/            — FileInfo collection, split per general file type
      mod.rs           — Per-source dispatch (gather() entry point)
      tests.rs         — Fixture-based tests against test-images / test-data
    render/            — Themed terminal rendering of FileInfo, split per section
      mod.rs           — render() entry, RenderOptions, shared push_field/section_header/paint_count
      file.rs          — File section: name, path, size, MIME, timestamps, permissions
    time.rs            — UTC ISO / local-with-offset timestamp formatting (libc::localtime_r)
  theme/
    mod.rs             — re-exports PeekThemeName, StyleMode, PeekTheme, ThemeManager, helpers
    name.rs            — PeekThemeName + embedded .tmTheme data + load_embedded_theme
    sgr.rs             — Low-level SGR mechanics: color/attr encoders, RESET_* consts, palette quantization, escape tokenizer (scan/Sgr) + classify + ActiveStyle (fg/bg tracked across a styled stream)
    style_mode.rs      — StyleMode (truecolor/256/16/grayscale/plain) + RGB→palette conversion
    peek_theme.rs      — PeekTheme semantic roles + paint helpers + lerp_color/blend + rgb↔hsl + search-match colors
    manager.rs         — ThemeManager: shared SyntaxSet/ThemeSet + active PeekTheme
  types/
    mod.rs             — Per-file-type modules (each owns reader + info + view-mode)
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
      format.rs        — CertFormat enum (Pem only; DER / PKCS#12 planned)
      detect.rs        — format_from_ext (`.pem` / `.csr` / `.crl` / `.key` / `.p7b` / `.p7c` / `.pub`) + sniff_pem (`-----BEGIN ` header / `ssh-rsa…` etc. content sniff). `.crt` / `.cer` left to content sniff because they routinely carry DER too
      compose.rs       — compose(): paired Source ContentMode for PEM text (no syntax token); Info aux mode renders the cert sidecar
      info.rs          — CertInfo { text: TextStats, entries: Vec<CertEntry>, parse_errors: Vec<String> } + CertEntry variants (Certificate / CSR / CRL / PrivateKey / PublicKey / SshPublicKey / Unknown — heavy variants boxed) + per-entry shapes + KeyType (Rsa / Ec(curve) / Ed25519 / Dsa / Other)
      info_gather.rs   — pem::parse_many → per-block dispatch by PEM label. X.509 cert / CSR / CRL decoded via x509-parser; SSH public-key lines (outside any PEM fence) decoded via ssh-key. Private/public keys: hand-rolled ASN.1 TLV walker reads PKCS#1 / SEC1 / PKCS#8 / SPKI envelopes to recover key type + bit size without pulling in a fourth crypto crate. SHA-1 + SHA-256 fingerprints over the cert DER (sha1 / sha2)
      info_render.rs   — Render PEM info section (Content + per-entry blocks: Subject / Issuer / Serial / NotBefore / NotAfter / Days Left / Public Key / Signature / SANs / Key Usage / fingerprints). Days-Left ≤ 30 painted as warning; expired painted as warning with negative day count
    font/
      mod.rs           — Module wiring
      format.rs        — FontFormat enum (TrueType / OpenType / Collection) + label
      detect.rs        — format_from_ext (`.ttf` / `.otf` / `.ttc` / `.otc`) + sniff_font_bytes (4-byte magic: `00 01 00 00` / `true` / `OTTO` / `ttcf`). WOFF / WOFF2 deferred (need separate decompressors)
      compose.rs       — compose(): rasterise specimen → SpecimenMode (primary view). Best-effort — a font fontdue can't parse skips the specimen push and falls through to the Info + Hex tail
      info.rs          — FontInfo { format, face_count, faces, parse_errors } + FaceInfo (family / subfamily / postscript_name / version / OS/2 weight + width / italic / monospaced / glyph_count / units_per_em / codepoint_count / scripts / hinting / designer / vendor / copyright / license_url)
      info_gather.rs   — ttf-parser Face walk: read_name_table (UTF-16BE + Mac Roman decoders — Apple system fonts still ship Macintosh-platform records as canonical, so the full Mac Roman upper-half mapping is bundled here) + head_flags (hinting bit) + scan_cmap (Unicode codepoint count + 12-bucket script coverage from cmap ranges). Every face in a collection is gathered; face_count() exposed so the compose path can size SpecimenMode's cycle range without re-parsing
      info_render.rs   — Render Font info section (Format + Faces count for collections, then a per-face block per FaceInfo). Weight painted as `<class> (<name>)` for canonical OS/2 weights, bare number otherwise; empty name-table fields skip their row entirely
      specimen.rs      — rasterise(bytes, face_index, target_height_px) → DynamicImage. fontdue rasterises each glyph of a hard-coded sample (pangrams + digits + ASCII alphabet) at a derived font size, blits them into a white RGBA8 canvas with baseline alignment. Coverage values darken the destination per glyph; the existing image pipeline downsamples + composites
      specimen_mode.rs — SpecimenMode: parallel to image::ImageRenderMode but owns a pre-decoded DynamicImage instead of a source. Same ImageView wiring (cycle background / image-mode / fit, FitHeight horizontal pan, single-slot cache invalidated on resize / margin / bg / fit change). For collections, holds the original `Bytes` + face count + current face index; `n` / `p` (NextFace / PrevFace) re-rasterise the next face in place with wrap, status segment surfaces `Face N/M`. Pipe path uses capped_for_image_pipe so font specimens don't dominate piped output
    structured/
      mod.rs           — Module wiring
      format.rs        — StructuredFormat enum (JSON/JSONC/JSON5/JSONL/YAML/TOML/XML)
      detect.rs        — format_from_ext: extension → StructuredFormat
      info.rs          — StructuredInfo / StructuredStats / TopLevelKind + gather_extras (per-format stats) + render_section (Format)
      pretty.rs        — JSON / YAML / TOML / XML pretty-printers (used by ContentMode)
    csv/
      mod.rs           — Module wiring; re-exports CsvStats
      format.rs        — CsvFormat enum (Csv/Tsv) + default_delimiter
      detect.rs        — format_from_ext: `.csv` / `.tsv` → CsvFormat
      parse.rs         — Streaming CSV record reader over `csv::Reader<Box<dyn Read>>`. Seed scan (first 1000 records into memory + header heuristic + delimiter sniff + UTF-8/UTF-16 BOM detect); subsequent records pulled lazily via `ensure_record(idx)`. UTF-16 LE/BE transcoded eagerly to UTF-8 byte buffer. Malformed guard: > 4 MiB per record OR > 10 000 physical lines OR csv-crate error → `<error>` row + malformed counter; reader resyncs on next newline.
      compose.rs       — compose(): CsvTableMode (primary) + paired Source ContentMode
      table_mode.rs    — CsvTableMode: aligned table with sticky header, monotonic auto-widen (grows widths as wider cells scroll into view; sticky header repaints on every change), `Shift+R` reflow widths from viewport (opt-in shrink), `Shift+H` toggle header, Left/Right column-step horizontal pan. Per-column Alignment inferred from seed body (Int/Float only → Right). Embedded `\n` collapses to a muted `↵` glyph via `display_cell`; `\t` → space, `\r` dropped. Cell-scoped `/` search: scans every cell's display-form bytes, matches stay inside one cell (never cross delimiters); `n`/`p` step matches and pan h_col + scroll top_record to bring the match's cell into view. Print mode uses seed widths only — single-row overflow pushes following columns of that row past the terminal edge.
      info.rs          — CsvStats { format, delimiter, encoding, has_bom, header_detected, columns: Vec<ColumnStats>, loaded_records, total_records, malformed_count, sampled } + ColumnStats / ColumnType (Int/Float/Bool/Date/String/Mixed)
      info_gather.rs   — gather: per-column type inference + width / empty counts over the seed sample
      info_render.rs   — render_section (CSV + Columns blocks)
    image/
      mod.rs           — Module wiring; re-exports ImageRenderMode, AnimationMode, ImageConfig
      compose.rs       — compose(): push AnimationMode for animated GIF/WebP, ImageRenderMode for static raster
      info.rs          — ImageStats + AnimationStats + LoopCount (animation summary)
      info_gather.rs   — gather_extras (dimensions, color, ICC, HDR) + IMAGE_HEAD_SCAN/read_head
      info_render.rs   — render_section (Image, EXIF, XMP, Animation)
      extract.rs       — Animation frame extract (GIF/WebP): decode all frames, re-encode frame N as PNG (Memory-backed)
      exif.rs          — EXIF field extraction
      xmp.rs           — XMP packet scrape (Dublin Core / xmp tags)
      animation_stats.rs — GIF/WebP animation stats (frames, duration, loop)
      view.rs          — ImageView: shared image-grid scroll + zoom + cycleable config for every Mode that scrolls through a PreparedImage (ImageRenderMode + AnimationMode + SvgAnimationMode + SpecimenMode). Holds (config, scroll_x, scroll_y, zoom); exposes view_bounds (effective grid - viewport per axis), render_prepared (clamp pan + zoom dispatch + render), pipe_snapshot/restore (force-Contain + zoom=1 wrapper for `--print`), scroll, handle_config_cycle (b/m/f keys + pan reset on fit change), handle_zoom (+/-/0/1-9 with viewport-centre anchor), status_segments
      zoom.rs          — ZoomLevel: multiplicative zoom factor on top of fit-mode base grid. 1.25× per +/- step, 1×..16× clamp, preset() for digit keys, label() for status bar
      anim_frame.rs    — AnimFrameState: shared frame-playback state (current / playing / last_advance) for animated image Modes (AnimationMode + SvgAnimationMode). play_pause / step / tick / next_tick / status_segment / extract_target
      scroll.rs        — Shared scroll-action handler for image-grid modes (ImageRenderMode + AnimationMode + SvgAnimationMode): arrows / PgUp / PgDn / Home / End → (scroll_x, scroll_y) deltas with Bounds clamping
      mode.rs          — ImageRenderMode: static raster + rasterized SVG view; embeds ImageView, owns InputSource + single-slot CachedFrame
      animation_mode.rs — AnimationMode: GIF/WebP playback (next_tick / tick driven); embeds ImageView + AnimFrameState, owns decoded frame list (no per-frame cache — frames change every tick)
      pipeline/        — Rasterization → ASCII-art rendering core
        mod.rs         — Module wiring + Background / FitMode / ImageConfig
        image_mode.rs  — ImageMode enum (full/block/geo/ascii/contour palette selection)
        render.rs      — Image → glyph-matched ASCII art with true color
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
      detect.rs        — format_from_ext: `.epub` → EbookFormat::Epub
      format.rs        — EbookFormat enum
      info.rs          — Shared ebook info shape (universal across EPUB / MOBI / FB2): EbookStats { metadata: Metadata, chapter_count }
      epub/
        mod.rs         — Module wiring; re-exports EpubReadMode
        package.rs     — Parse EPUB ZIP: META-INF/container.xml → OPF rootfile → DC metadata (into shared Metadata) + manifest (id→href) + spine; resolve spine to absolute ZIP paths; ZIP entry reader
        read_mode.rs   — EpubReadMode: one chapter at a time via shared html `render`. Per-chapter render cache keyed by (idx, width); n / N step chapter (Action::NextChapter / PrevChapter). render_to_pipe walks the whole spine. Pre-processes `<img>` tags to inject `alt="image: <basename>"` for empty / missing alt so chapter image refs stay visible. Cover-style chapters (≤ 3 non-empty rendered lines + at least one `<img>`) render the first image as ASCII via the image pipeline
        info_gather.rs — Populate EbookStats (DC metadata + chapter count) from package::open
        info_render.rs — Render EPUB info section from EbookStats
    document/
      mod.rs           — Module wiring; re-exports DocumentStats / DocumentMetadata / DocRenderer
      compose.rs       — compose(): DOCX/ODT → RenderedTextMode<DocRenderer> + ZIP TOC ListingMode; RTF → RenderedTextMode<RtfRenderer> + inline-embed listing when any \pict groups parsed
      detect.rs        — format_from_ext + format_from_mime (RTF magic-byte route)
      format.rs        — DocumentFormat enum (Docx/Odt/Rtf) + label
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
      compose.rs       — compose(): PagedImageMode<PdfPageRenderer> (fit forced to FitWidth) + RenderedTextMode<PdfTextRenderer> + /EmbeddedFiles ListingMode
      package.rs       — Lazy global Pdfium init (exe-dir → .pdfium/lib dev fallback → system); load_pdf_from_byte_vec → Arc-backed Doc with page_count / render_page (RGBA via image feature) / page_text / metadata / list_embeds / read_embed; list_embeds returns one tree under `attachments/<name>` (/EmbeddedFiles) plus `pages/page{N}/image{M}.{ext}` (inline image XObjects); read_embed dispatches by prefix and falls back to `get_raw_image` → PNG re-encode for codecs `get_raw_image_data` doesn't surface as a usable file. PDF date `D:YYYYMMDDHHMMSSZ` → `YYYY-MM-DD HH:MM:SS UTC` formatter
      page_renderer.rs — PdfPageRenderer: PageRenderer impl — rasterizes a page via Pdfium (~16 px/col) and ASCII-renders it through `pipeline::render::{prepare_decoded, render_prepared}`. Wrapped in the generic `viewer::paged::PagedImageMode`
      text_renderer.rs — PdfTextRenderer: TextRenderer impl over `Doc::page_text`; pages joined with muted `--- Page N ---` separator; greedy word-wrap with hard-break for over-width tokens. Per-page extract failures degrade to a placeholder line + warning
      extract.rs       — Extract `/EmbeddedFiles` attachment by name → InputSource::Memory; reuses `extract::sanitize_entry_path`
      info.rs          — PdfStats { metadata: DocumentMetadata, page_count, attachment_count (/EmbeddedFiles), image_count (per-page XObjects), encrypted, pdf_version, error: Option<String> }
      info_gather.rs   — Populate PdfStats via package::open_doc; failures land as `error` field rendered as warning row
      info_render.rs   — Render PDF info section (Version / Title / Author / Subject / Keywords / Created / Modified / Pages / Attachments). On error, render only `Error: ...` and stop
    comic/
      mod.rs           — Module wiring; re-exports ComicStats / CbzPageRenderer
      compose.rs       — compose(): PagedImageMode<CbzPageRenderer> (paged images) + ZIP TOC ListingMode
      detect.rs        — format_from_ext: `.cbz` → ComicFormat::Cbz
      format.rs        — ComicFormat enum + label
      info.rs          — Shared comic-archive info shape (only CBZ ships today; the shape is sized for CBR / CB7 / CBT if they're ever added): ComicStats { format, page_count, total_image_bytes }
      cbz/
        mod.rs         — Module wiring; re-exports CbzPageRenderer
        package.rs     — list_pages: walk ZIP central directory, filter image entries by extension (png/jpg/jpeg/webp/gif/bmp/tif/tiff), skip __MACOSX/, sort by name; open_zip + read_page for body fetch
        page_renderer.rs — CbzPageRenderer: PageRenderer impl — decodes one ZIP image entry and ASCII-renders it via the image pipeline. Wrapped in the generic `viewer::paged::PagedImageMode`
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
      detect.rs        — format_from_ext + format_from_mime (audio container routing)
      format.rs        — AudioFormat enum + label
      info.rs          — Shared audio info shape: AudioStats { format, codec, duration_secs, sample_rate, channels, channel_layout, bits_per_sample, bitrate, metadata: AudioMetadata, has_lyrics, has_album_art, error } + AudioMetadata { title, artist, album, album_artist, track_number, disc_number, date, genre, composer, comment }
      package.rs       — Central symphonia probe. `probe(source, format)` → `Probed { codec/track params, AudioMetadata, visuals: Vec<EmbedVisual>, lyrics: Option<String> }`. Walks both `format.metadata().current()` (Vorbis on Ogg/FLAC) and `probed.metadata.get().current()` (ID3v2 sidecar on MP3/AIFF); embedded visuals carried as raw bytes + media_type + canonical `usage_root` (front_cover / back_cover / artist / …). Lyrics joined across USLT/SYLT/`LYRICS=` sources. `to_stats(&Probed)` projects onto AudioStats for InfoMode. `primary_cover(&Probed)` picks the FrontCover-tagged visual (fallback first) for the dedicated Cover tab; `visual_filename` builds its suggested name. `build_listing(&Probed)` synthesises `pictures/<usage>.<ext>` (with `_N` suffix on dup roots) + `lyrics/lyrics.txt`; empty when nothing embedded. `read_embed(&Probed, key)` returns `(Vec<u8>, suggested_name)` for extract. Re-probes per call (header + tag walk, ms-cheap)
      info_gather.rs   — Thin shim: calls `package::probe` + `package::to_stats`; failures land as `error` field
      info_render.rs   — Render Audio + Tags info sections. Tags section omitted when no tag fields populated
      extract.rs       — Per-key extract: `package::probe` → `package::read_embed` → `InputSource::Memory`. Image bytes re-detect as Image (route through ASCII pipeline on recursive peek); lyrics text re-detect as plain text
    archive/
      mod.rs           — Module wiring (no re-exports; consumers reach in via reader / info / extract)
      compose.rs       — compose(): list TOC entries → ListingMode under the format's label
      detect.rs        — format_from_name + format_from_mime (handles double-extensions `.tar.gz` etc. before bare compression)
      format.rs        — ArchiveFormat enum + label
      reader.rs        — list_entries dispatcher (returns Vec<Entry>) + ReadSeek helper
      info.rs          — ArchiveStats + gather_extras (TOC stats via Stats::from_root) + render_section (Archive info section)
      extract.rs       — Per-format entry extract via materialise(reader, declared_size, opts): entries ≥ SPOOL_THRESHOLD (16 MiB) or unknown size land in InputSource::TempFile ($TMPDIR/peek-*, RAII unlink via Arc<NamedTempFile>); smaller stay in Bytes. --no-tempfile forces Vec path and drops the 256 MiB MAX_EXTRACT_BYTES cap. zip/tar[gz/bz2/xz/zst/lz4]/7z/cpio[gz]/ar. decompress_tar() delegates codec dispatch to crate::input::compression::decompress_bytes
      backends/
        mod.rs         — Backend module wiring
        zip.rs         — Zip TOC via central directory (no decompression); returns Vec<FlatEntry>
        tar.rs         — Tar TOC via header walk; gz/bz2/zst/lz4 stream-decompress, xz batch-decompresses (lzma-rs has no streaming Read wrapper)
        sevenz.rs      — 7-Zip TOC via sevenz-rust2 (header-only)
        cpio.rs        — cpio TOC via hand-rolled newc (`070701`/`070702`) + ODC (`070707`) header walker. CpioReader state machine drives both list (skip bodies) and extract (read matched body). plain + gz wrappers; old-binary cpio not supported
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
      detect.rs        — format_from_ext + Raw → Iso upgrade (cheap 6-byte PVD probe at offset 32768)
      format.rs        — DiskImageFormat enum (Iso/Dmg/Raw) + label
      info.rs          — DiskImageInfo + DiskImageMeta { Iso | Dmg | Raw } + IsoVolumeMeta / IsoDateTime / DmgMeta / DmgVariant / DmgChecksumKind / RawImageMeta / MbrTable / MbrPartition
      iso_pvd.rs       — Hand-rolled ISO 9660 Primary Volume Descriptor parser + Joliet / El Torito scan + root-extent locator
      iso_listing.rs   — ISO 9660 directory walker → Listing tree (Joliet preferred; depth/entry caps; no Rock Ridge) + lookup_file_range for extract
      dmg_trailer.rs   — Hand-rolled UDIF (Apple Disk Image) "koly" trailer parser (last 512 bytes)
      mbr.rs           — MBR partition-table parser for raw `.img` / `.bin` / `.dd` images; reads the 512-byte boot sector and populates `MbrTable` / `MbrPartition` shown in the Info view
      extract.rs       — ISO entry extract: lookup_file_range → zero-copy FileRange (or Bytes::slice for stdin-piped); DMG returns Unsupported
      info_gather.rs   — gather_extras: ISO reads 16 KiB at offset 32768; DMG reads tail 512 bytes
      info_render.rs   — render_section (Disk Image info section, ISO + DMG blocks)
    objfile/
      mod.rs           — Module wiring
      compose.rs       — compose(): InfoMode landing view + Sections / Symbols TableMode (no extract path)
      load.rs          — Fat-aware load: object::FileKind probe → universal Mach-O slice select (host arch, else first) → object::File::parse; FatSummary lists every slice
      info.rs          — ObjectInfo { meta: Option<ObjectMeta>, error } + ObjectMeta (semantic `object` enums: BinaryFormat / Architecture / ObjectKind / Endianness — not pre-formatted)
      info_gather.rs   — gather_extras: load + capture header counts into ObjectMeta
      info_render.rs   — render_section (Object File section) + enum→label maps (format / kind / arch / endianness — the one place metadata becomes text)
      tables.rs        — build(): Sections / Symbols as shared `viewer::table::Table` data (typed Cell + CellRole) via the `object` crate
    classfile/
      mod.rs           — Module wiring
      compose.rs       — compose(): InfoMode landing view + Fields / Methods TableMode (no extract path)
      info.rs          — ClassfileInfo { meta: Option<ClassfileMeta>, error } + ClassfileMeta (keeps cafebabe's ClassAccessFlags semantic; render maps it)
      info_gather.rs   — gather_extras: cafebabe parse_class_with_options (bytecode parsing off) → ClassfileMeta
      info_render.rs   — render_section (Class File section) + version / access-flag → label maps
      descriptor.rs    — Render cafebabe descriptor types as syntax-highlighted spans (`(Ljava/lang/String;I)V` → coloured `(String, int) -> void`: primitives / class names / `[]` / punctuation each a CellRole)
      tables.rs        — build(): Fields / Methods as shared `viewer::table::Table` data
  viewer/
    mod.rs             — Registry, compose_modes (single-file dispatch table delegating to `types::<x>::compose::compose`), ComposeCtx (theme manager / name / plain mode — the `text_content_mode` bundle), free `image_config`. Re-exports highlight_lines / LineStreamHighlighter from `highlight`
    highlight.rs       — Syntect highlighting: highlight_lines (whole-text), LineStreamHighlighter (forward-only streaming feeder used by ContentMode), syntax_token_for (FileType + filename → syntect syntax token, honors `--language`), fallback_syntax_token (extensions syntect doesn't natively support)
    cell_size.rs       — Terminal cell aspect-ratio detection: cell_aspect_h_over_w reads cell pixel dims from TIOCGWINSZ (cached on first call), falls back to 1:2 when the terminal can't report; set_override for an explicit user override. Used by the image pipeline to preserve source aspect across fonts
    interactive.rs     — Unified event loop driving a Vec<Box<dyn Mode>> stack; routes raw keys to active prompt overlay when one is open
    search.rs          — Text-search primitives: smart_case_sensitive, find_matches (exact substring), overlay_matches (paint match backgrounds onto a styled line), SearchState (scan/step/line_overlay/status_segment — shared by every searchable mode), reveal_h_scroll (minimal-pan offset to bring a match on screen) + overlay_window
    wrap_scroll.rs     — WrapScroll: wrap-aware scroll position (logical line / visual sub-row / horizontal pan) + LineView enum (Raw(&LineSource) | Pretty(&[String])). ContentMode's scroll geometry — step / page / clamp / bottom-find over wrapped lines — lives here, branch-agnostic via LineView
    paged.rs           — Shared paged-render mechanism: PageCacheKey / CachedRender / render_cached / step_paged / pipe_rows. Image-config cycling: cycle_image_config handler + the CYCLE_BACKGROUND/IMAGE_MODE/FIT_HELP rows it dispatches, pinned together by a unit test so help and handling can't drift (shared by paged / image / animation / svg-anim / epub modes). Plus PagedImageMode<R> — generic one-page-at-a-time image Mode over the PageRenderer trait (page_count + render_page); PDF / CBZ each supply a small PageRenderer impl. Mirrors RenderedTextMode<R>. EPUB stays separate (adds chapter search + cover render)
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
      info.rs          — InfoMode: file metadata view
      help.rs          — HelpMode: keyboard-shortcut listing
      about.rs         — AboutMode: logo, version, palette swatches, tips
      rendered_text.rs — RenderedTextMode<R>: generic whole-document read mode (caching per (width, style_mode), search, windowing) over a TextRenderer R. Used by DOCX/ODT, RTF, HTML, PDF text — each supplies a small TextRenderer impl
    table/
      mod.rs           — Generic aligned-table view shared by objfile + classfile: Table / Column / Cell / CellRole / Align data + cell / cell_spans (multi-colour token cell) / fit_columns (content-fitted widths). CsvTableMode does NOT use this — streaming backing, cell-scoped search
      mode.rs          — TableMode: sticky-header table over materialised rows, live-theme cell repaint, vertical scroll + Left/Right pan + `/` search (minimal reveal_h_scroll pan)
    ui/
      mod.rs           — with_alternate_screen, status line composer, terminal-size helpers
      state.rs         — ViewerState: mode stack, active index, scroll, lazy line cache, extract dispatch + prompt overlay slot + status flash
      prompt.rs        — Modal text-input Prompt overlay (readline-style nav) consuming raw key events; replaces status line while open
      screen.rs        — ScreenBuffer: per-row diff against prev frame, no-flash redraw
      keys.rs          — Action enum (centralized keybindings), Outcome
      help.rs          — Keyboard-shortcut help screen renderer
    hex.rs             — Hex layout primitives + format_row (used by HexMode)
themes/
  idea-dark.tmTheme           — JetBrains IDEA default Dark theme (default)
  vscode-dark-modern.tmTheme  — VS Code Dark Modern theme
  vscode-dark-2026.tmTheme    — VS Code Dark 2026 theme
  vscode-monokai.tmTheme      — VS Code Monokai theme
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
