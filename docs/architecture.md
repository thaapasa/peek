# Architecture

Structure and how the pieces fit. File map: [CLAUDE.md](../CLAUDE.md). Coding
rules: [conventions.md](conventions.md).

## Design principles

1. **Single-file viewer.** One path (or stdin) at a time — closer to `less` than to `cat`.
2. **Zero runtime deps.** Themes, glyph bitmaps, and syntax definitions are compiled in. No config
   files, no downloads, no setup.
3. **One mode stack, two outputs.** `compose_modes` builds a `Vec<Box<dyn Mode>>` per file type.
   TTY → interactive viewer (alternate screen, scrolling, key bindings). Pipe → first non-aux
   mode's `render_to_pipe(ctx)` straight to stdout. Same rendering logic, different targets — no
   parallel `Viewer` trait.
4. **Theme-aware everything.** All colored output uses `PeekTheme` semantic roles. Theme switch
   re-renders the whole view without re-reading files.
5. **Compose modes, not viewers.** New file types compose a list of view modes (text-extract,
   render-preview, hex, info, …) and hand it to one event loop, instead of forking a new interactive
   viewer per type.

## Data flow

```
CLI args (clap)
  |
  v
build_source() --> InputSource  (File path, or buffered Stdin)
  |
  v
detect::detect(source) --> FileType
  |
  +-- Registry::compose_modes() --> Vec<Box<dyn Mode>>
        |
        +-- TTY?  --> interactive::run() --> event loop on the mode stack
        |
        +-- Pipe? --> first non-aux mode (or first, for binary)
                        |
                        v
                      Mode::render_to_pipe(ctx) --> PrintOutput --> stdout
```

### InputSource (`input/source.rs`)

Decouples "where data comes from" from "how it's displayed". `File` holds a `PathBuf` and reads on
demand; `Stdin` holds pre-buffered bytes. All viewers and modes take `&InputSource` and call
`read_text()` / `read_bytes()` — image, animation, and SVG modes decode from either path or memory.

For random-access reads without slurping, `open_byte_source() -> Box<dyn ByteSource>` returns a
seeking handle. `HexMode` uses this to read just the visible window per scroll. `File` seeks per
call; `Stdin` slices the buffered bytes.

For line-oriented streaming, `open_line_source() -> LineSource` (in `input/lines.rs`) does one pass
of the source to count newlines and capture sparse byte-offset anchors (every 1024 lines), then
serves windowed line lookups in O(stride) — `ContentMode` uses this so multi-GB text files never
materialize. Stdin and file go through the same path: stdin's `Arc<[u8]>` backing makes "streaming"
a zero-cost slice; file seeks per chunk via `FileByteSource`.

When stdin is consumed (`-` argument or no args + piped stdin), `input/stdin.rs` reopens fd 0 from
the controlling terminal so the event loop can still read keystrokes. Resolved via `ttyname()` on
stderr/stdout, not `/dev/tty` directly — macOS kqueue rejects the latter with EINVAL.

Stdin detection: magic bytes (images, binary) → content sniffing (leading `{`/`[` → JSON, `<` →
XML/SVG, `---` → YAML), in `input::detect::detect_bytes()`.

## Key abstractions

### Mode trait — interactive (`viewer/modes/mod.rs`)

```rust
pub(crate) struct Window { pub lines: Vec<String>, pub total: usize }

pub(crate) trait Mode {
    fn id(&self) -> ModeId;
    fn label(&self) -> &str;
    fn is_aux(&self) -> bool { false }
    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window>;
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let w = self.render_window(ctx, 0, ctx.term_rows)?;
        for line in w.lines { out.write_line(&line)?; }
        Ok(())
    }
    fn total_lines(&self) -> Option<usize> { None }

    fn owns_scroll(&self) -> bool { false }
    fn scroll(&mut self, _action: Action) -> bool { false }
    fn rerender_on_resize(&self) -> bool { false }
    fn on_resize(&mut self, _term_cols: usize, _term_rows: usize) {}
    fn status_segments(&self, _theme: &PeekTheme) -> Vec<(String, Color)> { vec![] }
    fn status_hints(&self, _has_return_target: bool) -> Vec<&'static str> { vec![] }
    fn extra_actions(&self) -> &'static [(Action, &'static str)] { &[] }
    fn handle(&mut self, _action: Action) -> Handled { Handled::No }
    fn next_tick(&self) -> Option<Duration> { None }
    fn tick(&mut self) -> bool { false }
    fn tracks_position(&self) -> bool { false }
    fn take_warnings(&mut self) -> Vec<String> { vec![] }
}

pub(crate) enum Handled { No, Yes, YesResetScroll }
```

`render_window` is the single rendering contract. The mode receives a viewport request `(scroll,
rows)` and returns the visible slice plus the full-source `total` line count — `ViewerState` writes
the slice verbatim (no further indexing) and uses `total` for scroll math. Streaming modes
(`ContentMode`) honor the window so they only fetch what's visible; fixed-content modes
(Info/Help/About) materialize their full output and pre-slice via the `slice_window` helper.
`Mode::total_lines()` lets a mode answer the line-count question cheaply when it can — `ContentMode`
returns its `LineSource.total_lines()` in O(1) so Bottom-jumps don't force a render.

`is_aux()` marks Info / Help / Hex as auxiliary so they can be reached only via dedicated keys
(Tab/i, h, x), are skipped by the `r` primary cycle, and toggle back to `last_primary`.
`status_hints` lets a mode contribute right-side hints contextually (Hex shows `x:exit hex` only
when it has somewhere to return to). `Handled::YesResetScroll` zeroes the active mode's scroll
offset (used when an action invalidates the prior position — e.g. ContentMode flipping pretty ↔
raw).

A `Mode` is one renderable + interactive view of a file. The interactive viewer drives a
`Vec<Box<dyn Mode>>`: Tab cycles modes (with `i`/`h`/`x` shortcuts to Info/Help/Hex). Today's modes:

| Mode              | Used by                                      | Owns scroll?           | Reacts to resize? |
|-------------------|----------------------------------------------|------------------------|-------------------|
| `ContentMode`     | text, source, structured, SVG XML            | no                     | no                |
| `HexMode`         | binary; reachable from any view via `x`      | **yes** (byte-aligned) | **yes**           |
| `ImageRenderMode` | raster + rasterized SVG                      | no                     | **yes**           |
| `AnimationMode`   | GIF / WebP (drives `next_tick`/`tick`)       | no                     | **yes**           |
| `InfoMode`        | every file (file metadata)                   | no                     | no                |
| `HelpMode`        | every file (keyboard-shortcut listing)       | no                     | no                |
| `AboutMode`       | every file (logo, version, palette swatches) | no                     | no                |

### Pipe-mode rendering (`Mode::render_to_pipe`)

`render_to_pipe` is the print-path entry point on every mode. The default impl materializes
`render(ctx)` and writes each line to `PrintOutput`; modes that can stream directly from a
`ByteSource` (HexMode) or that need byte-faithful raw output (ContentMode without a syntax token)
override it. `RenderCtx` injects `term_cols = $COLUMNS-or-80` and `term_rows = usize::MAX` for the
pipe path, so a single `render` body can serve both interactive and pipe contexts when bounded
viewports aren't required.

`main` picks the pipe primary as the first non-aux mode in the stack, falling back to the first
mode when all are aux (binary files, where the stack is `[Hex, Info, About, Help]`).

### ViewerState (`viewer/ui/state.rs`)

The interactive controller: mode list, active index, `last_primary` slot (most recent non-aux mode),
per-mode scroll offsets, lazy per-mode rendered-lines cache, and a `Position` (last known logical
location in the source). Builds a `RenderCtx` (source, file type, file info, theme) and dispatches
to the active mode.

`apply()` handles global actions (scroll, theme cycle, mode switch). The event loop tries the active
mode's `scroll()` and `handle()` first, then falls through to globals — so mode-local actions (`r`
raw/pretty, `b` background) stay scoped.

### Position tracking

`Position` (`Unknown` / `Byte(u64)` / `Line(usize)`) is captured from the outgoing mode and pushed
to the incoming mode on every active-mode change. Modes that override `tracks_position()`
participate; the rest pass it through. So detours through Info / Help / Image / Animation preserve
where you were. Conversion lives on `InputSource` (`byte_to_line` / `line_to_byte`, chunked 64 KB
streaming scan).

Pretty-printed structured content has more lines than the raw source, so the displayed line index
doesn't map cleanly to source bytes. `ContentMode` opts out of position tracking when pretty mode is
active (`tracks_position()` returns `!use_pretty`). Switching from pretty Content to Hex preserves
whichever byte Hex was last on, instead of synthesizing a wrong one. Modes that need exact mapping
will eventually carry their own line-to-source-byte table.

### Registry (`viewer/mod.rs`)

Factory built once from CLI args. Holds the shared `ThemeManager` plus the resolved `PeekTheme` /
`plain_mode` flags consumed during composition. Provides `compose_modes(source, detected, args)`,
the single dispatcher that produces the mode stack consumed by both the interactive event loop and
the pipe path.

### HexMode (`viewer/hex.rs` + `viewer/modes/hex.rs`)

`viewer/hex.rs` hosts the layout primitives — `bytes_per_row` (`14 + 4*bpr` columns; rounded to a
multiple of 8), `align_down`, `max_top`, `format_row`. Layout matches `hexdump -C`.

`HexMode` (`viewer/modes/hex.rs`) owns a `Box<dyn ByteSource>` plus `top_offset: u64` aligned to
the current `bytes_per_row`. Returns `owns_scroll() = true` so `ViewerState`'s line-scroll is
suppressed; handles ScrollUp/Down/PageUp/Down/Top/Bottom byte-wise via `scroll()`. `on_resize`
re-aligns `top_offset` to the new column count. `render_to_pipe` streams the whole file in 4 KB
chunks straight to the print sink — never holds more than one chunk in memory, so multi-GB hex
dumps are first-class.

### ContentMode (`viewer/modes/content.rs`)

Streams the raw view from a `LineSource` (anchor-indexed line iterator over `InputSource`); a
window-only render fetches just the visible lines per scroll, so multi-GB text never materializes.
With a syntax token, `LineStreamHighlighter` (in `viewer/mod.rs`) carries syntect `ParseState` +
`HighlightState` across `feed()` calls so multi-line constructs (block comments, here-docs)
highlight correctly. Backward scrolls past the highlighter's cursor reset and replay forward —
typical top-to-bottom reading is cheap; pathological backward jumps on huge files pay a one-time
cost. Theme cycle resets state too (cached styles are theme-derived); color cycle takes effect on
the next `feed()` without a reset.

Pretty-print is whole-file with a 16 MB cap (`PRETTY_MAX_BYTES` in `content.rs`). Above the cap
ContentMode pushes a warning, clears `use_pretty`, and the streamed raw view takes over. Below the
cap, pretty-print runs lazily on first access; the parsed text is cached, and the highlighted-pretty
form (when a syntax token is set) is cached keyed by `(theme, color)` so a cycle invalidates and
recomputes. On parse failure ContentMode caches the `Err`, falls back to raw, and queues a one-shot
warning via `take_warnings()`. `ViewerState` polls `take_warnings()` after each render and merges
new entries into `FileInfo.warnings`, invalidating InfoMode's cached lines so the next `i` view
shows the new warning alongside extension-mismatch notices.

Pipe path: highlighted output is `\n`-terminated per line (escape sequences are line-scoped);
un-highlighted preserves the source's trailing-newline status (`LineSource.ends_with_newline()`)
for byte-for-byte fidelity with `cat`.

### Animation (`viewer/modes/animation.rs` + `viewer/image/animate.rs`)

`viewer/image/animate.rs` decodes GIF/WebP frames up front (`decode_anim_frames`) and exports
`render_frame` for the mode. The composition decision — `AnimationMode` for animated images,
`ImageRenderMode` for static — lives in `Registry::compose_modes`, so `main.rs` has one uniform
interactive path across file types.

`AnimationMode` owns the frame list, `current` index, `playing` flag, `last_advance` instant, and an
`ImageConfig`. It drives the unified event loop's timeout via `next_tick()` (remaining duration to
next frame, or `None` when paused / on detour to Info / Help / Hex). When `event::poll` times out,
`tick()` advances `current` and signals a redraw.

### ImageConfig (`viewer/image/mod.rs`)

Bundles image rendering parameters (mode, width, background, margin, color mode) into one struct
passed through the image pipeline.

### PeekTheme (`theme/`)

Split by concern: `name.rs` holds `PeekThemeName` and the embedded `.tmTheme` data; `color_mode.rs`
holds `ColorMode` and the RGB→palette conversion helpers; `peek_theme.rs` holds the `PeekTheme`
struct, paint helpers, and `lerp_color`; `manager.rs` holds `ThemeManager` (shared `SyntaxSet`/
`ThemeSet` + active `PeekTheme`).

Semantic roles derive automatically from syntect `.tmTheme` files. All colored output goes through
`PeekTheme::paint()`. Color interpolation via `lerp_color()` for continuous scales (file size, age,
resolution).

`PeekTheme` carries a `ColorMode` (`TrueColor`/`Ansi256`/`Ansi16`/`Grayscale`/`Plain`) that owns
RGB → wire-format conversion. Callers always paint truecolor RGB; the mode decides 24-bit /
256-palette / 16-base / luminance-only / no-escape. Image rendering uses the same conversion via
`ColorMode::write_fg` / `write_fg_bg`. Mode is set from `--color` (or `PEEK_COLOR`) and cyclable
interactively with `c` — cycling invalidates every mode's line cache so the UI repaints in the new
encoding.

Shared escape walker for syntect's `LineRanges`: `viewer::ranges_to_escaped` — replaces syntect's
hardcoded-24-bit `as_24_bit_terminal_escaped`, routed through `ColorMode::fg_seq`.

## Image rendering pipeline

```
source image/SVG
  |
  v
add_margin() --> transparent padding
  |
  v
compute_grid() --> aspect-ratio-preserving grid (cols x rows)
                   constraint axis chosen by FitMode:
                     Contain   --> fit both axes (default)
                     FitWidth  --> width fixed, rows may exceed terminal
                     FitHeight --> height fixed, cols may exceed terminal
  |
  v
resize_exact() --> target pixel resolution (cols*CELL_W x rows*CELL_H)
  |
  v
composite_with_bg() --> resolve alpha (auto/black/white/checkerboard)
  |
  v
render_block_color() or render_density()
  |  GridWindow selects the visible sub-rectangle of the prepared grid;
  |  ImageRenderMode passes a window derived from scroll_x/scroll_y when
  |  the prepared grid exceeds the terminal viewport.
  |  Per cell (8x16 pixels):
  |    fast_2_color() --> 2 cluster colors + u128 bitmap
  |    best_glyph()   --> Hamming-distance match against glyph atlas
  |    emit ANSI fg/bg + character
  |
  v
Vec<String> lines
```

**Critical order:** resize *before* composite. Otherwise the checkerboard pattern doesn't align to
the glyph grid at the final resolution.

**Windowed render:** under `FitWidth` / `FitHeight` the prepared grid can be larger than the
terminal. The renderer never builds full lines and re-slices them — horizontal substring of styled
strings would have to parse ANSI escapes. Instead the inner cell loops iterate `GridWindow`'s
sub-range so the emitted strings are pre-windowed. `ImageRenderMode::owns_scroll() = true` and the
mode tracks `scroll_x`/`scroll_y`; pipe / `--print` always renders with `Contain`.

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

`redraw` calls `state.ensure_active_rendered()` (lazy mode render), composes the status line (name,
mode label, status segments, theme), then `state.draw()`.

### Toggle semantics: Tab, `i`, `h`, `x`

Aux modes (Info, Help, Hex) are reachable only via dedicated keys — they don't appear in the `r`
primary cycle. Aux-ness is declared by the mode itself (`Mode::is_aux()`), not hardcoded — adding a
new aux mode means overriding one trait method, no churn in `ViewerState`.
`ViewerState::toggle_aux(target_id)` is shared by Tab (Info), `h` (Help), and `x` (Hex): if active
mode *is* the target, return to `last_primary`; otherwise enter target. `i` (`SwitchInfo`) is a
one-way jump to Info.

`last_primary` updates whenever the active mode lands on a non-aux mode. Aux-to-aux transitions
(Hex → Info, Info → Hex) leave it alone, so the path back to "your actual work" survives any number
of detours — Hex → Info → Tab returns to the original primary, not to Hex.

For binary files (stack: `[Hex, Info, Help]`, no primary), `last_primary` stays `None`; exiting an
aux falls back to mode 0 (Hex itself), so `x` from standalone hex is a no-op.

## Adding a new file type

1. Add a `FileType` variant in `input/detect.rs` and wire detection.
2. Build one or more `Mode` impls in `viewer/modes/`. Add a `ModeId` variant if the mode needs to be
   toggleable by id. Override `render_to_pipe` if the default (materialize-then-write) wastes memory
   or violates byte-fidelity for that mode.
3. Wire the modes into `Registry::compose_modes` for that file type. Hex / Info / About / Help are
   appended automatically; pipe mode picks the first non-aux mode (or first, if all are aux).
4. Add info gathering in `info/gather/` if the type has interesting metadata (and themed display in
   `info/render.rs` for novel field types).

Example — adding PDF:

```rust
// in compose_modes
FileType::Pdf => {
modes.push(Box::new(PdfTextMode::new(source.clone()) ? ));    // text extract
modes.push(Box::new(PdfRenderMode::new(source.clone())? ));  // page preview
}
```

User gets Tab cycling between text extract ↔ Info, `r` cycling between text/render primaries, `x`
toggling to hex — without touching `main.rs` or the event loop.

## Adding a new theme

1. Drop `themes/<name>.tmTheme`.
2. Add a `PeekThemeName` variant in `theme/name.rs`.
3. Wire `include_str!()`, `cli_name()`, `tmtheme_source()`, `next()`, `help_text()`.
4. `PeekTheme` semantic roles derive automatically from syntect.
