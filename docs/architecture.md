# Architecture

Structure and how the pieces fit. File map: [CLAUDE.md](../CLAUDE.md). Coding
rules: [conventions.md](conventions.md).

## Design principles

1. **Single-file viewer.** One path (or stdin) at a time — closer to `less` than to `cat`.
2. **Zero runtime deps in the common path.** Themes, glyph bitmaps, and syntax definitions are
   compiled in. No config files, no downloads, no setup. PDF is the one exception: peek
   dynamically loads `libpdfium.{dylib,so,dll}` from alongside the binary (release tarball ships
   it; dev builds find it under `.pdfium/{lib,bin}`). The library has to be present for PDF
   support to work, but no system install is required.
3. **One mode stack, two outputs.** `compose_modes` builds a `Vec<Box<dyn Mode>>` per file type.
   TTY → interactive viewer (alternate screen, scrolling, key bindings). Pipe → first non-aux
   mode's `render_to_pipe(ctx)` straight to stdout. Same rendering logic, different targets — no
   parallel `Viewer` trait.
4. **Theme-aware everything.** All colored output uses `PeekTheme` semantic roles. Theme switch
   re-renders the whole view without re-reading files.
5. **Compose modes, not viewers.** New file types compose a list of view modes (text-extract,
   render-preview, hex, info, …) and hand it to one event loop, instead of forking a new interactive
   viewer per type.

## Crate structure

peek is a Cargo workspace: the `peek` **binary** at the repo root, plus leaf **library crates**
under `crates/`. The crates are layers — each depends only on the ones below it, and Cargo enforces
that no edge points back up. This turns architectural rules ("detection must not depend on the
readers") from convention into compile errors.

```
peek (bin)        session layer: CLI + the compose / gather / extract dispatch hubs + the
  ▲               interactive event loop (viewer_session). Depends on every crate below.
peek-types        per-file-type readers (one module per type). Owns the parser deps. Depends on
  ▲               foundation / detect / io / theme.
peek-foundation   reader/viewer toolkit + info base + output/extract vocab + base64/xml. Depends
  ▲               on theme / io / detect.
peek-theme  ◀──┐  theming leaf (PeekTheme + tmThemes + SGR). Parallel to peek-io.
peek-detect    │  file-type detection: FileType + format enums + magic/extension/content
  ▲            │  classification + MIME + transparent decompress-then-redetect. Depends on peek-io.
peek-io  ◀─────┘  input foundation: InputSource + streaming byte/line sources + bare codecs +
                  stdin/tty. Depends on nothing in-tree.
```

Why these cuts:

- **`peek-io`** is the "stream, don't load" foundation (principle in CLAUDE.md). Everything reads
  through `InputSource`; isolating it keeps the IO primitives reviewable and reusable, and
  guarantees they carry no knowledge of file types or rendering.
- **`peek-detect`** is the layer we most want to review and harden in isolation: a small, mostly
  dependency-light surface that maps bytes/names to a `FileType`. Because it can't reach the reader
  crates, `cargo tree -p peek-detect` is the litmus — it must stay free of the heavy reader deps
  (calamine, rusqlite, pdfium, symphonia, object, image…). The lone heavy detection dep is
  `x509-parser`, which cert detection uses to content-verify DER certs.
- **`peek-theme`** is the terminal-styling leaf, parallel to peek-io. Both are pure foundations the
  layers above build on.
- **`peek-foundation`** is the shared reader/viewer toolkit (the `Mode` engine, shared view modes,
  the UI primitives, the image-render vocab, the info base, the output/extract value types). It can
  reach theme/io/detect but **not** the bin — so the toolkit can't reach the event loop or process
  control.
- **`peek-types`** is the layer that parses untrusted file bytes (fonts, PDFs, archives, disk
  images). Cargo bars it from naming the bin's session layer, so a parser bug is contained to a
  crate with no I/O-control surface. `cargo tree -p peek-types` must not show the `peek` bin. It
  owns the heavy parser dependency set.
- The **binary** is the thin session layer: the CLI plus the three `FileType → types::<x>` dispatch
  hubs (`compose.rs`, `gather/`, `extract/`) and the interactive event loop (`viewer_session/`). It
  reaches the lower crates through thin façades (`crate::input` over peek-io/peek-detect; re-exports
  of `peek_foundation::{viewer, info, …}` and `peek_types::types`) so the hubs' `crate::*` paths are
  unchanged.

The top-level file map lives in [CLAUDE.md](../CLAUDE.md); the per-file detail (what each module
does and why) lives in each file's `//!` module doc-comment.

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

### InputSource (`crates/peek-io/src/source.rs`)

The input layer is its own crate, `peek-io` — the dependency-free foundation everything builds on
(see the crate structure section above). The binary reaches it through
the `crate::input` façade, so the paths below are also reachable as `crate::input::*`.

Decouples "where data comes from" from "how it's displayed". Four variants: `File` (path on
disk, reads on demand), `Memory { bytes: Bytes, name }` (stdin, small extracted archive
entries, encoded animation frames — `Bytes::clone` is a refcount bump, not a copy),
`FileRange { base, offset, len, name }` (zero-copy offset+limit view used by ISO extracts and
uncompressed archive entries), and `TempFile { file: Arc<NamedTempFile>, name }` (large
extracted archive entries spooled to `$TMPDIR/peek-*` — RAII unlink on last `Arc` drop).

All viewers and modes take `&InputSource` and call `read_text()` / `read_bytes()` — image,
animation, and SVG modes decode from any variant. `read_bytes()` returns `Bytes` so the
`Memory` arm is a refcount clone and accidental copies have to be spelled `.to_vec()` at the
call site.

For random-access reads without slurping, `open_byte_source() -> Box<dyn ByteSource>` returns a
seeking handle. `HexMode` uses this to read just the visible window per scroll. `File` and
`TempFile` seek per call; `Memory` slices the buffered `Bytes`; `FileRange` wraps a `File`
reader with offset translation. The `TempFile` byte source carries its own `Arc<NamedTempFile>`
clone so reads outlive any drop of the source.

For line-oriented streaming, `open_line_source() -> LineSource` (in `crates/peek-io/src/lines.rs`)
does one pass of the source to count newlines and capture sparse byte-offset anchors (every 1024
lines), then serves windowed line lookups in O(stride) — `ContentMode` uses this so multi-GB text
files never materialize. Stdin and file go through the same path: stdin's `Arc<[u8]>` backing
makes "streaming"a zero-cost slice; file seeks per chunk via `FileByteSource`.

When stdin is consumed (`-` argument or no args + piped stdin), `peek_io::stdin::read_stdin` reads
it into a `Memory` source and reopens fd 0 from the controlling terminal so the event loop can still
read keystrokes (resolved via `ttyname()` on stderr/stdout, not `/dev/tty` directly — macOS kqueue
rejects the latter with EINVAL). The CLI-level "file vs stdin" decision (`build_source`, needs
`Args`) stays in the binary at `src/input.rs`.

Stdin detection: magic bytes (images, binary) → content sniffing (leading `{`/`[` → JSON, `<` →
XML/SVG, `---` → YAML), in `peek-detect`'s `detect_bytes()` (`crates/peek-detect/src/detect.rs`).

## Key abstractions

Paths below are relative to `crates/peek-foundation/src/` unless prefixed otherwise: `src/…` is the
bin, `crates/peek-theme/src/…` is the theme crate, `crates/peek-types/src/…` is the readers crate.

### Mode trait — interactive (`viewer/modes/mod.rs`)

```rust
pub struct Window { pub lines: Vec<String>, pub total: usize }

pub trait Mode {
    fn id(&self) -> ModeId;
    fn label(&self) -> &str;
    fn is_aux(&self) -> bool { false }

    // Rendering
    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window>;
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()>
        { /* default: render_window(0, term_rows), write each line */ }
    fn render_flat_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()>
        { /* default: render_to_pipe; ListingMode overrides for --list */ }
    fn total_lines(&self) -> Option<usize> { None }

    // Scroll / resize
    fn owns_scroll(&self) -> bool { false }
    fn scroll(&mut self, _action: Action) -> bool { false }
    fn rerender_on_resize(&self) -> bool { false }
    fn on_resize(&mut self, _term_cols: usize, _term_rows: usize) {}

    // Status line + keys
    fn status_segments(&self, _theme: &PeekTheme) -> Vec<(String, Color)> { vec![] }
    fn status_hints(&self, _has_return_target: bool) -> Vec<&'static str> { vec![] }
    fn extra_actions(&self) -> &'static [HelpEntry] { &[] }  // HelpEntry = (&[Action], &str)
    fn help_entries(&self) -> Vec<HelpEntry> { self.extra_actions().to_vec() }
        // help-screen card; override to drop entries inert for this instance
        // (ListingMode filters by the source's ListingHelp descriptor)
    fn handle(&mut self, _action: Action) -> Handled { Handled::No }

    // Time-driven content (animations)
    fn next_tick(&self) -> Option<Duration> { None }
    fn tick(&mut self) -> bool { false }

    // Position tracking (cross-mode "where was I")
    fn tracks_position(&self) -> bool { false }
    fn position(&self) -> Position { Position::Unknown }
    fn set_position(&mut self, _pos: Position, _source: &InputSource) {}

    // Async warnings, merged into FileInfo.warnings after each render
    fn take_warnings(&mut self) -> Vec<String> { vec![] }

    // Extract / descend / in-frame jump (see "Session stack" below)
    fn extract_target(&self) -> Option<ExtractTarget> { None }
    fn build_descend_frame(&mut self) -> Option<Result<DescendFrame>> { None }
    fn select_jump(&self) -> Option<(ModeId, Position)> { None }
    fn jump_position(&mut self, pos: Position, source: &InputSource)
        { /* default: set_position; Hex also marks the landed byte */ }

    // Text search
    fn set_search(&mut self, _query: Option<&str>) -> SearchTarget { SearchTarget::Owned }
}

pub enum Handled { No, Yes, YesResetScroll, YesScrollTo(usize) }
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

`extract_target` / `select_jump` / `build_descend_frame` / `jump_position` are the mode side of
recursive peek — what the extract key saves and what Enter descends into. The session side
(resolution order, frame stack) is under "Session stack / recursive peek" below.

Two deliberate asymmetries in the trait surface, examined and kept (checkup M18 / L4):
`render_window`'s `scroll` is dead for `owns_scroll() = true` modes — they keep their own position
(byte offset, wrap-aware line, page index) and ignore the caller's. A `ScrolledMode` /
`OwnsScrollMode` trait split would drop the dead parameter but bifurcate the mode vocabulary and
`ViewerState`'s dispatch — too much surface for one ignored argument, with most data modes owning
scroll anyway. Likewise `status_hints(has_return_target)` is read only by `HexMode`: the parameter
stays because it is the only channel for session context to reach a foundation-crate mode — the
Cargo layering bars modes from calling back into the bin's `ViewerState`, and a mode-side setter
would mean hand-synced state at every mode switch.

A `Mode` is one renderable + interactive view of a file. The interactive viewer drives a
`Vec<Box<dyn Mode>>`: Tab cycles modes (with `i`/`h`/`x` shortcuts to Info/Help/Hex). Today's modes:

| Mode                  | Used by                                                                            | Owns scroll?                  | Reacts to resize? |
|-----------------------|------------------------------------------------------------------------------------|-------------------------------|-------------------|
| `ContentMode`         | text, source, structured, SVG XML                                                  | **yes**                       | **yes**           |
| `RenderedTextMode<R>` | whole-document read views (DOCX / ODT / RTF / HTML / PDF text / vCard / iCalendar) | no                            | **yes**           |
| `EpubReadMode`        | EPUB chapter-by-chapter read (cover render + chapter search)                       | no                            | **yes**           |
| `ListingMode`         | generic listing engine over a `ListSource`: container TOCs (archive / ISO / PDF / EPUB / DOCX / ODT / audio / comic / sqlite / spreadsheet) + filesystem directory listings | **yes**                       | **yes**           |
| `HexMode`             | binary; reachable from any view via `x`                                            | **yes** (byte-aligned)        | **yes**           |
| `ImageRenderMode`     | raster + rasterized SVG                                                            | **yes** (FitWidth/Height pan) | **yes**           |
| `AnimationMode`       | GIF / WebP (drives `next_tick`/`tick`)                                             | **yes**                       | **yes**           |
| `SvgAnimationMode`    | CSS-`@keyframes` SVG (lazy per-frame raster)                                       | **yes**                       | **yes**           |
| `PagedImageMode<R>`   | PDF / CBZ paged image render                                                       | **yes**                       | **yes**           |
| `SpecimenMode`        | font specimen rasterisation (`.ttf` / `.otf` / `.ttc`)                             | **yes**                       | **yes**           |
| `TableMode`           | objfile / classfile aligned tables                                                 | **yes**                       | **yes**           |
| `RowsTableMode`       | streaming CSV / TSV + SQLite contents                                              | **yes**                       | **yes**           |
| `InfoMode`            | every file (file metadata)                                                         | no                            | no                |
| `HelpMode`            | every file (keyboard-shortcut listing)                                             | no                            | no                |
| `AboutMode`           | every file (logo, version, palette swatches)                                       | no                            | no                |

### Pipe-mode rendering (`Mode::render_to_pipe`)

`render_to_pipe` is the print-path entry point on every mode. The default impl materializes
`render(ctx)` and writes each line to `PrintOutput`; modes that can stream directly from a
`ByteSource` (HexMode) or that need byte-faithful raw output (ContentMode without a syntax token)
override it. `RenderCtx` injects `term_cols = $COLUMNS-or-80` and `term_rows = usize::MAX` for the
pipe path, so a single `render` body can serve both interactive and pipe contexts when bounded
viewports aren't required.

`main` picks the pipe primary as the first non-aux mode in the stack, falling back to the first
mode when all are aux (binary files, where the stack is `[Hex, Info, About, Help]`).

### ViewerState (`src/viewer_session/`)

The interactive controller — one type split across four concern files: `state.rs` (the struct +
key dispatch + `apply` + mode switching), `frame.rs` (`SessionFrame` + the recursive-peek stack /
descend / extract), `prompt.rs` (the modal-prompt slot and its confirm dispatch), `render.rs`
(view cache + render-failure recovery + caller-side scroll math + `draw`).

State splits per-session vs cross-session. Each `SessionFrame` owns one peek session: source,
detected type, `FileInfo`, the mode list, active index, `last_primary` slot (most recent non-aux
mode), per-mode scroll offsets, lazy per-mode rendered-view cache, and a `Position` (last known
logical location in the source). `ViewerState` holds the frame *stack* plus what survives across
frames: theme, `ScreenBuffer`, the prompt slot, the status flash, and the `ModeBuilder` closure
(captured at construction so descend can compose modes for a new frame without knowing about
`Registry` / `Args`).

`apply()` handles session-level actions (scroll, theme cycle, mode switch, extract / descend).
The event loop tries the active mode's `scroll()` and `handle()` first, then falls through — so
mode-local actions (`r` raw/pretty, `b` background) stay scoped. The session / mode-local split
is declared per variant in `Action::is_mode_local` (an exhaustive match: a new variant fails to
compile until categorised).

### Session stack / recursive peek (`src/viewer_session/frame.rs`)

The stack is a `Vec<SessionFrame>`; the active session is always the last entry. The status-line
breadcrumb joins the frames' names (`archive.zip > inner.tar > notes.txt`); a frame's
`breadcrumb_label` overrides its source name when a synthetic frame reuses the parent source
(SQLite table view shows the table name, not the db file twice).

Enter (`Action::Descend`) resolves through three mode hooks in order:

1. **`select_jump`** — in-frame jump: switch to a sibling mode and seek it to a `Position`
   (object-file symbol → its byte offset in Hex). No stack change; the target's `jump_position`
   runs so it can mark the landed spot.
2. **`build_descend_frame`** — mode-supplied frame, bypassing the extract pipeline. For synthetic
   views over the *current* source (SQLite table → row viewer) that would otherwise have to
   materialise to a temp file.
3. **`extract_target`** — the standard path: extract the selection, `detect` the result,
   `resolve_transparent` (so descending into an extracted `.gz` lands on the inner content),
   compose modes via the `ModeBuilder`, push the new frame.

Dir → dir descent *replaces* the current frame instead of pushing, so browsing sibling
subdirectories doesn't accumulate a stack to back out of. `Back` (Esc) pops; at depth 1 it quits.
`MAX_STACK_DEPTH` (16) caps the stack so a hostile container that resolves to itself can't grow
it without bound. Descend failures (no selection, unsupported, broken entry, stack full) flash on
the status line and leave the current frame active.

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

### Registry (`src/compose.rs`)

Factory built once from CLI args. Holds the shared `ThemeManager` plus the resolved `PeekTheme` /
`plain_mode` flags consumed during composition. Provides `compose_modes(source, detected, args)`,
the single dispatcher that produces the mode stack consumed by both the interactive event loop and
the pipe path.

`--plain` is deliberately more than `--color plain` (checkup M12, merge declined). `StyleMode::Plain`
only drops ANSI escapes at the encoder; `plain_mode` additionally suppresses structured
pretty-print, skips the syntect pipeline entirely (`syntax_token = None`), and disables the
rendered views for SVG / HTML / Markdown. A user asking for sterile *colors* still expects
pretty-print and rendered views to work, so the two flags must not be conflated. Known wart:
`main.rs` mutates `args.color` to `Plain` when `--plain` is set — correct but hides the user's
actual `--color` choice; compute an `effective_color` at theme construction instead next time the
argument plumbing is touched.

### HexMode (`viewer/modes/hex.rs`)

One file: the layout primitives — `bytes_per_row` (`14 + 4*bpr` columns; rounded to a multiple
of 8), `align_down`, `max_top`, `format_row` (layout matches `hexdump -C`) — plus the Mode impl
built on them.

`HexMode` owns a `Box<dyn ByteSource>` plus `top_offset: u64` aligned to
the current `bytes_per_row`. Returns `owns_scroll() = true` so `ViewerState`'s line-scroll is
suppressed; handles ScrollUp/Down/PageUp/Down/Top/Bottom byte-wise via `scroll()`. `on_resize`
re-aligns `top_offset` to the new column count. `render_to_pipe` streams the whole file in 4 KB
chunks straight to the print sink — never holds more than one chunk in memory, so multi-GB hex
dumps are first-class.

### ContentMode (`viewer/modes/content.rs`)

Streams the raw view from a `LineSource` (anchor-indexed line iterator over `InputSource`); a
window-only render fetches just the visible lines per scroll, so multi-GB text never materializes.
With a syntax token, `LineStreamHighlighter` (in `viewer/highlight.rs`) carries syntect
`ParseState` + `HighlightState` across `feed()` calls so multi-line constructs (block comments,
here-docs) highlight correctly. Backward scrolls past the highlighter's cursor reset and replay
forward — typical top-to-bottom reading is cheap; pathological backward jumps on huge files pay a
one-time cost. Theme cycle resets state too (cached styles are theme-derived); color cycle takes
effect on the next `feed()` without a reset.

Pretty-print is whole-file with a cap (`PRETTY_MAX_BYTES` = the whole-doc budget class). Above the cap
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

### Animation (`types/image/animation_mode.rs` + `types/image/pipeline/animate.rs`)

`types/image/pipeline/animate.rs` decodes GIF/WebP frames up front (`decode_anim_frames`) and
exports `render_frame` for the mode. The composition decision — `AnimationMode` for animated
images, `ImageRenderMode` for static — lives in `types::image::compose::compose`, so `main.rs`
has one uniform interactive path across file types.

`AnimationMode` owns the frame list, `current` index, `playing` flag, `last_advance` instant, and an
`ImageConfig`. It drives the unified event loop's timeout via `next_tick()` (remaining duration to
next frame, or `None` when paused / on detour to Info / Help / Hex). When `event::poll` times out,
`tick()` advances `current` and signals a redraw.

### SVG animation (`types/svg/animation_mode.rs` + `types/image/pipeline/svg_anim/`)

resvg/usvg do not evaluate CSS animations. To play an animated SVG, the parser in
`types/image/pipeline/svg_anim/` (split into `mod.rs`, `scan.rs`, `spec.rs`, `keyframes.rs`,
`timeline.rs`, `marker.rs`, `util.rs`) extracts the animation timeline from the SVG itself:
`<style>` blocks are scanned for `@keyframes` rules, and elements with inline
`style="...animation-name:..."` references are matched to those rules. The parser builds an
`AnimatedSvg` value: a marked SVG string with `__PEEK_ANIM_<i>__` placeholders inserted at each
animated element's opening tag, plus a merged frame timeline (one entry per visible transition
with its hold delay). `render_frame(model, idx)` substitutes each placeholder with
`transform="..."` to produce a complete frame-N SVG that resvg can rasterize.

`SvgAnimationMode` mirrors `AnimationMode`'s controls (play/pause, frame nav, fit, scroll) but
rasterizes lazily per frame via `render::prepare_svg_bytes`. A bounded `VecDeque<(CacheKey,
PreparedImage)>` of size 64 holds recently composited frames, keyed by `(frame_idx, cols, rows,
margin, ascii, fit)`; full-loop replay after a steady state is free. Cache is cleared on
mode/background/fit toggles since the prepared grid no longer matches.

The composition decision lives in `types::svg::compose::compose`: SVG first tries
`svg_anim::try_parse` and pushes `SvgAnimationMode` if a model is found, falling back to
`ImageRenderMode` (static) otherwise. `--no-svg-anim` bypasses parsing.

Memory profile and first-loop latency analysis (plus optimization options that have *not* been
implemented yet) live in [svg-anim-perf.md](svg-anim-perf.md). Phase 1 is the working baseline; the
perf doc is the queue.

### ImageConfig (`types/image/pipeline/mod.rs`)

Bundles image rendering parameters (mode, width, background, margin, color mode) into one struct
passed through the image pipeline.

### PeekTheme (`theme/`)

Split by concern: `name.rs` holds `PeekThemeName` and the embedded `.tmTheme` data; `style_mode.rs`
holds `StyleMode` and the RGB→palette conversion helpers; `peek_theme.rs` holds the `PeekTheme`
struct, paint helpers, and `lerp_color`; `manager.rs` holds `ThemeManager` (shared `SyntaxSet`/
`ThemeSet` + active `PeekTheme`).

Semantic roles derive automatically from syntect `.tmTheme` files. All colored output goes through
`PeekTheme::paint()`. Color interpolation via `lerp_color()` for continuous scales (file size, age,
resolution).

`PeekTheme` carries a `StyleMode` (`TrueColor`/`Ansi256`/`Ansi16`/`Grayscale`/`Plain`) that owns
RGB → wire-format conversion. Callers always paint truecolor RGB; the mode decides 24-bit /
256-palette / 16-base / luminance-only / no-escape. Image rendering uses the same conversion via
`StyleMode::write_fg` / `write_fg_bg`. Mode is set from `--color` (or `PEEK_COLOR`) and cyclable
interactively with `c` — cycling invalidates every mode's line cache so the UI repaints in the new
encoding.

Shared escape walker for syntect's `LineRanges`: `viewer::ranges_to_escaped_trim_newline` —
replaces syntect's hardcoded-24-bit `as_24_bit_terminal_escaped`, routed through
`StyleMode::fg_seq`.

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

## Event loop (`src/viewer_session/interactive.rs`)

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
        if state.prompt_active()                     // modal overlay open?
            state.handle_prompt_key(key)             // keys go to the input
            -> consumed: redraw; continue
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

**Render-failure fallbacks.** `ensure_active_rendered` never lets a bad input abort the viewer. When
the active mode's `render_window` errors, it tries two recoveries before giving up. First,
`retry_frame_detection` re-detects the source with `detect_ignore_name` (magic bytes only, no path
bias) — for a file whose extension lied about its content this rebuilds the frame with the correct
type. If that doesn't apply (already ran, or re-detection agrees with the original), it falls back
to
`degrade_active_to_hex`: the active mode is repointed at the always-present Hex view, the decode
cause (deepest error in the chain, e.g. a PNG `CRC error`) is pushed onto `FileInfo.warnings` and
flashed on the status line, and `last_primary` is cleared if it pointed at the broken mode so aux
toggles don't bounce back into it. Only when there is nothing safer to fall back to (the failed mode
*is* Hex, or no Hex view exists — directories) does the error propagate. The pipe path
(`main.rs`) mirrors this: a primary-mode `render_to_pipe` failure falls back to Hex with the cause
on stderr. Net effect: a corrupt image, truncated archive, or malformed payload degrades to a hex
dump plus a warning rather than crashing peek.

### Modal prompt overlay (`viewer/ui/prompt.rs`)

A single `Option<(Prompt, PromptKind)>` slot on `ViewerState`. While `Some`, raw key events route
straight to the `Prompt` (readline-style text input) and the status line shows its render instead
of the usual segments — globals and mode keys are inert until the prompt closes. `PromptKind` is
the work to run on confirm, so one overlay serves two flows: `Extract` (save-to path, writes the
`Extracted` payload) and `Search` (hands the typed query to the active mode's `set_search`). Esc
cancels; an empty-query confirm clears.

### Text search (`viewer/search.rs`)

`/` opens the `Search` prompt; confirm calls `Mode::set_search(Some(query))` on the active mode.
Searchable modes scan their own lines into a `SearchState` — every match plus the `n`/`p` cursor —
and arm highlight overlays. `search.rs` holds the shared pieces: `smart_case_sensitive` (any
uppercase ⇒ case-sensitive), `find_matches` (non-overlapping byte ranges, exact substring),
`overlay_matches` (paints `search_match` / `search_current` colours onto an already-SGR-styled
line, dropping the syntax colour under a match), and `SearchState` itself. The scan is one full
pass over the active view's lines, capped at `MAX_MATCHES` (100 000).

`set_search` returns the first match's line for the caller to scroll to (caller-scrolled modes
ignore the return and scroll themselves). `n`/`p` go through `step_search`, a `handle` helper
shared by every caller-scrolled searchable mode — it steps the `SearchState` cursor and returns
`Handled::YesScrollTo(line)`. `ContentMode`, the rendered HTML view, and the EPUB / DOCX / ODT /
RTF / PDF-text read views all implement `set_search`; the default trait impl is a no-op so
non-text modes opt out for free. A mode drops its `SearchState` when the scanned line set changes
underneath it (raw/pretty toggle, chapter step, resize).

### View cycle: Tab, `i`, `h`, `x`, `a`

Tab cycles through the file's view modes — every mode in the stack except the overlay-style aux
modes (`Help`, `About`) and `Hex` (which has its own dedicated key). For SVG that's `ImageRender →
ContentMode (XML source) → Info`; for text/source `Content → Info`; for an animated image
`AnimationMode → Info`. The exception is binary files, where `Hex` *is* the data view: when the
stack contains no non-aux mode, `cycle_view` includes `Hex` so Tab still toggles `Hex ↔ Info`.

Aux modes (`Help`, `Hex`, `About`) are reachable only via dedicated keys (`h`/`?`, `x`, `a`).
Aux-ness is declared by the mode itself (`Mode::is_aux()`), not hardcoded — adding a new aux mode
means overriding one trait method, no churn in `ViewerState`. `ViewerState::toggle_aux(target_id)`
is shared by `h`, `x`, and `a`: if active mode *is* the target, return to `last_primary`; otherwise
enter target. `i` (`SwitchInfo`) is a one-way jump to Info.

`r` is mode-local to `ContentMode` (toggle pretty/raw on structured JSON/YAML/TOML/XML). Modes that
don't consume `r` ignore it — there is no global fallback.

`last_primary` updates whenever the active mode lands on a non-aux mode. Aux-to-aux transitions
(Hex → Info → Hex) leave it alone, so the path back to "your actual work" survives any number of
detours — Hex → Info → Tab returns to the original primary, not to Hex.

For binary files (stack: `[Hex, Info, About, Help]`, no primary), `last_primary` stays `None`;
exiting an aux falls back to mode 0 (Hex itself), so `x` from standalone hex is a no-op, and Tab
toggles `Hex ↔ Info` via the binary-file branch in `cycle_view`.

## Memory budgets

> The strategy narrative — threat model, the stream/cap/spill mechanisms, and the decision rule
> every new read path follows — lives in [memory-streaming.md](memory-streaming.md). This section
> is the budget-class *index*.

Every size gate in the workspace draws its number from one of three budget classes in
`peek-io::limits` (reachable as `crate::input::limits` from foundation / types / bin). The classes
are named by consumption shape; per-site constants alias a class and keep their domain name plus
local rationale. Membership is by rationale, not by number — a byte limit guarding a different
shape (per-record caps, pixel ceilings, count caps) stays local to its site.

| Class | Size | Shape | Members |
|---|---|---|---|
| `WHOLE_DOC_BYTES` | 32 MB | materialize **and transform** (5–20× expansion, blocks the UI during parse+highlight) | `RENDER_MAX_BYTES` (rendered views + `read_zip_entry` payloads), `PRETTY_MAX_BYTES` (structured pretty-print), the UTF-16 CSV transcode (`csv/parse.rs`) |
| `SIDECAR_PARSE_BYTES` | 64 MB | whole-text read, small derived output | `SIDECAR_TEXT_LIMIT` (markdown / SQL / CSS info), the UTF-16 text-stats decode (`text/info_gather.rs`), `DMG_PLIST_MAX_BYTES` |
| `BULK_WALK_BYTES` | 256 MB | one bounded pass over untrusted / unbounded data, nothing proportional retained | `MAX_DECOMPRESS_BYTES` (the batch `decompress_bytes` helper), `MAX_EXTRACT_BYTES` (per archive entry), `SEARCH_SCAN_MAX_BYTES` (raw-content + table-cell search), `STATIC_LIB_SUMMARY_CAP` (`ar` object-member summary — materialized whole but held for one pass; real `.a` files exceed the sidecar budget) |

The transparent-decompression path (`resolve_transparent` → `decompress_to_source`) does *not*
alias a budget class: it streams the compressed input and spills the decompressed output to a
tempfile past `DECOMPRESS_SPOOL_THRESHOLD` (16 MB), so RAM stays bounded by the spill threshold
regardless of inner size and an arbitrarily large bare-codec file (`bigdb.sqlite.xz`) opens the
same way the identical entry inside a `.tar.xz` does. Disk capacity is the limit on the spilled
path. The 16 MB threshold mirrors the archive-extract spool (`extract.rs::SPOOL_THRESHOLD`).

Gate helpers — call one of these rather than hand-rolling a check:

- `InputSource::read_bytes_capped(cap, what)` / `read_text_capped(cap, what)`
  (`peek-io/source.rs`) — whole-file read refused above `cap` via a cheap `byte_len` stat, for
  the parse paths with no streaming option (`object::File`, the EPS header, the notebook JSON).
  Lets compose / info degrade (Info-only, streaming source view, dropped section) instead of
  slurping a multi-GB file into RAM.
- `ensure_under_render_cap(len, what)` / `render_cap_exceeded(len, what)`
  (`viewer/modes/rendered_text.rs`) — refuse / warn before a whole-document read.
- `read_zip_entry` (`types/archive/reader.rs`) — gated zip-entry payload read (declared *and*
  actual size).
- `gather_capped_text` (`types/text/info_gather.rs`) — capped whole-text read for sidecar parsers.
- `SearchState::scan_capped` (`viewer/search.rs`) — byte-budgeted scan over a streaming source.

Deliberately local limits (different shapes, not class members): CSV `MAX_RECORD_BYTES` /
`MAX_RECORD_LINES` (per record), `MAX_MATCHES` (count), animation per-frame / cumulative budgets,
`PDFIUM_RENDER_CAP_PX` / `SVG_RASTER_CAP_PX` (pixel ceilings), `MAX_FIT_CELLS` /
`MAX_FORCED_WIDTH_CELLS` (image cell-grid ceilings, `types/image/pipeline/render.rs`), ISO
`MAX_DIR_BYTES` (metadata sanity bound).

## Adding a new file type

See [conventions.md → File types](conventions.md#file-types) for the complete owned-files /
wiring-sites checklist. Quick summary:

1. Add a `FileType` variant in `crates/peek-detect/src/detect.rs` and wire detection. The per-type
   format enum + extension/MIME/content-sniff helpers live in `crates/peek-detect/src/types/<x>.rs`
   (the `peek-detect` crate, NOT the reader). Re-export the format enum from the reader module root
   (`crates/peek-types/src/types/<x>/mod.rs`: `pub use peek_detect::types::<x>::<X>Format;`) so
   reader code keeps a local `crate::types::<x>::<X>Format` path. Detection must stay reader-free —
   that boundary is Cargo-enforced.
2. Create the `crates/peek-types/src/types/<x>/` module and build the type's `Mode` impls there.
   Generic, reusable modes — `ContentMode`, `RenderedTextMode`, `PagedImageMode`, `ListingMode` —
   already live in `peek-foundation` (`viewer/`); prefer wrapping one over a bespoke `Mode`. Add a
   `ModeId` variant if a mode must be toggleable by id. Override `render_to_pipe` if the default
   (materialize-then-write) wastes memory or violates byte-fidelity for that mode.
   **Any whole-file or whole-payload read must be gated**: pick a budget class from
   "Memory budgets" above and call the matching gate helper (`ensure_under_render_cap` for
   renders, `gather_capped_text` for sidecar parses, `read_zip_entry` for container payloads)
   before the read — never `read_bytes()` / `read_text()` bare.
3. Add `types/<x>/compose.rs` with a `compose()` that pushes the type's modes, then **one arm in the
   bin's `src/compose.rs`** (`Registry::compose_modes`) delegating to it. Hex / Info / About / Help
   are appended automatically; pipe mode picks the first non-aux mode (or first, if all are aux).
   The per-type `compose()` lives in peek-types; only the dispatch arm lives in the bin.
4. Add `types/<x>/info_gather.rs` (`gather_extras(...)` returning `Extras`, i.e.
   `Box::new(<Stats>)`) and `types/<x>/info_render.rs` for type-specific metadata, with one
   `impl_info_extras!` row binding the stats struct to the `InfoExtras` trait (`info::render`
   dispatches through the trait — no per-type render match). Then wire **one arm in the bin's
   `src/gather/mod.rs`** calling the type's `gather_extras`. Tiny types may combine gather + render
   into one `info.rs`.

   Build the section in one of **three modes**, in order of preference:

    - **Derive (regular sections — default).** Define a
      `#[derive(serde::Serialize, peek_foundation::info::InfoView)]` view struct (see
      `info/section.rs`) so print + `--info --json` fall out of one definition:
      `#[info(label/nest/skip/title/title_from)]` for the print tree, `serde` attrs for JSON,
      per-field paint via `InfoValue`. Cells are the semantic `Value` (`Size` / `Count` /
      `Timestamp` / `Text` / …, see `info/value.rs`) painted by role; `Muted` / `Accent` / `Warn`
      are the off-colour string newtypes. When a leaf's print text and JSON value **diverge**
      (prints `ELF`, serializes `"elf"`; or a join-string that serializes as an array), use
      `Value::split(text, Role, json)` / `Value::labelled(label, token)` rather than a bespoke
      newtype. Wire with `impl_info_extras!(<View>, json = "<key>")`.

    - **`InfoRow` (irregular but row-shaped — enum-variant dispatch, one print row mapping to
      several JSON keys).** The derive walks struct *fields*, so it can't express a `Vec<enum>`
      whose variants lay out differently. Build a `Vec<InfoRow>` per entry instead (see
      `info/rows.rs`): each row carries an optional print label, an optional JSON key, and a `Value`
      cell (`InfoRow::new/text/count/int/muted/…`, `print_only`, `json_only`). One list feeds both
      outputs — `push_rows` for the themed lines, `rows_to_json` for the object — so the two can't
      drift. `cert` and `font` are the worked examples. The section frame (which blocks exist,
      custom headers) stays hand-built in `render_section` / `json_section`.

    - **Fully bespoke (rare).** A layout neither covers (pre-painted composite cells, nested
      partition tables) wraps its gathered struct and hand-implements `InfoView::info_nodes` (the
      `InfoNode` tree, capturing `lines`-based renderers as `Line` nodes) plus `serde::Serialize`.

   The `InfoRow` and bespoke modes wire through the free `render_section` / `json_section` form of
   `impl_info_extras!`.
5. If the type is a container, add `types/<x>/extract.rs` (returning `peek_foundation::extract`'s
   `Extracted` / `ExtractError`) and **one arm in the bin's `src/extract/extract.rs`**.

So a new type is: a peek-detect entry, a peek-types module, and up to three one-line dispatch arms
in the bin (compose / gather / extract).
See [conventions.md → File types](conventions.md#file-types).

### Why the dispatch arms stay explicit (no `FileTypeRegistry`)

A `trait FileTypeRegistry` wiring all the per-type dispatch sites at one place was considered and
declined after the 6th file type proved the cost was acceptable (checkup M6):

- **Detection can't join.** `detect.rs` *produces* the `FileType` from magic / extension / content
  sniff — there is no `FileType` value to dispatch on yet, so a `FileType → impl` registry can
  never absorb all the wiring sites.
- **Format sub-enums break a 1:1 type→impl map.** `Archive(fmt)`, `Document(fmt)`, `Audio(fmt)`,
  `Cert(fmt)` dispatch on the inner format too; a uniform per-type trait fits them awkwardly.
- **The explicit `match` is compiler-enforced completeness.** A missing compose / extract / gather
  arm is a compile error; a registry trait with defaulted methods would silently no-op instead.
  `compose_modes` also keeps the whole dispatch table readable in one file, where a wide trait with
  no-op defaults trades that map for scattered impls and click-through.

Touching ~6 sites per new type is mechanical and compiler-guided — low cognitive load. Revisit only
if a dispatcher can someday silently fall through and ship a bug; today a missing `mime` / info
arm degrades visibly (a `?` mime, an absent Info section), never silently.

Example — PDF (`crates/peek-types/src/types/pdf/compose.rs`):

```rust
// fn compose(source, detected, args, ctx, modes) — invoked from the
// FileType::Pdf arm of Registry::compose_modes (in the bin's src/compose.rs)
let doc = pdf::package::open_doc(source)?;            // Pdfium-backed Doc, Arc-cloneable
modes.push(Box::new(PagedImageMode::new(PdfPageRenderer::new(doc.clone()), image_config))); // page render
modes.push(Box::new(RenderedTextMode::new(PdfTextRenderer::new(doc.clone())))); // text extract
let embeds = doc.list_embeds();                       // /EmbeddedFiles attachments
if !embeds.is_empty() {
    modes.push(Box::new(ListingMode::new("PDF", "Embeds", from_flat_paths(embeds), vec![])));
}
```

User gets Tab cycling through `page render → text extract → embed listing → Info`, `n`/`p` stepping
pages in the page view, `e` extracting attachments from the listing, `x` toggling to hex, `i`
jumping to Info. Pdfium is dynamically loaded from `libpdfium.*` shipped alongside the binary; the
loader path-search is in `pdf::package::locate_bindings` (exe dir → `.pdfium/{lib,bin}` dev
fallback — Windows pdfium tarball ships the dll under `bin/`, Unix tarballs ship the dylib under
`lib/` → system).

## Adding a new theme

1. Drop `crates/peek-theme/themes/<name>.tmTheme`.
2. Add a `PeekThemeName` variant in `crates/peek-theme/src/name.rs`.
3. Wire `include_str!()`, `cli_name()`, `tmtheme_source()`, `next()`, `help_text()`.
4. `PeekTheme` semantic roles derive automatically from syntect.
