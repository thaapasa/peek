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
  main.rs              — CLI entry point (clap), file type dispatch
  viewer/
    mod.rs             — Viewer trait, Registry, highlight_lines helper
    ui.rs              — Shared viewer UI: ViewerState, ScrollState, key handling, drawing
    interactive.rs     — Static content interactive viewer (text, code, structured data)
    syntax.rs          — Syntax-highlighted source code (syntect)
    structured.rs      — JSON/YAML/TOML/XML pretty-print + syntax highlight
    text.rs            — Plain text passthrough
    image/
      mod.rs           — Image viewer (interactive + piped)
      render.rs        — Image → glyph-matched ASCII art with true color
      animate.rs       — Animated GIF/WebP playback with frame-rate-driven event loop
      svg.rs           — SVG rasterization via resvg
      glyph_atlas.rs   — Precomputed glyph bitmaps
      clustering.rs    — Two-color clustering for cell rendering
  detect.rs            — File type detection (extension + magic bytes)
  info.rs              — File metadata gathering and themed rendering
  pager.rs             — Output abstraction (pager / direct stdout)
  theme.rs             — Theme management, PeekTheme semantic colors, color blending
  help.rs              — CLI help and version screens
themes/
  islands-dark.tmTheme — JetBrains Islands-inspired dark theme (default)
  dark-2026.tmTheme    — VS Code Dark 2026-inspired theme
  vivid-dark.tmTheme   — High-contrast vivid dark theme
docs/
  architecture.md      — Design, data flow, key abstractions, extension guide
  features.md          — Feature specification and status tracking
  conventions.md       — Coding conventions
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
- **CLAUDE.md** — architecture map (if files/modules are added, moved, or removed)
