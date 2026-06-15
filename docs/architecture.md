# Architecture

Structure + how the pieces fit. File map: [CLAUDE.md](../CLAUDE.md). Coding rules:
[conventions.md](conventions.md).

## Design principles

1. **Single-file viewer.** One path (or stdin) at a time — closer to `less` than `cat`.
2. **Zero runtime deps in the common path.** Themes, glyph bitmaps, syntax defs compiled in. No
   config files, downloads, setup. PDF is the one exception: peek dynamically loads
   `libpdfium.{dylib,so,dll}` from beside the binary (release tarball ships it; dev builds find it
   under `.pdfium/{lib,bin}`). The library must be present for PDF; no system install required.
3. **One mode stack, two outputs.** `compose_modes` builds a `Vec<Box<dyn Mode>>` per type. TTY →
   interactive viewer (alt screen, scroll, keys). Pipe → first non-aux mode's `render_to_pipe(ctx)`
   straight to stdout. Same logic, different targets — no parallel `Viewer` trait.
4. **Theme-aware everything.** All colored output uses `PeekTheme` roles. Theme switch re-renders
   the whole view without re-reading files.
5. **Compose modes, not viewers.** New types compose a list of view modes (text-extract,
   render-preview, hex, info…) handed to one event loop, not a new viewer per type.

## Crate structure

peek is a Cargo workspace: the `peek` **binary** at repo root + leaf **library crates** under
`crates/`. The crates are layers — each depends only on those below, and Cargo enforces no edge
points back up. Turns architectural rules ("detection must not depend on the readers") from
convention into compile errors.

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

- **`peek-io`** — the "stream, don't load" foundation. Everything reads through `InputSource`;
  isolating it keeps the IO primitives reviewable + reusable, carrying no knowledge of file types
  or rendering.
- **`peek-detect`** — the layer we most want to harden in isolation: small, dependency-light, maps
  bytes/names → `FileType`. Can't reach the reader crates, so `cargo tree -p peek-detect` is the
  litmus — must stay free of the heavy reader deps (calamine, rusqlite, pdfium, symphonia, object,
  image…). The one heavy crate it also pulls is `x509-parser` — owned by peek-types' parser set
  (cert parsing is the heavier surface); cert detection borrows it to content-verify DER certs.
- **`peek-theme`** — terminal-styling leaf, parallel to peek-io. Both pure foundations.
- **`peek-foundation`** — shared reader/viewer toolkit (the `Mode` engine, shared view modes, UI
  primitives, image-render vocab, info base, output/extract value types). Reaches theme/io/detect
  but **not** the bin — the toolkit can't reach the event loop or process control.
- **`peek-types`** — parses untrusted file bytes (fonts, PDFs, archives, disk images). Cargo bars
  it from the bin's session layer, so a parser bug is contained to a crate with no I/O-control
  surface. `cargo tree -p peek-types` must not show the `peek` bin. Owns the heavy parser dep set.
- **binary** — thin session layer: CLI + the three `FileType → types::<x>` dispatch hubs
  (`compose.rs`, `gather/`, `extract/`) + the event loop (`viewer_session/`). Names the lower
  crates directly — `peek_io`, `peek_detect`, `peek_theme`, `peek_foundation::{viewer, info, …}`,
  `peek_types::types` — no re-export shims.

Top-level file map: [CLAUDE.md](../CLAUDE.md). Per-file detail: each file's `//!` header.

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

Its own crate, `peek-io` — the dependency-free foundation (see crate structure). Named directly as
`peek_io::*`.

Decouples "where data comes from" from "how it's displayed". Four variants:

- `File` — path on disk, reads on demand.
- `Memory { bytes: Bytes, name }` — stdin, small extracted archive entries, encoded animation
  frames. `Bytes::clone` is a refcount bump, not a copy.
- `FileRange { base, offset, len, name }` — zero-copy offset+limit view (ISO extracts, uncompressed
  archive entries).
- `TempFile { file: Arc<NamedTempFile>, name }` — large extracted archive entries spooled to
  `$TMPDIR/peek-*`; RAII unlink on last `Arc` drop.

All modes take `&InputSource`, call `read_text(Budget)` / `read_bytes(Budget)` — image/anim/SVG
decode from any variant. The required `Budget` (`peek_io::limits`) names the read's consumption
shape so an unguarded whole-file slurp won't compile; see [Memory budgets](#memory-budgets).
`read_bytes` returns `Bytes` so the `Memory` arm is a refcount clone; accidental copies must be
spelled `.to_vec()` at the call site.

Random access without slurping: `open_byte_source() -> Box<dyn ByteSource>` — a seeking handle.
`HexMode` reads just the visible window per scroll. `File`/`TempFile` seek per call; `Memory` slices
the buffered `Bytes`; `FileRange` wraps a `File` reader with offset translation. The `TempFile` byte
source carries its own `Arc<NamedTempFile>` clone so reads outlive any drop of the source.

Line streaming: `open_line_source() -> LineSource` (`crates/peek-io/src/lines.rs`) does one pass to
count newlines + capture sparse byte anchors (every 1024 lines), then serves windowed line lookups
in O(stride). `ContentMode` uses it so multi-GB text never materializes. Stdin + file share the
path: stdin's `Arc<[u8]>` backing makes "streaming" a zero-cost slice; file seeks per chunk via
`FileByteSource`.

Stdin consumed (`-` arg, or no args + piped stdin): `peek_io::stdin::read_stdin` reads it into a
`Memory` source + reopens fd 0 from the controlling terminal so the event loop can still read
keystrokes (resolved via `ttyname()` on stderr/stdout, not `/dev/tty` directly — macOS kqueue
rejects the latter with EINVAL). The CLI "file vs stdin" decision (`build_source`, needs `Args`)
stays in the bin at `src/input.rs`.

Detection: file, resident-memory (stdin / archive entry), and spooled-stream sources all run
through one `classify` core over a `Probe` in peek-detect (`crates/peek-detect/src/detect.rs`).
Precedence: name routing → magic bytes → content sniff (leading `{`/`[` → JSON via serde's `Eof`
prefix signal; root `<svg>`/`<html>` → SVG/HTML; `---` → YAML) → plain text. Strong magic that
*contradicts* a recognised extension overrides it (a `.csv` holding a zip); coarse magic the
extension merely refines (zip → `.docx`, `%PDF` → `.ai`) keeps the name.

## Key abstractions

Paths relative to `crates/peek-foundation/src/` unless prefixed: `src/…` = bin,
`crates/peek-theme/src/…` = theme crate, `crates/peek-types/src/…` = readers crate.

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
    fn set_search(&mut self, _query: Option<&SearchQuery>) -> SearchTarget { SearchTarget::Owned }
}

pub enum Handled { No, Yes, YesResetScroll, YesScrollTo(usize) }
```

`render_window` is the single rendering contract. The mode gets a viewport request `(scroll, rows)`,
returns the visible slice + full-source `total` line count — `ViewerState` writes the slice verbatim
and uses `total` for scroll math. Streaming modes (`ContentMode`) honor the window, fetch only
what's visible; fixed-content modes (Info/Help/About) materialize their full output + pre-slice via
`slice_window`. `total_lines()` lets a mode answer line count cheaply — `ContentMode` returns its
`LineSource.total_lines()` in O(1) so Bottom-jumps don't force a render.

`is_aux()` marks Help / About / Hex as auxiliary so they're reached only via dedicated keys
(h, a, x), skipped by the Tab primary cycle, and toggle back to `last_primary`. Info is *not* aux
(no `is_aux` override) — it sits in the Tab cycle, with `i` as a one-way shortcut to it.
`status_hints` lets a mode contribute right-side hints contextually (Hex shows `x:exit hex` only
when it has somewhere to return). `Handled::YesResetScroll` zeroes the active mode's scroll offset
(when an action invalidates the prior position — e.g. ContentMode flipping pretty ↔ raw).

`extract_target` / `select_jump` / `build_descend_frame` / `jump_position` are the mode side of
recursive peek — what the extract key saves, what Enter descends into. Session side (resolution
order, frame stack) under "Session stack / recursive peek" below.

Two deliberate trait-surface asymmetries, examined + kept (checkup M18 / L4): `render_window`'s
`scroll` is dead for `owns_scroll() = true` modes — they keep their own position (byte offset,
wrap-aware line, page index) and ignore the caller's. A `ScrolledMode` / `OwnsScrollMode` split
would drop the dead param but bifurcate the mode vocabulary + `ViewerState` dispatch — too much
surface for one ignored arg, most data modes own scroll anyway. Likewise
`status_hints(has_return_target)` is read only by `HexMode`: the param stays because it's the only
channel for session context to reach a foundation-crate mode — Cargo layering bars modes from
calling back into the bin's `ViewerState`, and a mode-side setter would mean hand-synced state at
every switch.

A `Mode` is one renderable + interactive view of a file. The interactive viewer drives a
`Vec<Box<dyn Mode>>`: Tab cycles modes (`i`/`h`/`x` shortcuts to Info/Help/Hex). Today's modes:

| Mode                  | Used by                                                                            | Owns scroll?                  | Reacts to resize? |
|-----------------------|------------------------------------------------------------------------------------|-------------------------------|-------------------|
| `ContentMode`         | text, source, structured, SVG XML                                                  | **yes**                       | **yes**           |
| `RenderedTextMode<R>` | whole-document read views (DOCX / ODT / RTF / HTML / PDF text / vCard / iCalendar) | no                            | **yes**           |
| `PagedTextReadMode<R>`| paged-text read views over a `PagedText` reader: presentation slides + EPUB chapters (per-page search; EPUB adds cover render) | no | **yes**           |
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

The print-path entry on every mode. Default impl materializes `render(ctx)` + writes each line to
`PrintOutput`; modes that stream from a `ByteSource` (HexMode) or need byte-faithful raw output
(ContentMode without a syntax token) override it. `RenderCtx` injects `term_cols = $COLUMNS-or-80`
and `term_rows = usize::MAX` for the pipe path, so one `render` body serves both contexts when
bounded viewports aren't required.

`main` picks the pipe primary as the first non-aux mode, falling back to the first when all are aux
(binary files, stack `[Hex, Info, About, Help]`).

### ViewerState (`src/viewer_session/`)

The interactive controller — one type across four concern files: `state.rs` (struct + key dispatch +
`apply` + mode switching), `frame.rs` (`SessionFrame` + recursive-peek stack / descend / extract),
`prompt.rs` (modal-prompt slot + confirm dispatch), `render.rs` (view cache + render-failure
recovery + caller-side scroll math + `draw`).

State splits per-session vs cross-session. Each `SessionFrame` owns one peek session: source,
detected type, `FileInfo`, mode list, active index, `last_primary` slot (most recent non-aux mode),
per-mode scroll offsets, lazy per-mode rendered-view cache, a `Position` (last known logical
location). `ViewerState` holds the frame *stack* + what survives across frames: theme,
`ScreenBuffer`, prompt slot, status flash, and the `ModeBuilder` closure (captured at construction
so descend composes modes for a new frame without knowing `Registry` / `Args`).

`apply()` handles session-level actions (scroll, theme cycle, mode switch, extract / descend). The
event loop tries the active mode's `scroll()` + `handle()` first, then falls through — mode-local
actions (`r` raw/pretty, `b` background) stay scoped. The split is declared per variant in
`Action::is_mode_local` (exhaustive match: a new variant fails to compile until categorised).

### Session stack / recursive peek (`src/viewer_session/frame.rs`)

The stack is a `Vec<SessionFrame>`; active = last entry. The status breadcrumb joins frame names
(`archive.zip > inner.tar > notes.txt`); a frame's `breadcrumb_label` overrides its source name when
a synthetic frame reuses the parent source (SQLite table view shows the table name, not the db file
twice).

Enter (`Action::Descend`) resolves through three mode hooks in order:

1. **`select_jump`** — in-frame jump: switch to a sibling mode, seek it to a `Position` (object-file
   symbol → its byte offset in Hex). No stack change; the target's `jump_position` runs so it can
   mark the landed spot.
2. **`build_descend_frame`** — mode-supplied frame, bypassing the extract pipeline. For synthetic
   views over the *current* source (SQLite table → row viewer) that would otherwise materialise to a
   temp file.
3. **`extract_target`** — standard path: extract the selection, `detect` the result,
   `resolve_transparent` (so descending into an extracted `.gz` lands on the inner content), compose
   modes via the `ModeBuilder`, push the new frame.

Dir → dir descent *replaces* the current frame instead of pushing, so browsing sibling subdirs
doesn't accumulate a stack to back out of. `Back` (Esc) pops; at depth 1 it quits.
`MAX_STACK_DEPTH` (16) caps the stack so a hostile container that resolves to itself can't grow it
unbounded. Descend failures (no selection, unsupported, broken entry, stack full) flash on the
status line, leave the current frame active.

### Position tracking

`Position` (`Unknown` / `Byte(u64)` / `Line(usize)`) is captured from the outgoing mode + pushed to
the incoming mode on every active-mode change. Modes overriding `tracks_position()` participate; the
rest pass through. So detours through Info / Help / Image / Animation preserve where you were.
Conversion on `InputSource` (`byte_to_line` / `line_to_byte`, chunked 64 KB streaming scan).

Pretty-printed structured content has more lines than the raw source, so the displayed line index
doesn't map cleanly to bytes. `ContentMode` opts out when pretty mode is active (`tracks_position()`
returns `!use_pretty`). Switching pretty Content → Hex preserves whichever byte Hex was last on,
instead of synthesizing a wrong one. Modes needing exact mapping will eventually carry their own
line→source-byte table.

### Registry (`src/compose.rs`)

Factory built once from CLI args. Holds the shared `ThemeManager` + the resolved `PeekTheme` /
`plain_mode` flags consumed during composition. Provides `compose_modes(source, detected, args)`,
the single dispatcher producing the mode stack for both the event loop + pipe path.

`--plain` is deliberately more than `--color plain` (checkup M12, merge declined). `StyleMode::Plain`
only drops ANSI at the encoder; `plain_mode` additionally suppresses structured pretty-print, skips
the syntect pipeline entirely (`syntax_token = None`), and disables rendered views (SVG / HTML /
Markdown). A user wanting sterile *colors* still expects pretty-print + rendered views to work, so
the two flags must not be conflated. Known wart: `main.rs` mutates `args.color` to `Plain` when
`--plain` is set — correct but hides the user's actual `--color` choice; compute an
`effective_color` at theme construction next time the arg plumbing is touched.

### HexMode (`viewer/modes/hex.rs`)

One file: layout primitives — `bytes_per_row` (`14 + 4*bpr` columns, rounded to a multiple of 8),
`align_down`, `max_top`, `format_row` (layout matches `hexdump -C`) — plus the Mode impl on them.

Owns a `Box<dyn ByteSource>` + `top_offset: u64` aligned to the current `bytes_per_row`.
`owns_scroll() = true` so `ViewerState`'s line-scroll is suppressed; handles
ScrollUp/Down/PageUp/Down/Top/Bottom byte-wise via `scroll()`. `on_resize` re-aligns `top_offset` to
the new column count. `render_to_pipe` streams the whole file in 4 KB chunks straight to the print
sink — never holds more than one chunk, so multi-GB hex dumps are first-class.

### ContentMode (`viewer/modes/content.rs`)

Streams the raw view from a `LineSource` (anchor-indexed iterator over `InputSource`); a window-only
render fetches just the visible lines per scroll, so multi-GB text never materializes. With a syntax
token, `LineStreamHighlighter` (`viewer/highlight.rs`) carries syntect `ParseState` +
`HighlightState` across `feed()` calls so multi-line constructs (block comments, here-docs)
highlight correctly. Backward scrolls past the highlighter's cursor reset + replay forward —
top-to-bottom reading is cheap; pathological backward jumps on huge files pay a one-time cost. Theme
cycle resets state too (cached styles are theme-derived); color cycle takes effect on the next
`feed()` without a reset.

Pretty-print is whole-file with a cap (`PRETTY_MAX_BYTES` = the whole-doc budget class). Above the
cap ContentMode pushes a warning, clears `use_pretty`, and the streamed raw view takes over. Below,
pretty-print runs lazily on first access; the parsed text is cached, and the highlighted-pretty form
(with a syntax token) is cached keyed by `(theme, color)` so a cycle invalidates + recomputes. On
parse failure ContentMode caches the `Err`, falls back to raw, queues a one-shot warning via
`take_warnings()`. `ViewerState` polls `take_warnings()` after each render + merges new entries into
`FileInfo.warnings`, invalidating InfoMode's cached lines so the next `i` shows the warning beside
extension-mismatch notices.

Pipe path: highlighted output is `\n`-terminated per line (escapes are line-scoped); un-highlighted
preserves the source's trailing-newline status (`LineSource.ends_with_newline()`) for byte-for-byte
fidelity with `cat`.

### Animation (`types/image/animation_mode.rs` + `types/image/pipeline/animate.rs`)

`animate.rs` decodes GIF/WebP frames up front (`decode_anim_frames`) + exports `render_frame`. The
composition decision — `AnimationMode` for animated, `ImageRenderMode` for static — lives in
`types::image::compose::compose`, so `main.rs` has one uniform interactive path.

`AnimationMode` owns the frame list, `current` index, `playing` flag, `last_advance` instant, an
`ImageConfig`. Drives the event loop's timeout via `next_tick()` (remaining duration to next frame,
or `None` when paused / on detour to Info/Help/Hex). On `event::poll` timeout, `tick()` advances
`current` + signals a redraw.

### SVG animation (`types/svg/animation_mode.rs` + `types/image/pipeline/svg_anim/`)

resvg/usvg don't evaluate CSS animations. To play an animated SVG, the parser in
`types/image/pipeline/svg_anim/` (split: `mod.rs`, `scan.rs`, `spec.rs`, `keyframes.rs`,
`timeline.rs`, `marker.rs`, `util.rs`) extracts the timeline from the SVG itself: `<style>` blocks
scanned for `@keyframes`, elements with inline `style="...animation-name:..."` matched to those
rules. Builds an `AnimatedSvg`: a marked SVG string with `__PEEK_ANIM_<i>__` placeholders at each
animated element's opening tag + a merged frame timeline (one entry per visible transition with its
hold delay). `render_frame(model, idx)` substitutes each placeholder with `transform="..."` to
produce a complete frame-N SVG resvg can rasterize.

`SvgAnimationMode` mirrors `AnimationMode`'s controls (play/pause, frame nav, fit, scroll) but
rasterizes lazily per frame via `render::prepare_svg_bytes`. A bounded `VecDeque<(CacheKey,
PreparedImage)>` of size 64 holds recent composited frames, keyed by `(frame_idx, cols, rows,
margin, ascii, fit)`; full-loop replay after a steady state is free. Cache cleared on
mode/background/fit toggles (prepared grid no longer matches).

Composition in `types::svg::compose::compose`: SVG first tries `svg_anim::try_parse`, pushes
`SvgAnimationMode` if a model is found, else falls back to `ImageRenderMode` (static).
`--no-svg-anim` bypasses parsing.

Memory profile + first-loop latency analysis (+ not-yet-implemented options) in
[svg-anim-perf.md](svg-anim-perf.md). Phase 1 is the working baseline; the perf doc is the queue.

### ImageConfig (`types/image/pipeline/mod.rs`)

Bundles image rendering params (mode, width, background, margin, color mode) into one struct passed
through the pipeline.

### PeekTheme (`theme/`)

Split by concern: `name.rs` (`PeekThemeName` + embedded `.tmTheme` data), `style_mode.rs`
(`StyleMode` + RGB→palette helpers), `peek_theme.rs` (the `PeekTheme` struct, paint helpers,
`lerp_color`), `manager.rs` (`ThemeManager` — shared `SyntaxSet`/`ThemeSet` + active `PeekTheme`).

Semantic roles derive automatically from syntect `.tmTheme` files. All colored output via
`PeekTheme::paint()`. Interpolation via `lerp_color()` for continuous scales (file size, age,
resolution).

`PeekTheme` carries a `StyleMode` (`TrueColor`/`Ansi256`/`Ansi16`/`Grayscale`/`Plain`) owning RGB →
wire conversion. Callers always paint truecolor RGB; the mode picks 24-bit / 256 / 16 / luminance /
no-escape. Image rendering uses the same via `StyleMode::write_fg` / `write_fg_bg`. Mode set from
`--color` (or `PEEK_COLOR`), cyclable with `c` — cycling invalidates every mode's line cache so the
UI repaints in the new encoding.

Shared escape walker for syntect's `LineRanges`: `viewer::ranges_to_escaped_trim_newline` — replaces
syntect's hardcoded-24-bit `as_24_bit_terminal_escaped`, routed through `StyleMode::fg_seq`.

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

**Critical order:** resize *before* composite. Else the checkerboard doesn't align to the glyph
grid at the final resolution.

**Windowed render:** under `FitWidth` / `FitHeight` the prepared grid can exceed the terminal. The
renderer never builds full lines + re-slices them — horizontal substring of styled strings would
have to parse ANSI escapes. Instead the inner cell loops iterate `GridWindow`'s sub-range so emitted
strings are pre-windowed. `ImageRenderMode::owns_scroll() = true`, tracks `scroll_x`/`scroll_y`; pipe
/ `--print` always renders `Contain`.

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

**Render-failure fallbacks.** `ensure_active_rendered` never lets bad input abort the viewer. When
the active mode's `render_window` errors, two recoveries before giving up. First,
`retry_frame_detection` re-detects with `detect_ignore_name` (magic only, no path bias) — for a file
whose extension lied, this rebuilds the frame with the correct type. If that doesn't apply (already
ran, or re-detection agrees), fall back to `degrade_active_to_hex`: the active mode repoints at the
always-present Hex view, the decode cause (deepest error in the chain, e.g. a PNG `CRC error`) is
pushed onto `FileInfo.warnings` + flashed, `last_primary` cleared if it pointed at the broken mode.
Only when nothing safer remains (the failed mode *is* Hex, or no Hex exists — directories) does the
error propagate. The pipe path (`main.rs`) mirrors this: a primary-mode `render_to_pipe` failure
falls back to Hex with the cause on stderr. Net: a corrupt image, truncated archive, or malformed
payload degrades to a hex dump + warning instead of crashing.

### Modal prompt overlay (`viewer/ui/prompt.rs`)

A single `Option<(Prompt, PromptKind)>` slot on `ViewerState`. While `Some`, raw key events route to
the `Prompt` (readline-style input) + the status line shows its render — globals + mode keys inert
until it closes. `PromptKind` is the work to run on confirm, so one overlay serves two flows:
`Extract` (save-to path, writes the `Extracted` payload) + `Search` (hands the query to the active
mode's `set_search`). Esc cancels; an empty-query confirm clears.

### Text search (`viewer/search.rs`)

`/` opens the `Search` prompt; `Ctrl-R` toggles literal/regex (remembered across searches), confirm
compiles a `SearchQuery` and calls `Mode::set_search(Some(&query))` on the active mode (a bad regex
flashes the parse reason instead). `SearchQuery` is the one compiled matching primitive — `Literal`
(exact substring, default) or `Regex` (linear-time `regex` engine) — that every scan site runs
against, so regex reaches all file types without per-type wiring. Searchable modes scan their lines
into a `SearchState` — every match + the `n`/`p` cursor — and arm highlight overlays. `search.rs`
holds the shared pieces: `smart_case_sensitive` (any uppercase ⇒ case-sensitive), `find_matches`
(non-overlapping byte ranges, exact substring — the `Literal` arm's backend), `SearchQuery::find`
(dispatches literal/regex per line), `overlay_matches` (paints `search_match` / `search_current`
onto an already-SGR-styled line, dropping the syntax colour under a match), `SearchState` itself.
The scan is one full pass over the active view's lines, capped at `MAX_MATCHES` (100 000).

`set_search` returns the first match's line for the caller to scroll to (caller-scrolled modes
ignore it + scroll themselves). `n`/`p` go through `step_search`, a `handle` helper shared by every
caller-scrolled searchable mode — steps the `SearchState` cursor, returns `Handled::YesScrollTo`.
`ContentMode`, the rendered HTML view, and the EPUB / DOCX / ODT / RTF / PDF-text read views
implement `set_search`; the default trait impl is a no-op so non-text modes opt out free. A mode
drops its `SearchState` when the scanned line set changes (raw/pretty toggle, chapter step, resize).

### View cycle: Tab, `i`, `h`, `x`, `a`

Tab cycles the file's view modes — every mode except the overlay aux modes (`Help`, `About`) and
`Hex` (own key). SVG: `ImageRender → ContentMode (XML source) → Info`; text/source: `Content →
Info`; animated image: `AnimationMode → Info`. Exception: binary, where `Hex` *is* the data view —
with no non-aux mode in the stack, `cycle_view` includes `Hex` so Tab still toggles `Hex ↔ Info`.

Aux modes (`Help`, `Hex`, `About`) reachable only via dedicated keys (`h`/`?`, `x`, `a`). Aux-ness
declared by the mode (`Mode::is_aux()`), not hardcoded — a new aux mode means overriding one method,
no `ViewerState` churn. `ViewerState::toggle_aux(target_id)` is shared by `h`, `x`, `a`: if active
mode *is* target, return to `last_primary`; else enter target. `i` (`SwitchInfo`) is a one-way jump
to Info.

`r` is mode-local to `ContentMode` (toggle pretty/raw on structured JSON/YAML/TOML/XML). Modes that
don't consume `r` ignore it — no global fallback.

`last_primary` updates whenever the active mode lands on a non-aux mode. Aux-to-aux (Hex → Info →
Hex) leaves it alone, so the path back to "your actual work" survives any detours — Hex → Info → Tab
returns to the original primary, not Hex.

For binary (stack `[Hex, Info, About, Help]`, no primary), `last_primary` stays `None`; exiting an
aux falls back to mode 0 (Hex), so `x` from standalone hex is a no-op, and Tab toggles `Hex ↔ Info`
via the binary branch in `cycle_view`.

## Memory budgets

> Strategy narrative — threat model, the stream/cap/spill mechanisms, the decision rule every new
> read path follows — in [memory-streaming.md](memory-streaming.md). This section is the
> budget-class *index*.

Every size gate draws its number from one of three budget classes in `peek-io::limits` (named
directly as `peek_io::limits` from foundation / types / bin). Classes named by consumption shape;
per-site constants alias a class + keep their domain name + local rationale. Membership is by
rationale, not number — a byte limit guarding a different shape (per-record caps, pixel ceilings,
count caps) stays local to its site.

| Class | Size | Shape | Members |
|---|---|---|---|
| `WHOLE_DOC_BYTES` | 32 MB | materialize **and transform** (5–20× expansion, blocks the UI during parse+highlight) | `RENDER_MAX_BYTES` (rendered views + `read_zip_entry` payloads), `PRETTY_MAX_BYTES` (structured pretty-print), the UTF-16 CSV transcode (`csv/parse.rs`) |
| `SIDECAR_PARSE_BYTES` | 64 MB | whole-text read, small derived output | `SIDECAR_TEXT_LIMIT` (markdown / SQL / CSS info), the UTF-16 text-stats decode (`text/info_gather.rs`), `DMG_PLIST_MAX_BYTES` |
| `BULK_WALK_BYTES` | 256 MB | one bounded pass over untrusted / unbounded data, nothing proportional retained | `MAX_EXTRACT_BYTES` (per archive entry), `SEARCH_SCAN_MAX_BYTES` (raw-content + table-cell search), `STATIC_LIB_SUMMARY_CAP` (`ar` object-member summary — materialized whole but held for one pass; real `.a` files exceed the sidecar budget) |

The transparent-decompression path (`resolve_transparent` → `decompress_to_source`) does *not* alias
a class: it streams the compressed input + spills decompressed output to a tempfile past
`DECOMPRESS_SPOOL_THRESHOLD` (16 MB), so RAM stays bounded by the spill threshold regardless of inner
size — an arbitrarily large bare-codec file (`bigdb.sqlite.xz`) opens the same way the identical
entry inside a `.tar.xz` does. Disk capacity bounds the spilled path. The 16 MB threshold mirrors the
archive-extract spool (`extract.rs::SPOOL_THRESHOLD`).

Gate helpers — call one rather than hand-rolling a check:

- `InputSource::read_bytes(Budget)` / `read_text(Budget)` (`peek-io/source.rs`) — the required
  `Budget` (`peek_io::limits`) is the gate: `Budget::WholeDoc` / `Sidecar` / `BulkWalk` refuse above
  the matching class via a cheap `byte_len` stat before any allocation, for parse paths with no
  streaming option (`object::File`, the EPS header, the notebook JSON). Lets compose / info degrade
  (Info-only, streaming source view, dropped section) instead of slurping a multi-GB file.
  `Budget::Unbounded("why-safe")` is the only escape — for sources bounded by construction; the
  `&str` reason is greppable.
- `ensure_under_render_cap(len, what)` / `render_cap_exceeded(len, what)`
  (`viewer/modes/rendered_text.rs`) — refuse / warn before a whole-document read.
- `read_zip_entry` (`types/archive/reader.rs`) — gated zip-entry payload read (declared + actual
  size).
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

1. Add a `FileType` variant in `crates/peek-detect/src/detect.rs` + wire detection. The format enum
   + extension/MIME/content-sniff helpers live in `crates/peek-detect/src/types/<x>.rs` (peek-detect,
   NOT the reader). Re-export the format enum from the reader root
   (`crates/peek-types/src/types/<x>/mod.rs`: `pub use peek_detect::types::<x>::<X>Format;`) so
   reader code keeps a local `crate::types::<x>::<X>Format` path. Detection stays reader-free —
   Cargo-enforced.
2. Create `crates/peek-types/src/types/<x>/` + build the type's `Mode` impls there. Generic reusable
   modes — `ContentMode`, `RenderedTextMode`, `PagedImageMode`, `ListingMode` — live in
   peek-foundation (`viewer/`); prefer wrapping one over a bespoke `Mode`. Add a `ModeId` variant if
   a mode must be toggleable by id. Override `render_to_pipe` if the default (materialize-then-write)
   wastes memory or breaks byte-fidelity. **Any whole-file / whole-payload read must be gated**:
   `read_bytes`/`read_text` take a required `Budget` (pick the class matching the read shape);
   render / container paths have dedicated helpers (`ensure_under_render_cap` for renders,
   `gather_capped_text` for sidecar parses, `read_zip_entry` for container payloads).
   `Budget::Unbounded("why")` only for sources bounded by construction.
3. Add `types/<x>/compose.rs` with a `compose()` pushing the type's modes, then **one arm in
   `src/compose.rs`** (`Registry::compose_modes`) delegating to it. Hex / Info / About / Help
   appended automatically; pipe mode picks the first non-aux mode (or first, if all aux). The
   per-type `compose()` lives in peek-types; only the dispatch arm in the bin.
4. Add `types/<x>/info_gather.rs` (`gather_extras(...)` returning `Extras`, i.e. `Box::new(<Stats>)`)
   + `types/<x>/info_render.rs`, with one `impl_info_extras!` row binding the stats struct to
   `InfoExtras` (`info::render` dispatches through the trait — no per-type render match). Wire **one
   arm in `src/gather/mod.rs`** calling `gather_extras`. Tiny types may combine gather + render in
   one `info.rs`.

   Build the section in one of **three modes**, in order of preference:

    - **Derive (regular — default).** Define a `#[derive(serde::Serialize,
      peek_foundation::info::InfoView)]` view struct (`info/section.rs`) so print + `--info --json`
      fall out of one definition: `#[info(label/nest/skip/title/title_from)]` for the print tree,
      `serde` attrs for JSON, per-field paint via `InfoValue`. Cells are the semantic `Value` (`Size`
      / `Count` / `Timestamp` / `Text` / …, `info/value.rs`) painted by role; `Muted` / `Accent` /
      `Warn` are the off-colour string newtypes. When a leaf's print text + JSON value **diverge**
      (prints `ELF`, serializes `"elf"`; or a join-string serializing as an array), use
      `Value::split(text, Role, json)` / `Value::labelled(label, token)`, not a bespoke newtype. Wire
      with `impl_info_extras!(<View>, json = "<key>")`.
    - **`InfoRow` (irregular but row-shaped — enum-variant dispatch, one print row → several JSON
      keys).** The derive walks struct *fields*, so it can't express a `Vec<enum>` whose variants lay
      out differently. Build a `Vec<InfoRow>` per entry (`info/rows.rs`): each row carries an optional
      print label, optional JSON key, a `Value` cell (`InfoRow::new/text/count/int/muted/…`,
      `print_only`, `json_only`). One list feeds both — `push_rows` for the themed lines,
      `rows_to_json` for the object — so they can't drift. `cert` + `font` are the worked examples.
      The section frame (which blocks exist, custom headers) stays hand-built in `render_section` /
      `json_section`.
    - **Fully bespoke (rare).** A layout neither covers (pre-painted composite cells, nested
      partition tables) wraps its gathered struct + hand-implements `InfoView::info_nodes` (the
      `InfoNode` tree, capturing `lines`-based renderers as `Line` nodes) + `serde::Serialize`.

   The `InfoRow` + bespoke modes wire through the free `render_section` / `json_section` form of
   `impl_info_extras!`.
5. Container type: add `types/<x>/extract.rs` (returning `peek_foundation::extract`'s `Extracted` /
   `ExtractError`) + **one arm in `src/extract/extract.rs`**.

So a new type is: a peek-detect entry, a peek-types module, up to three one-line dispatch arms in the
bin (compose / gather / extract). See [conventions.md → File types](conventions.md#file-types).

### Why the dispatch arms stay explicit (no `FileTypeRegistry`)

A `trait FileTypeRegistry` wiring all per-type dispatch at one place was considered + declined after
the 6th file type proved the cost acceptable (checkup M6):

- **Detection can't join.** `detect.rs` *produces* the `FileType` from magic / extension / content
  sniff — no `FileType` value to dispatch on yet, so a `FileType → impl` registry can never absorb
  all the wiring sites.
- **Format sub-enums break a 1:1 type→impl map.** `Archive(fmt)`, `Document(fmt)`, `Audio(fmt)`,
  `Cert(fmt)` dispatch on the inner format too; a uniform per-type trait fits awkwardly.
- **The explicit `match` is compiler-enforced completeness.** A missing compose / extract / gather
  arm is a compile error; a registry trait with defaulted methods would silently no-op.
  `compose_modes` also keeps the whole dispatch table readable in one file.

Touching ~6 sites per new type is mechanical + compiler-guided — low cognitive load. Revisit only if
a dispatcher could someday silently fall through + ship a bug; today a missing `mime` / info arm
degrades visibly (a `?` mime, an absent Info section), never silently.

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

User gets Tab cycling `page render → text extract → embed listing → Info`, `n`/`p` stepping pages,
`e` extracting attachments from the listing, `x` to hex, `i` to Info. Pdfium dynamically loaded from
`libpdfium.*` beside the binary; the loader path-search is `pdf::package::locate_bindings` (exe dir →
`.pdfium/{lib,bin}` dev fallback — Windows pdfium tarball ships the dll under `bin/`, Unix tarballs
ship the dylib under `lib/` → system).

## Adding a new theme

1. Drop `crates/peek-theme/themes/<name>.tmTheme`.
2. Add a `PeekThemeName` variant in `crates/peek-theme/src/name.rs`.
3. Wire `include_str!()`, `cli_name()`, `tmtheme_source()`, `next()`, `help_text()`.
4. `PeekTheme` semantic roles derive automatically from syntect.
