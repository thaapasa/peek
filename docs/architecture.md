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
4. **Compose modes, not viewers.** New file types compose a list of view modes
   (text-extract, render-preview, hex, info, …) and hand it to one event
   loop, instead of forking a new interactive viewer per type.

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
  +-- TTY?  --> Registry::compose_modes() --> Vec<Box<dyn Mode>>
  |               |
  |               v
  |             interactive::run() --> event loop on the mode stack
  |
  +-- Pipe? --> Registry::viewer_for(file_type) --> &dyn Viewer
                  |
                  v
                Viewer::render() --> pager::Output --> stdout
```

### InputSource (`input/source.rs`)

`InputSource` decouples "where data comes from" from "how it's displayed". The
`File` variant holds a `PathBuf` and reads on demand; the `Stdin` variant holds
pre-buffered bytes. All viewers and modes take `&InputSource` and call
`read_text()` or `read_bytes()` — image, animation, and SVG modes decode from
either path or memory as needed.

For random-access reads without loading the whole file, `InputSource` also
exposes `open_byte_source() -> Box<dyn ByteSource>`. `HexMode` uses this to
seek-and-read just the visible window per scroll. The `File` implementation
holds an open `File` handle and seeks per call; the `Stdin` implementation
slices the already-buffered bytes.

When stdin is consumed (because `-` is passed or no args are given with a piped
stdin), `input/stdin.rs` reopens fd 0 from the controlling terminal so the
interactive event loop can still read keystrokes. The path is resolved via
`ttyname()` on stderr/stdout rather than opening `/dev/tty` directly — macOS
kqueue rejects `/dev/tty` with EINVAL when mio tries to register it. Stdin
detection uses magic bytes (images, binary) followed by content sniffing
(leading `{`/`[` → JSON, `<` → XML/SVG, `---` → YAML) in
`input::detect::detect_bytes()`.

## Key abstractions

### Mode trait (`viewer/modes/mod.rs`) — interactive

```rust
pub(crate) trait Mode {
    fn id(&self) -> ModeId;
    fn label(&self) -> &str;
    fn render(&mut self, ctx: &RenderCtx) -> Result<Vec<String>>;

    fn owns_scroll(&self) -> bool { false }
    fn scroll(&mut self, _action: Action) -> bool { false }
    fn rerender_on_resize(&self) -> bool { false }
    fn on_resize(&mut self) {}
    fn status_segments(&self, _theme: &PeekTheme) -> Vec<(String, Color)> { vec![] }
    fn extra_actions(&self) -> &'static [(Action, &'static str)] { &[] }
    fn handle(&mut self, _action: Action) -> bool { false }
    fn next_tick(&self) -> Option<Duration> { None }
    fn tick(&mut self) -> bool { false }
    fn tracks_position(&self) -> bool { false }
    fn take_warnings(&mut self) -> Vec<String> { vec![] }
}
```

A `Mode` is one renderable + interactive view of a file. The interactive
viewer drives a `Vec<Box<dyn Mode>>`: Tab cycles modes (with `i`/`h`/`x`
shortcuts to Info/Help/Hex), and each mode declares what it owns. Today's
modes:

| Mode               | Used by                                          | Owns scroll? | Reacts to resize? |
| ------------------ | ------------------------------------------------ | ------------ | ----------------- |
| `ContentMode`      | text, source, structured, SVG XML                | no           | no                |
| `HexMode`          | binary; reachable from any view via `x`          | **yes** (byte-aligned) | **yes** |
| `ImageRenderMode`  | raster + rasterized SVG                          | no           | **yes**           |
| `AnimationMode`    | GIF / WebP (drives `next_tick`/`tick`)           | no           | **yes**           |
| `InfoMode`         | every file (file metadata)                       | no           | no                |
| `HelpMode`         | every file (keyboard-shortcut listing)           | no           | no                |

### Viewer trait (`viewer/mod.rs`) — piped output

```rust
pub trait Viewer {
    fn render(&self, source: &InputSource, file_type: &FileType, output: &mut Output) -> Result<()>;
}
```

Used only for the pipe/pager output path — one shot, no event loop. Each
piped viewer (syntax, structured, image, SVG, text, hex) implements this.

### ViewerState (`viewer/ui/state.rs`)

The interactive controller: holds the mode list, the active index, a
`last_primary` slot (the most recent non-aux mode the user was on), per-mode
scroll offsets, a lazy per-mode rendered-lines cache, and a `Position`
(the last known logical location in the source). Builds a `RenderCtx`
(carrying source, detected file type, file info, theme) and dispatches
it to the active mode.

`apply()` handles global actions (scroll, theme cycle, mode switching). The
event loop checks the active mode's `scroll()` and `handle()` before
falling through to globals; this keeps mode-local actions (`r` raw/pretty,
`b` background) cleanly scoped.

### Position tracking

`Position` (`Unknown` / `Byte(u64)` / `Line(usize)`) is captured from the
outgoing mode and pushed to the incoming mode on every active-mode change.
Modes that override `tracks_position()` participate; the rest pass it
through untouched, so detours through Info / Help / Image / Animation
preserve where you were in the file. Conversion lives on `InputSource`
(`byte_to_line`, `line_to_byte` — chunked 64 KB streaming scan via
`open_byte_source`, never a whole-file load).

Pretty-printed structured content has more lines than the raw source, so
the displayed line index has no meaningful relation to source bytes.
`ContentMode` opts out of position tracking when pretty mode is active
(`tracks_position()` returns `!use_pretty`) — switching from pretty
Content to Hex preserves whichever byte Hex was previously on instead of
synthesizing a wrong one. PDF-style modes that need exact mapping will
eventually carry their own line-to-source-byte table inside the mode.

### Registry (`viewer/mod.rs`)

Factory built once from CLI args. Holds the shared `ThemeManager`. Provides
`viewer_for(file_type) -> &dyn Viewer` for piped output and
`compose_modes(source, detected, args) -> Vec<Box<dyn Mode>>` for the
interactive path.

### Hex viewer + HexMode (`viewer/hex.rs` + `viewer/modes/hex.rs`)

`viewer/hex.rs` keeps only the piped `HexViewer` (writes the whole file as
hex via `format_row`) and the layout helpers — `bytes_per_row` (`14 + 4*bpr`
columns; rounded to a multiple of 8), `align_down`, `max_top`, `format_row`,
`pipe_bytes_per_row`. Layout matches `hexdump -C`. Pipe mode honors `$COLUMNS`
(≥ 24) or falls back to 16.

`HexMode` is the interactive half: it owns a `Box<dyn ByteSource>` plus a
`top_offset: u64` aligned to the current `bytes_per_row`. It returns
`owns_scroll() = true` so `ViewerState`'s line-scroll is suppressed; it
handles ScrollUp/Down/PageUp/Down/Top/Bottom byte-wise via `scroll()`.
`on_resize()` re-aligns `top_offset` to the new column count.

### ContentMode (`viewer/modes/content.rs`)

Holds the eagerly-loaded raw text plus a lazy pretty-print slot. The
pretty-print only runs the first time the user lands on pretty mode — for
files where the user opens with `--raw` or never toggles, the parse never
happens. On parse failure ContentMode caches the `Err`, falls back to the
raw view, and queues a one-shot warning via `take_warnings()`.
`ViewerState` polls `take_warnings()` after each render and merges new
entries into `FileInfo.warnings`, invalidating InfoMode's cached lines so
the next `i` view shows the new warning alongside any extension-mismatch
notices.

### Animation (`viewer/modes/animation.rs` + `viewer/image/animate.rs`)

`viewer/image/animate.rs` decodes GIF/WebP frames up front (`decode_anim_frames`),
counts frames cheaply via `anim_frame_count` (header-only via the `gif`
crate; WebP has no equivalent and returns `None`), and exports
`render_frame` for use by the mode. The composition decision —
`AnimationMode` for animated images, `ImageRenderMode` for static — lives
inside `Registry::compose_modes`, so `main.rs` has one uniform interactive
path across file types. `AnimationMode` owns the frame list, `current`
index, `playing` flag, `last_advance` instant, and an `ImageConfig`. It
drives the unified event loop's timeout via `next_tick()` (returns the
remaining duration until the next frame, or `None` when paused or when the
user navigates to Info/Help/Hex). When the loop's `event::poll` times out,
it calls `tick()`, which advances `current` and signals a redraw.

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

## Event loop (`viewer/interactive.rs`)

```
state = ViewerState::new(source, detected, theme, modes)
loop {
    timeout = state.active_next_tick().unwrap_or(<long>)
    if !event::poll(timeout) {
        // timeout: tick the active mode (animation frame advance)
        if state.tick_active() { state.invalidate_active(); redraw }
        continue
    }

    Event::Key(key) =>
        let action = state.dispatch_key(key)         // mode extras + globals
        try state.try_active_scroll(action)          // byte-offset for hex
            -> consumed: invalidate + redraw
        try state.try_active_handle(action)          // toggle pretty, cycle bg
            -> consumed: invalidate + redraw
        match state.apply(action)                    // global dispatch
            Quit | Redraw | Unhandled
    Event::Resize =>
        state.handle_resize()                        // on_resize + invalidate
        redraw
}
```

`redraw` calls `state.ensure_active_rendered()` (lazy mode render),
composes the status line (name, mode label, mode's status segments, theme),
then `state.draw()`.

### Toggle semantics: Tab, `i`, `h`, `x`

Aux modes (Info, Help, Hex) are reachable only via dedicated keys — they
don't appear in the `r` primary cycle. `ViewerState::toggle_aux(target_id)`
is shared by Tab (Info), `h` (Help), and `x` (Hex): if the active mode
*is* the target, return to `last_primary`; otherwise, enter the target.
`i` (`SwitchInfo`) is a one-way jump to Info.

`last_primary` is updated whenever the active mode lands on a non-aux
mode. Aux-to-aux transitions (Hex → Info, Info → Hex) leave it alone, so
the path back to "your actual work" survives any number of detours —
Hex → Info → Tab returns to the original primary, not to Hex.

For binary files (where the stack is `[Hex, Info, Help]` and there's no
primary), `last_primary` stays `None`; exiting an aux falls back to mode
0 (Hex itself), so pressing `x` from standalone hex is a no-op — matching
the old behavior.

## Adding a new file type

1. Add a variant to `FileType` in `input/detect.rs` and wire detection logic.
2. (Piped path) Create a `Viewer` impl in `viewer/`; register it in `Registry`
   and add a `viewer_for()` arm.
3. (Interactive path) Build one or more `Mode` impls under `viewer/modes/` —
   each owns its render + scroll + extra-action state. Add a `ModeId` variant
   if the new mode needs to be toggleable by id.
4. Wire the new modes into `Registry::compose_modes` for that file type. The
   universal Hex / Info / Help modes are appended automatically.
5. Add info gathering in `info/gather.rs` if the type has interesting metadata
   (and themed display in `info/render.rs` for novel field types).

Example — adding a PDF type:

```rust
// in compose_modes
FileType::Pdf => {
    modes.push(Box::new(PdfTextMode::new(source.clone())?));    // text extract
    modes.push(Box::new(PdfRenderMode::new(source.clone())?));  // page preview
}
```

The user gets Tab cycling between text extract ↔ Info, with `r` cycling
between text/render primaries and `x` toggling to hex — all without
touching `main.rs` or the event loop.

## Adding a new theme

1. Create a `.tmTheme` file in `themes/`.
2. Add a `PeekThemeName` variant in `theme.rs`.
3. Wire: `include_str!()`, `cli_name()`, `tmtheme_source()`, `next()`,
   `help_text()`.
4. `PeekTheme` semantic roles derive automatically from the syntect theme.
