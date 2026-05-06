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

No external runtime dependencies. Image rendering is built in.

## Architecture map

```
src/
  main.rs              — CLI entry point: dispatches inputs to viewers
  cli.rs               — Args struct (clap derive)
  input/
    mod.rs             — re-exports InputSource, ByteSource, LineSource
    source.rs          — InputSource enum (File path or buffered Stdin), ByteSource trait
    lines.rs           — LineSource: streaming, anchor-indexed line view over InputSource
    detect.rs          — File-type detection (extension + magic bytes + stdin sniffing)
    stdin.rs           — Build the input source from CLI args, reopen fd 0 from /dev/tty after pipe
  output/
    mod.rs             — re-exports PrintOutput
    print.rs           — PrintOutput: write-once stdout for --print / pipes / --info
    help.rs            — CLI help and version screens
  info/
    mod.rs             — FileInfo, FileExtras data types and shared permission helpers
    gather/            — FileInfo collection, split per general file type
      mod.rs           — Per-source dispatch (gather() entry point)
      image.rs         — Image extras: dimensions, color, ICC, HDR
      exif.rs          — EXIF field extraction
      xmp.rs           — XMP packet scrape (Dublin Core / xmp tags)
      animation.rs     — GIF/WebP animation stats (frames, duration, loop)
      text.rs          — Streaming text stats + BOM-based encoding
      svg.rs           — SVG-specific extras (viewBox, element counts, security)
      tests.rs         — Fixture-based tests against test-images / test-data
    render/            — Themed terminal rendering of FileInfo, split per section
      mod.rs           — render() entry, RenderOptions, shared push_field/section_header/paint_count
      file.rs          — File section: name, path, size, MIME, timestamps, permissions
      image.rs         — Image section: dimensions, megapixels, animation, EXIF/XMP
      svg.rs           — SVG section: viewBox, element counts, security flags
      text.rs          — Text/Source section: line/word counts, encoding, indent labels
    time.rs            — UTC ISO / local-with-offset timestamp formatting (libc::localtime_r)
  theme/
    mod.rs             — re-exports PeekThemeName, ColorMode, PeekTheme, ThemeManager, helpers
    name.rs            — PeekThemeName + embedded .tmTheme data + load_embedded_theme
    color_mode.rs      — ColorMode (truecolor/256/16/grayscale/plain) + RGB→palette conversion
    peek_theme.rs      — PeekTheme semantic roles + paint helpers + lerp_color/blend
    manager.rs         — ThemeManager: shared SyntaxSet/ThemeSet + active PeekTheme
  types/
    mod.rs             — Per-file-type modules (each owns reader + info + view-mode)
    binary/
      mod.rs           — Module wiring
      info.rs          — gather_extras (friendly format label) + render_section (Format)
    structured/
      mod.rs           — Module wiring
      info.rs          — gather_extras (per-format stats) + render_section (Format)
      pretty.rs        — JSON / YAML / TOML / XML pretty-printers (used by ContentMode)
    archive/
      mod.rs           — Module wiring; re-exports ArchiveMode
      reader.rs        — ArchiveEntry / ArchiveMtime / ArchiveStats / list_entries dispatch + ReadSeek helper
      info.rs          — gather_extras (TOC stats) + render_section (Archive info section)
      mode.rs          — ArchiveMode: tree-style TOC view (perms, size, mtime, path)
      backends/
        mod.rs         — Backend module wiring
        zip.rs         — Zip TOC via central directory (no decompression)
        tar.rs         — Tar TOC via header walk; gz/bz2/zst stream-decompress, xz batch-decompresses (lzma-rs has no streaming Read wrapper)
        sevenz.rs      — 7-Zip TOC via sevenz-rust2 (header-only)
  viewer/
    mod.rs             — Registry, compose_modes, syntax_token_for, highlight_lines, LineStreamHighlighter
    interactive.rs     — Unified event loop driving a Vec<Box<dyn Mode>> stack
    modes/
      mod.rs           — Mode trait, ModeId, RenderCtx; render_to_pipe for print path
      content.rs       — ContentMode: streamed text / syntax / structured / SVG XML source (LineSource-backed)
      hex.rs           — HexMode: byte-offset-scrolled hex dump (interactive + pipe stream)
      image_render.rs  — ImageRenderMode: raster + rasterized SVG
      animation.rs     — AnimationMode: GIF/WebP playback (next_tick / tick driven)
      svg_animation.rs — SvgAnimationMode: CSS `@keyframes` SVG playback (per-frame rasterize + LRU cache)
      info.rs          — InfoMode: file metadata view
      help.rs          — HelpMode: keyboard-shortcut listing
      about.rs         — AboutMode: logo, version, palette swatches, tips
    ui/
      mod.rs           — with_alternate_screen, status line composer, terminal-size helpers
      state.rs         — ViewerState: mode stack, active index, scroll, lazy line cache
      keys.rs          — Action enum (centralized keybindings), Outcome
      help.rs          — Keyboard-shortcut help screen renderer
    hex.rs             — Hex layout primitives + format_row (used by HexMode)
    image/
      mod.rs           — Module wiring + Background / ImageConfig generic types
      mode.rs          — ImageMode enum (full/block/geo/ascii/contour palette selection)
      svg.rs           — SVG rasterization (resvg): svg_dimensions / rasterize_svg / rasterize_svg_bytes
      svg_anim/
        mod.rs         — Public API: try_parse / try_parse_bytes / render_frame, AnimatedSvg, Frame; parse_text orchestrator
        scan.rs        — quick-xml walk: byte-span collection of animated elements + <style> text
        spec.rs        — Inline-style animation: / animation-* parser → AnimSpec
        keyframes.rs   — CSS @keyframes rule parser → KeyframeStop, TransformValue
        timeline.rs    — Merged frame timeline: build_frames, sample_target (steps + linear)
        marker.rs      — __PEEK_ANIM_*__ marker injection + per-frame substitution
        util.rs        — Shared helpers: skip_ws, find_substr/brace, parse_length, root_svg_dimensions
      render.rs        — Image → glyph-matched ASCII art with true color (incl. prepare_svg_bytes)
      animate.rs       — GIF/WebP frame decoding + frame counting + render_frame
      glyph_atlas.rs   — Precomputed glyph bitmaps
      clustering.rs    — Two-color clustering for cell rendering
      contour.rs       — Sobel + Otsu edge detection for ImageMode::Contour
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
