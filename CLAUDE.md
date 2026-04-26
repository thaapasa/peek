# peek

Modern terminal file viewer with syntax highlighting, structured data pretty-printing, and image rendering.

## Build & Run

```sh
cargo build                  # debug build
cargo build --release        # release build
cargo run -- [args]          # run with arguments
cargo test                   # run all tests
cargo clippy                 # lint
```

No external runtime dependencies. Image rendering is built-in.

## Architecture

```
src/
  main.rs              — CLI entry point: dispatches inputs to viewers
  cli.rs               — Args struct (clap derive)
  input/
    mod.rs             — re-exports InputSource, ByteSource
    source.rs          — InputSource enum (File path or buffered Stdin), ByteSource trait
    detect.rs          — File type detection (extension + magic bytes + stdin sniffing)
    stdin.rs           — Build sources from CLI args, reopen fd 0 from /dev/tty after pipe
  output/
    mod.rs             — re-exports Output
    pager.rs           — Output abstraction (pager / direct stdout / buffer)
    help.rs            — CLI help and version screens
  info/
    mod.rs             — FileInfo, FileExtras data types and shared permission helpers
    gather.rs          — FileInfo collection: FS metadata, MIME, EXIF, HDR, text/image extras
    render.rs          — Themed terminal rendering of FileInfo
  theme.rs             — Theme management, PeekTheme semantic colors, color blending
  viewer/
    mod.rs             — Viewer trait (piped output), Registry, compose_modes, highlight_lines
    interactive.rs     — Unified event loop driving a Vec<Box<dyn Mode>> stack
    modes/
      mod.rs           — Mode trait, ModeId, RenderCtx (the interactive abstraction)
      content.rs       — ContentMode: text / syntax / structured / SVG XML source
      hex.rs           — HexMode: byte-offset-scrolled hex dump
      image_render.rs  — ImageRenderMode: raster + rasterized SVG
      animation.rs     — AnimationMode: GIF/WebP playback (next_tick / tick driven)
      info.rs          — InfoMode: file metadata view
      help.rs          — HelpMode: keyboard-shortcut listing
    ui/
      mod.rs           — with_alternate_screen, status line composer, terminal-size helpers
      state.rs         — ViewerState: mode stack, active index, scroll, lazy line cache
      keys.rs          — Action enum (centralized keybindings), Outcome
      help.rs          — Keyboard-shortcut help screen renderer
    syntax.rs          — Piped-output syntax-highlighted source code (Viewer impl)
    structured.rs      — JSON/YAML/TOML/XML pretty-print + Viewer impl for piped output
    text.rs            — Plain text Viewer impl for piped output
    hex.rs             — Hex dump Viewer impl for piped output + shared layout helpers
    image/
      mod.rs           — ImageViewer / SvgViewer: piped-output Viewer impls; ImageConfig
      render.rs        — Image → glyph-matched ASCII art with true color
      animate.rs       — Frame decoding + viewer entry that composes AnimationMode
      svg.rs           — SVG rasterization via resvg
      glyph_atlas.rs   — Precomputed glyph bitmaps
      clustering.rs    — Two-color clustering for cell rendering
themes/
  islands-dark.tmTheme — JetBrains Islands-inspired dark theme (default)
  dark-2026.tmTheme    — VS Code Dark 2026-inspired theme
  vivid-dark.tmTheme   — High-contrast vivid dark theme
docs/
  architecture.md      — Design, data flow, key abstractions, extension guide
  features.md          — Feature specification and status tracking
  conventions.md       — Coding conventions
  release.md           — Release pipeline, install.sh, recovery from failed runs
.github/workflows/
  release.yml          — Manual-dispatch release workflow (5-target build matrix)
install.sh             — POSIX installer for curl | sh on macOS/Linux
```

## Workflow

- **Do not commit unless explicitly asked.** The user decides when and what to commit.

## Conventions

See [docs/conventions.md](docs/conventions.md) for coding conventions.

## Documentation

Keep documentation up to date when making changes. In particular:

- **README.md** — project overview, feature summary, usage examples
- **docs/architecture.md** — design, data flow, key abstractions, how to extend
- **docs/features.md** — feature specification and implementation status
- **docs/conventions.md** — coding conventions and patterns
- **docs/release.md** — release pipeline and recovery procedures
- **CLAUDE.md** — architecture map (if files/modules are added, moved, or removed)
