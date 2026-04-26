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
    pager.rs           — Output abstraction (pager / direct stdout)
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
      animate.rs       — GIF/WebP frame decoding + frame counting + render_frame
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

## Collaboration

The project's three north stars:

1. **Architecture is clean, robust, and maintainable.** New abstractions earn their place by reducing total surface area or making extension easier. Modules have clear, narrow responsibilities. The dispatcher in `main.rs` should stay short — file-type-specific logic lives in `compose_modes` and the modes themselves.
2. **Stream, don't load.** The viewer should handle multi-GB files comfortably. Prefer `InputSource::open_byte_source()` for random-access reads (HexMode does this) or chunked iteration over `read_bytes()` / `read_text()`, which load the whole file into memory. Whole-file reads are acceptable only when the feature genuinely needs them (full-file pretty-print of structured data, image decode) — never as a casual default.
3. **Keep cognitive load low.** What matters here is how much a reader has to track to understand a piece of code — branches, scattered state, layers of indirection, concerns tangled together. Abstractions can *reduce* that load (a well-named trait lets you stop thinking about mechanism) or *add* to it (chasing through four files to follow one operation). Inlining can do either too — sometimes everything-visible is the right call, sometimes it's a 200-line function with three concerns mixed in. The test is what the next reader has to hold in their head; type count, line count, and call-site count are not the test.

Be a critical collaborator, not an order-taker. Push back when a proposed change would:

- **Deteriorate architecture quality** — leak abstractions, blur module boundaries, conflate orthogonal concerns (e.g. mixing piped-output and interactive paths), or re-introduce a `match file_type` chain that `compose_modes` was meant to eliminate.
- **Add cognitive load without payoff** — deep branching, state scattered across structs that has to be kept in sync by hand, mechanism that leaks through several call sites instead of being hidden behind one, layers of indirection that don't earn the click-through cost, hypothetical-future abstractions whose concept isn't real yet. Whether the fix is a new abstraction, inlining what's there, or restructuring is the judgment call.
- **Worsen performance** — redundant re-renders, extra allocations on hot paths, full-file reads where streaming or seeking would do, eager work that should be lazy.

Surface the trade-off concretely and propose an alternative — the user wants help finding the best path, not the path of least resistance.

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
