# peek

Modern terminal file viewer with syntax highlighting, structured-data pretty-printing, and image
rendering.

**Single-file viewer.** One path (or stdin) at a time. No batch mode, no file list, no `cat`-style
concatenation — those use cases belong to other tools.

## Build & Run

```sh
cargo build                  # debug build
cargo build --release        # release build
cargo run -- [args]          # run with arguments
cargo test                   # run all tests
cargo clippy                 # lint
```

No external runtime dependencies. Image rendering is built in. (PDF support uses Pdfium —
shipped alongside the binary in the release tarball, dynamically loaded at startup; no system
install needed.)

## Architecture map

```
src/
  main.rs              — CLI entry point: dispatches inputs to viewers
  cli.rs               — Args struct (clap derive)
  update.rs            — `--update` flow: GitHub Releases check + pipe install.sh into sh
  input/
    mod.rs             — re-exports InputSource, ByteSource, LineSource
    source.rs          — InputSource (File / Memory{Bytes} / FileRange{base,offset,len}) + ByteSource trait + FileByteSource / BytesByteSource / RangeByteSource
    lines.rs           — LineSource: streaming, anchor-indexed line view over InputSource
    detect.rs          — File-type detection (extension + magic bytes + stdin sniffing). FileType::Compressed(CompressionFormat) splits bare wrappers out of ArchiveFormat
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
    mod.rs             — FileInfo, FileExtras data types and shared permission helpers
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
    style_mode.rs      — StyleMode (truecolor/256/16/grayscale/plain) + RGB→palette conversion
    peek_theme.rs      — PeekTheme semantic roles + paint helpers + lerp_color/blend
    manager.rs         — ThemeManager: shared SyntaxSet/ThemeSet + active PeekTheme
  types/
    mod.rs             — Per-file-type modules (each owns reader + info + view-mode)
    binary/
      mod.rs           — Module wiring
      info.rs          — gather_extras (friendly format label) + render_section (Format)
    text/
      mod.rs           — Module wiring
      info_gather.rs   — gather_text_stats: streaming UTF-8/UTF-16 stats (lines/words/encoding/indent/shebang)
      info_render.rs   — push_text_stats: Content/Source section content
    markdown/
      mod.rs           — Module wiring
      info_gather.rs   — Single-pass MD stats: headings by level, fenced blocks + langs, links/images/tables/lists, task progress, frontmatter, prose word count, reading time
      info_render.rs   — Render Markdown info section
    sql/
      mod.rs           — Module wiring
      info_gather.rs   — Statement scanner with string/comment/dollar-quote state; classifies DDL/DML/DQL/TCL, records created objects, guesses dialect
      info_render.rs   — Render SQL info section
    structured/
      mod.rs           — Module wiring
      info.rs          — gather_extras (per-format stats) + render_section (Format)
      pretty.rs        — JSON / YAML / TOML / XML pretty-printers (used by ContentMode)
    image/
      mod.rs           — Module wiring; re-exports ImageRenderMode, AnimationMode, ImageConfig
      info_gather.rs   — gather_extras (dimensions, color, ICC, HDR) + IMAGE_HEAD_SCAN/read_head
      info_render.rs   — render_section (Image, EXIF, XMP, Animation)
      extract.rs       — Animation frame extract (GIF/WebP): decode all frames, re-encode frame N as PNG (Memory-backed)
      exif.rs          — EXIF field extraction
      xmp.rs           — XMP packet scrape (Dublin Core / xmp tags)
      animation_stats.rs — GIF/WebP animation stats (frames, duration, loop)
      mode.rs          — ImageRenderMode: static raster + rasterized SVG view
      animation_mode.rs — AnimationMode: GIF/WebP playback (next_tick / tick driven)
      pipeline/        — Rasterization → ASCII-art rendering core
        mod.rs         — Module wiring + Background / FitMode / ImageConfig
        image_mode.rs  — ImageMode enum (full/block/geo/ascii/contour palette selection)
        render.rs      — Image → glyph-matched ASCII art with true color
        animate.rs     — GIF/WebP frame decoding + frame counting + render_frame
        glyph_atlas.rs — Precomputed glyph bitmaps
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
          util.rs      — Shared helpers: skip_ws, find_substr/brace, parse_length, root_svg_dimensions
    html/
      mod.rs           — Module wiring; re-exports RenderedMode
      mode.rs          — RenderedMode: width-keyed cache wrapper around `render::render`; rerender on resize
      render.rs        — Shared html2text driver: bytes → ANSI lines via StyleMode (also used by EPUB chapters). CSS via html2text `use_doc_css`; near-grayscale colours filtered to avoid fighting terminal foreground
    ebook/
      mod.rs           — Module wiring; re-exports EbookStats / Metadata
      info.rs          — Shared ebook info shape (universal across EPUB / MOBI / FB2): EbookStats { metadata: Metadata, chapter_count }
      epub/
        mod.rs         — Module wiring; re-exports EpubReadMode
        package.rs     — Parse EPUB ZIP: META-INF/container.xml → OPF rootfile → DC metadata (into shared Metadata) + manifest (id→href) + spine; resolve spine to absolute ZIP paths; ZIP entry reader
        read_mode.rs   — EpubReadMode: one chapter at a time via shared html `render`. Per-chapter render cache keyed by (idx, width); n / N step chapter (Action::NextChapter / PrevChapter). render_to_pipe walks the whole spine. Pre-processes `<img>` tags to inject `alt="image: <basename>"` for empty / missing alt so chapter image refs stay visible. Cover-style chapters (≤ 3 non-empty rendered lines + at least one `<img>`) render the first image as ASCII via the image pipeline
        info_gather.rs — Populate EbookStats (DC metadata + chapter count) from package::open
        info_render.rs — Render EPUB info section from EbookStats
    document/
      mod.rs           — Module wiring; re-exports DocumentStats / DocumentMetadata / DocReadMode
      ast.rs           — Shared word-processing AST (Doc / Block::{Paragraph,Table} / Paragraph / Run + count_words + merge_paragraphs). Populated by both docx::package and odt::package; RTF stays separate because its on-the-wire shape is a flat painter-tagged text stream
      render.rs        — Shared render(&Doc, width, theme, style_mode) -> Vec<String>: width-aware word wrap, per-run SGR (bold/italic/underline/strike + custom fg color), heading bold + theme.heading colour, bullet prefix "• ", table rows joined " | ". Used by both DOCX and ODT
      read_mode.rs     — Shared DocReadMode: per-(width, style_mode) line cache over render::render. Format-agnostic; the per-format wiring only supplies the parsed Doc
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
        mod.rs         — Module wiring; re-exports RtfReadMode. RTF stays outside the shared AST because its on-the-wire shape is a flat painter-tagged text stream, not a paragraph/run tree
        parse.rs       — Pre-process RTF (strip `{\info ...}` group, inject `\\\n` after each `\par` so rtf-parser's lexer emits CRLF) → RtfDocument::try_from → owned Vec<Block { painter, paragraph, text }> with painter resolved against \colortbl. Hand-scans `\info` group bytes for title / author / subject / keywords / creatim / revtim
        render.rs      — render(&Parsed, width, theme, style_mode) -> Vec<String>: wraps StyleBlock.text by width, emits SGR for painter bold/italic/underline/strike + colortbl color
        read_mode.rs   — RtfReadMode: per-(width, style_mode) line cache; no listing or extract (RTF is single-file)
        info_gather.rs — Populate DocumentStats via parse::open_source
    pdf/
      mod.rs           — Module wiring; re-exports PdfStats, PdfPageMode, PdfTextMode
      package.rs       — Lazy global Pdfium init (exe-dir → .pdfium/lib dev fallback → system); load_pdf_from_byte_vec → Arc-backed Doc with page_count / render_page (RGBA via image feature) / page_text / metadata / list_embeds / read_embed; list_embeds returns one tree under `attachments/<name>` (/EmbeddedFiles) plus `pages/page{N}/image{M}.{ext}` (inline image XObjects); read_embed dispatches by prefix and falls back to `get_raw_image` → PNG re-encode for codecs `get_raw_image_data` doesn't surface as a usable file. PDF date `D:YYYYMMDDHHMMSSZ` → `YYYY-MM-DD HH:MM:SS UTC` formatter
      page_mode.rs     — PdfPageMode: paged image render via `pipeline::render::{prepare_decoded, render_prepared}`. Per-page cache keyed by (cols, rows, style, image config); n / N step page (Action::NextChapter / PrevChapter, labeled "page"). Mirrors CbzReadMode shape
      text_mode.rs     — PdfTextMode: width-cached text via `Doc::page_text`; pages joined with muted `--- Page N ---` separator. Mirrors DocxReadMode shape; greedy word-wrap with hard-break for over-width tokens
      extract.rs       — Extract `/EmbeddedFiles` attachment by name → InputSource::Memory; reuses `extract::sanitize_entry_path`
      info.rs          — PdfStats { metadata: DocumentMetadata, page_count, attachment_count (/EmbeddedFiles), image_count (per-page XObjects), encrypted, pdf_version, error: Option<String> }
      info_gather.rs   — Populate PdfStats via package::open_doc; failures land as `error` field rendered as warning row
      info_render.rs   — Render PDF info section (Version / Title / Author / Subject / Keywords / Created / Modified / Pages / Attachments). On error, render only `Error: ...` and stop
    comic/
      mod.rs           — Module wiring; re-exports ComicStats / CbzReadMode
      info.rs          — Shared comic-archive info shape (CBZ / CBR / CB7 / CBT): ComicStats { format, page_count, total_image_bytes }
      cbz/
        mod.rs         — Module wiring; re-exports CbzReadMode
        package.rs     — list_pages: walk ZIP central directory, filter image entries by extension (png/jpg/jpeg/webp/gif/bmp/tif/tiff), skip __MACOSX/, sort by name; open_zip + read_page for body fetch
        read_mode.rs   — CbzReadMode: one page at a time via image pipeline. Per-page render cache keyed by (idx, cols, rows, style, image config); n / N step page (Action::NextChapter / PrevChapter, relabeled "page"). render_to_pipe walks every page separated by blank line
        info_gather.rs — Populate ComicStats (page count + uncompressed image bytes) from package::list_pages
        info_render.rs — Render comic info section from ComicStats
    svg/
      mod.rs           — Module wiring; re-exports SvgAnimationMode
      info_gather.rs   — gather_extras (viewBox, element counts, security flags, animation summary)
      info_render.rs   — render_section (SVG info section)
      extract.rs       — SVG anim frame extract: render_frame → resvg rasterize at intrinsic size (sub-512px upscaled to 512 floor) → PNG
      animation_mode.rs — SvgAnimationMode: CSS `@keyframes` SVG playback (per-frame rasterize + LRU cache)
    audio/
      mod.rs           — Module wiring; re-exports AudioStats
      info.rs          — Shared audio info shape: AudioStats { format, codec, duration_secs, sample_rate, channels, channel_layout, bits_per_sample, bitrate, metadata: AudioMetadata, has_lyrics, has_album_art, error } + AudioMetadata { title, artist, album, album_artist, track_number, disc_number, date, genre, composer, comment }
      package.rs       — Central symphonia probe. `probe(source, format)` → `Probed { codec/track params, AudioMetadata, visuals: Vec<EmbedVisual>, lyrics: Option<String> }`. Walks both `format.metadata().current()` (Vorbis on Ogg/FLAC) and `probed.metadata.get().current()` (ID3v2 sidecar on MP3/AIFF); embedded visuals carried as raw bytes + media_type + canonical `usage_root` (front_cover / back_cover / artist / …). Lyrics joined across USLT/SYLT/`LYRICS=` sources. `to_stats(&Probed)` projects onto AudioStats for InfoMode. `build_listing(&Probed)` synthesises `pictures/<usage>.<ext>` (with `_N` suffix on dup roots) + `lyrics/lyrics.txt`; empty when nothing embedded. `read_embed(&Probed, key)` returns `(Vec<u8>, suggested_name)` for extract. Re-probes per call (header + tag walk, ms-cheap)
      info_gather.rs   — Thin shim: calls `package::probe` + `package::to_stats`; failures land as `error` field
      info_render.rs   — Render Audio + Tags info sections. Tags section omitted when no tag fields populated
      extract.rs       — Per-key extract: `package::probe` → `package::read_embed` → `InputSource::Memory`. Image bytes re-detect as Image (route through ASCII pipeline on recursive peek); lyrics text re-detect as plain text
    archive/
      mod.rs           — Module wiring (no re-exports; consumers reach in via reader / info / extract)
      reader.rs        — list_entries dispatcher (returns Vec<Entry>) + ReadSeek helper
      info.rs          — gather_extras (TOC stats via Stats::from_root) + render_section (Archive info section)
      extract.rs       — Per-format entry extract (Phase 1: spool to memory, 256 MB cap, path sanitised); zip/tar[gz/bz2/xz/zst/lz4]/7z/cpio[gz]. decompress_tar() delegates codec dispatch to crate::input::compression::decompress_bytes
      backends/
        mod.rs         — Backend module wiring
        zip.rs         — Zip TOC via central directory (no decompression); returns Vec<FlatEntry>
        tar.rs         — Tar TOC via header walk; gz/bz2/zst/lz4 stream-decompress, xz batch-decompresses (lzma-rs has no streaming Read wrapper)
        sevenz.rs      — 7-Zip TOC via sevenz-rust2 (header-only)
        cpio.rs        — cpio TOC via hand-rolled newc (`070701`/`070702`) + ODC (`070707`) header walker. CpioReader state machine drives both list (skip bodies) and extract (read matched body). plain + gz wrappers; old-binary cpio not supported
    listing/
      mod.rs           — Re-exports: Entry, EntryMtime, FlatEntry, Stats, ListingMode, from_flat_paths, time_from_epoch_secs
      entry.rs         — Entry / EntryKind { File | Dir { children } } / EntryMtime + epoch helper
      stats.rs         — Stats: aggregate counts / sizes computed by tree walk
      build.rs         — FlatEntry + from_flat_paths(): build hierarchical tree from path-keyed entries (synthesizes implicit dirs)
      mode.rs          — ListingMode: generic tree-style TOC view (perms, size, mtime, path) + file-selection cursor (used by archive + ISO)
    directory/
      mod.rs           — Module wiring; re-exports DirectoryMode
      read.rs          — One-level fs::read_dir → Vec<DirEntry>; sorts dirs-first then case-insensitive name; follows symlinks for kind/size/mtime, broken links surface as `?`
      mode.rs          — DirectoryMode: flat one-level listing. Selects every entry (files + dirs); prepends synthetic `..` row when canonical parent exists. Enter (Action::Descend) targets selected entry. Uses ModeId::Listing so Tab cycle / --list pickup keep working. ViewerState::push_extracted collapses dir→dir descent onto the current frame so there's no stack of directories
      info.rs          — gather_extras (FileExtras::Directory { entry / file / dir counts }) + render_section
      extract.rs       — Resolve key (single-segment filename) against parent path → InputSource::File(child_path). `..` walks up via Path::canonicalize → parent. Rejects `/` and `.`
    disk_image/
      mod.rs           — Module wiring (ISO + DMG)
      iso_pvd.rs       — Hand-rolled ISO 9660 Primary Volume Descriptor parser + Joliet / El Torito scan + root-extent locator
      iso_listing.rs   — ISO 9660 directory walker → Listing tree (Joliet preferred; depth/entry caps; no Rock Ridge) + lookup_file_range for extract
      dmg_trailer.rs   — Hand-rolled UDIF (Apple Disk Image) "koly" trailer parser (last 512 bytes)
      extract.rs       — ISO entry extract: lookup_file_range → zero-copy FileRange (or Bytes::slice for stdin-piped); DMG returns Unsupported
      info_gather.rs   — gather_extras: ISO reads 16 KiB at offset 32768; DMG reads tail 512 bytes
      info_render.rs   — render_section (Disk Image info section, ISO + DMG blocks)
  viewer/
    mod.rs             — Registry, compose_modes, syntax_token_for, highlight_lines, LineStreamHighlighter
    interactive.rs     — Unified event loop driving a Vec<Box<dyn Mode>> stack; routes raw keys to active prompt overlay when one is open
    modes/
      mod.rs           — Mode trait, ModeId, RenderCtx, ExtractTarget (extract_target hook: EntryPath / FrameIndex)
      content.rs       — ContentMode: streamed text / syntax / structured / SVG XML source (LineSource-backed)
      hex.rs           — HexMode: byte-offset-scrolled hex dump (interactive + pipe stream)
      info.rs          — InfoMode: file metadata view
      help.rs          — HelpMode: keyboard-shortcut listing
      about.rs         — AboutMode: logo, version, palette swatches, tips
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
docs/
  architecture.md      — Design, data flow, key abstractions, extension guide
  features.md          — Currently shipped features (✅ implemented + ◐ partial)
  planned.md           — Planned features and ideas (☐ planned + ❓ open)
  conventions.md       — Coding conventions
  release.md           — Release pipeline, install.sh, recovery from failed runs
  theme-conversion.md  — How to port VS Code / IDEA themes to peek .tmTheme
  svg-anim-perf.md     — SVG animation memory profile + optimization options
  css-info-plan.md     — Plan for rich CSS info view + lightningcss adoption
.github/workflows/
  release.yml          — Manual-dispatch release workflow (5-target build matrix)
install.sh             — POSIX installer for curl | sh on macOS/Linux
```

## Workflow

- **Don't commit unless asked.** The user decides what and when.
- **Run `cargo fmt` after editing Rust code** so formatting drift doesn't pile up across unrelated
  files. Cheap; keeps diffs focused on real changes.

## Collaboration

Three north stars:

1. **Clean, robust, maintainable architecture.** New abstractions earn their place by reducing total
   surface area or making extension easier. Modules have narrow responsibilities. `main.rs` stays
   short — file-type-specific logic lives in `compose_modes` and the modes themselves.
2. **Stream, don't load.** Multi-GB files are first-class. Prefer
   `InputSource::open_byte_source()` (random access) or chunked iteration over `read_bytes()` /
   `read_text()` (whole-file). Whole-file reads only when the feature truly needs it (full-file
   pretty-print of structured data, image decode) — never as a casual default.
3. **Keep cognitive load low.** What matters is what the next reader has to hold in their head.
   Abstractions can reduce that load (named trait → stop thinking about mechanism) or add to it
   (chasing four files for one operation). Inlining cuts both ways. Type count, line count, and
   call-site count aren't the test — what the reader has to track is.

Be a critical collaborator. Push back when a change would:

- **Damage architecture quality** — leak abstractions, blur boundaries, conflate orthogonal
  concerns (mixing print-mode + interactive paths), or re-introduce a `match file_type` chain that
  `compose_modes` was meant to eliminate.
- **Add cognitive load without payoff** — deep branching, scattered state synced by hand, mechanism
  leaking through call sites, indirection that doesn't earn the click-through, hypothetical-future
  abstractions whose concept isn't real yet.
- **Hurt performance** — redundant re-renders, hot-path allocations, full-file reads where streaming
  or seeking would do, eager work that should be lazy.

Surface the trade-off concretely; propose an alternative.

## Conventions

[docs/conventions.md](docs/conventions.md).

## Documentation

Keep these in sync with code changes:

- **README.md** — project overview, feature summary, usage examples
- **docs/architecture.md** — design, data flow, key abstractions, how to extend
- **docs/features.md** — currently shipped features (✅ + ◐)
- **docs/planned.md** — planned features and open ideas (☐ + ❓)
- **docs/conventions.md** — coding conventions
- **docs/release.md** — release pipeline and recovery
- **CLAUDE.md** — architecture map (when files / modules are added, moved, or removed)
