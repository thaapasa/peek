# Architecture

This document describes how peek is structured and how the pieces fit together.
For the file map, see [CLAUDE.md](../CLAUDE.md). For coding rules, see
[conventions.md](conventions.md).

## Design principles

1. **Zero runtime dependencies.** Everything is compiled in — themes, glyph
   bitmaps, syntax definitions. No config files, no downloads, no setup.
2. **Two output paths.** TTY gets an interactive viewer (alternate screen,
   scrolling, key bindings). Pipe gets plain streamed output. Same rendering
   logic, different output targets.
3. **Theme-aware everything.** All colored output uses `PeekTheme` semantic
   roles. Switching themes re-renders the entire view without re-reading files.
4. **Add viewers, not complexity.** New file types should mean a new `Viewer`
   implementation, not changes to the core event loop.

## Data flow

```
CLI args (clap)
  |
  v
build_sources() --> Vec<InputSource>  (File paths, or buffered Stdin)
  |
  v
detect::detect(source) --> FileType
  |
  v
Registry::viewer_for(file_type) --> &dyn Viewer
  |
  +-- TTY? --> content_renderer() closure --> interactive viewer
  |                                            (ui::ViewerState + event loop)
  |
  +-- Pipe? --> Viewer::render() --> pager::Output --> stdout
```

### InputSource (`input.rs`)

`InputSource` decouples "where data comes from" from "how it's displayed". The
`File` variant holds a `PathBuf` and reads on demand; the `Stdin` variant holds
pre-buffered bytes. All viewers take `&InputSource` and call `read_text()` or
`read_bytes()` — image, animation, and SVG viewers decode from either path or
memory as needed.

For random-access reads without loading the whole file, `InputSource` also
exposes `open_byte_source() -> Box<dyn ByteSource>`. The hex viewer uses this
to seek-and-read just the visible window per scroll. The `File` implementation
holds an open `File` handle and seeks per call; the `Stdin` implementation
slices the already-buffered bytes.

When stdin is consumed (because `-` is passed or no args are given with a piped
stdin), `main.rs` reopens fd 0 from the controlling terminal so the interactive
event loop can still read keystrokes. The path is resolved via `ttyname()` on
stderr/stdout rather than opening `/dev/tty` directly — macOS kqueue rejects
`/dev/tty` with EINVAL when mio tries to register it. Stdin detection uses
magic bytes (images, binary) followed by content sniffing (leading `{`/`[` →
JSON, `<` → XML/SVG, `---` → YAML) in `detect::detect_bytes()`.

### TTY path (interactive)

1. `Registry::content_renderer()` captures the file content and returns a
   `Fn(PeekThemeName, bool) -> Result<Vec<String>>` closure. The closure
   re-renders content for any theme without re-reading the file.
2. `interactive::view_interactive()` enters the alternate screen and creates a
   `ViewerState` with the initial render.
3. The event loop calls `state.handle_key()` for shared bindings (quit, scroll,
   view switching, theme cycling) and handles viewer-specific keys locally.
4. On theme change, `ViewerState` re-renders info and help internally; the
   caller re-renders content via the closure.

### Pipe path (direct output)

`Viewer::render()` writes lines to `pager::Output`, which forwards to stdout
or the built-in pager. No interactive state, no alternate screen.

## Key abstractions

### Viewer trait (`viewer/mod.rs`)

```rust
pub trait Viewer {
    fn render(&self, source: &InputSource, file_type: &FileType, output: &mut Output) -> Result<()>;
}
```

Used for the pipe/pager output path. Each viewer (syntax, structured, image,
SVG, text) implements this. The interactive path uses a different mechanism
(closures + `ViewerState`) because it needs re-rendering on theme/resize.

### Registry (`viewer/mod.rs`)

Factory that creates all viewers from CLI args and dispatches by `FileType`.
Also builds `ContentRenderer` closures for the interactive path. Holds the
shared `ThemeManager` via `Rc`.

### ViewerState (`viewer/ui.rs`)

Shared state for interactive viewers: view mode, theme, scroll offsets, and
content/info/help line buffers. Provides `handle_key()` for common key
bindings and `draw()` for screen rendering. Both `interactive.rs` and
`animate.rs` create a `ViewerState` and layer their own keys on top.

### Hex viewer (`viewer/hex.rs`)

A streaming hex-dump viewer that becomes the default rendering for
`FileType::Binary` and is reachable from any other viewer via the `x` key.
Layout matches `hexdump -C` (`offset  hex bytes  |ASCII|`) and bytes-per-row
is computed from terminal width (`14 + 4*bpr` columns; rounded down to a
multiple of 8). Pipe mode honors `$COLUMNS` (≥ 24) or falls back to 16.

The interactive event loop owns its own scrolling state (a `top_offset: u64`
aligned to bytes-per-row) and reads `rows × bpr` bytes from a `ByteSource`
per redraw. Info, Help, and theme cycling are delegated to a `ViewerState`
with empty `content_lines`. Pressing `x` from another viewer enters hex
positioned at the byte offset corresponding to the current top line
(translated via `compute_byte_offset_for_line` in `interactive.rs`); pressing
`x` again returns. When hex is the standalone default for a binary file, `x`
is a no-op.

### ImageConfig (`viewer/image/mod.rs`)

Bundles image rendering parameters (mode, width, background, margin) into a
single struct passed through the image pipeline.

### PeekTheme (`theme.rs`)

Semantic color roles derived from syntect `.tmTheme` files. All colored output
goes through `PeekTheme::paint()`. Color interpolation via `lerp_color()` for
continuous scales (file size, age, resolution).

## Image rendering pipeline

```
source image/SVG
  |
  v
add_margin() --> transparent padding
  |
  v
contain_size() --> aspect-ratio-preserving grid dimensions (cols x rows)
  |
  v
resize_exact() --> target pixel resolution (cols*CELL_W x rows*CELL_H)
  |
  v
composite_with_bg() --> resolve alpha (auto/black/white/checkerboard)
  |
  v
render_block_color() or render_density()
  |  Per cell (8x16 pixels):
  |    fast_2_color() --> 2 cluster colors + u128 bitmap
  |    best_glyph()   --> Hamming-distance match against glyph atlas
  |    emit ANSI fg/bg + character
  |
  v
Vec<String> lines
```

**Critical order:** resize *before* composite. This ensures the checkerboard
pattern aligns to the glyph grid at the final resolution.

### Animation

Animated GIFs/WebPs use a separate event loop in `animate.rs` that calls
`event::poll(timeout)` instead of blocking `event::read()`. Frame timing
drives the poll timeout. All frames are decoded up front into `Vec<AnimFrame>`.
The loop shares `ViewerState` with the static viewer for common key handling.

## Interactive viewer structure

Both event loops follow the same pattern:

```
ViewerState::new(source, file_type, theme, content_lines, help_keys)
  |
  v
loop {
    read/poll event
      |
      v
    state.handle_key(key) --> KeyAction
      |
      +-- Quit --> break
      +-- Redraw --> state.draw(stdout, &status_line)
      +-- ThemeChanged --> re-render content, then draw
      +-- Unhandled(key) --> handle viewer-specific keys
}
```

**Adding a new shared key binding:** edit `ViewerState::handle_key()` in
`ui.rs`. Both viewers pick it up automatically.

**Adding a viewer-specific key:** handle it in the `Unhandled` arm of that
viewer's event loop.

**Switching to hex from another viewer:** `ViewerState::handle_key()` returns
`KeyAction::SwitchToHex` for the `x` key. Each event loop calls
`hex::run_hex_loop(stdout, source, file_type, theme, byte_offset, true)` from
inside its existing alternate screen, then redraws on return. The byte offset
is computed by the caller (text/code: source-newline scan via
`compute_byte_offset_for_line`; image/animation: 0, since position is
meaningless for those modes).

## Adding a new file type

1. Add a variant to `FileType` in `detect.rs` and wire detection logic.
2. Create a new viewer struct implementing `Viewer` in `viewer/`.
3. Register it in `Registry` (`viewer/mod.rs`) and add dispatch in
   `viewer_for()`.
4. For interactive support: create a `ContentRenderer` closure in
   `content_renderer()` or add a `view_interactive()` method.
5. Wire the new type in `main.rs`'s TTY/pipe branches.
6. Add info gathering in `info.rs` if the type has interesting metadata.

## Adding a new theme

1. Create a `.tmTheme` file in `themes/`.
2. Add a `PeekThemeName` variant in `theme.rs`.
3. Wire: `include_str!()`, `cli_name()`, `tmtheme_source()`, `next()`,
   `help_text()`.
4. `PeekTheme` semantic roles derive automatically from the syntect theme.
