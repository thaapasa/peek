# Architecture

This document describes the structure of peek and how the pieces fit together. For the file map, see
[CLAUDE.md](../CLAUDE.md). For coding rules, see [conventions.md](conventions.md).

## Design principles

1. **Single-file viewer.** peek shows one path, or stdin, at a time. It is closer to `less` than to
   `cat`.
2. **Zero runtime dependencies in the common path.** Themes, glyph bitmaps, and syntax definitions
   are compiled in. There are no config files, downloads, or setup steps. PDF is the one exception.
   peek loads `libpdfium.{dylib,so,dll}` dynamically from beside the binary. The release tarball
   ships the library, and dev builds find it under `.pdfium/{lib,bin}`. PDF support needs the
   library to be present, but you don't need a system install.
3. **One mode stack, two outputs.** `compose_modes` builds a `Vec<Box<dyn Mode>>` for each file
   type. On a TTY, the stack drives the interactive viewer with the alternate screen, scrolling, and
   keys. On a pipe, peek writes the first non-auxiliary mode's `render_to_pipe(ctx)` output straight
   to stdout. The logic is the same and only the target differs, so there is no parallel `Viewer`
   trait.
4. **Theme-aware everything.** All colored output uses `PeekTheme` roles. A theme switch re-renders
   the whole view without reading the file again.
5. **Compose modes, not viewers.** A new file type composes a list of view modes, such as text
   extract, render preview, hex, and info. One event loop runs them all. A new type doesn't get its
   own viewer.

## Crate structure

peek is a Cargo workspace. The `peek` **binary** sits at the repository root, and the **library
crates** sit under `crates/`. The crates are layers. Each crate depends only on the crates below it,
and Cargo makes sure that no dependency points back up. This turns architectural rules from
conventions into compile errors. One such rule is "detection must not depend on the readers."

```
peek (bin)        session layer: CLI + the compose / gather / extract dispatch hubs + the
  ▲               interactive event loop (viewer_session). Depends on every crate below.
peek-types        per-file-type readers (one module per type). Owns the parser deps. Depends on
  ▲               foundation / detect / io / theme.
peek-manual       the user manual embedded at build time (TOC + chapter extract + info). Beside
  ▲               peek-types, not above it: depends on foundation / detect / io / theme.
peek-foundation   reader/viewer toolkit + info base + output/extract vocab + base64/xml. Depends
  ▲               on theme / io / detect.
peek-theme  ◀──┐  theming leaf (PeekTheme + tmThemes + SGR). Parallel to peek-io.
peek-detect    │  file-type detection: FileType + format enums + magic/extension/content
  ▲            │  classification + MIME + transparent decompress-then-redetect. Depends on peek-io.
peek-io  ◀─────┘  input foundation: InputSource + streaming byte/line sources + bare codecs +
                  stdin/tty. Depends on nothing in-tree.
```

Each crate boundary has a reason:

- **`peek-io`** is the foundation for the "stream, don't load" principle. Everything reads through
  `InputSource`. Keeping it separate makes the I/O primitives easy to review and reuse, and the
  crate knows nothing about file types or rendering.
- **`peek-detect`** is the layer that most needs hardening in isolation. It is small, has few
  dependencies, and maps bytes and names to a `FileType`. It can't reach the reader crates, so
  `cargo tree -p peek-detect` is the test. Its output must stay free of the heavy reader
  dependencies, such as `calamine`, `rusqlite`, `pdfium`, `symphonia`, `object`, and `image`. The
  one heavy crate that it also pulls in is `x509-parser`. That crate belongs to the parser set of
  `peek-types`, because certificate parsing is the heavier surface. Certificate detection borrows it
  to verify the content of DER certificates.
- **`peek-theme`** is the terminal-styling leaf. It sits parallel to `peek-io`, and both are pure
  foundations.
- **`peek-foundation`** is the shared reader and viewer toolkit. It holds the `Mode` engine, the
  shared view modes, UI primitives, the image-render vocabulary, the info base, and the output and
  extract value types. It can reach `peek-theme`, `peek-io`, and `peek-detect`, but **not** the bin.
  This means the toolkit can't reach the event loop or process control.
- **`peek-types`** parses untrusted file bytes, such as fonts, PDFs, archives, and disk images.
  Cargo bars it from the bin's session layer, so a parser bug stays inside a crate with no surface
  for I/O control. `cargo tree -p peek-types` must not show the `peek` bin. The crate owns the set
  of heavy parser dependencies.
- **`peek-manual`** compiles `manual/src/` into a static chapter table in its `build.rs`. It serves
  the table as the `FileType::Manual` container for `peek --manual`. Chapters extract as in-memory
  `.md` sources, so the Markdown reader in `peek-types` renders them without either crate knowing
  about the other. The crate holds only trusted bytes that are compiled in.
- **The binary** is a thin session layer. It holds the command-line interface, the three
  `FileType → types::<x>` dispatch hubs (`compose.rs`, `gather/`, and `extract/`), and the event
  loop (`viewer_session/`). It names the lower crates directly, with no re-export shims: `peek_io`,
  `peek_detect`, `peek_theme`, `peek_foundation::{viewer, info, …}`, `peek_types::types`, and
  `peek_manual`.

The top-level file map is in [CLAUDE.md](../CLAUDE.md). For per-file detail, read the `//!` header
of each file.

## Data flow

```
CLI args (clap)
  |
  v
input::build_input() --> (InputSource, Detected)
  |   File path or buffered stdin, then detect::detect(source) --> FileType.
  |   --manual skips both: peek_manual::source() + FileType::Manual, which
  |   detection never produces.
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

### `InputSource` (`crates/peek-io/src/source.rs`)

`InputSource` lives in its own crate, `peek-io`, which is the dependency-free foundation described
in [Crate structure](#crate-structure). Code names it directly as `peek_io::*`.

`InputSource` separates where the data comes from and how peek displays it. It has four variants:

- `File` is a path on disk that peek reads on demand.
- `Memory { bytes: Bytes, name }` holds stdin, small extracted archive entries, and encoded
  animation frames. `Bytes::clone` bumps a reference count and doesn't copy the data.
- `FileRange { base, offset, len, name }` is a zero-copy view with an offset and a limit. ISO
  extracts and uncompressed archive entries use it.
- `TempFile { file: Arc<NamedTempFile>, name }` holds large extracted archive entries that peek
  spools to `$TMPDIR/peek-*`. peek deletes the file when it drops the last `Arc`, following the RAII
  pattern (resource acquisition is initialization).

All modes take `&InputSource` and call `read_text(Budget)` or `read_bytes(Budget)`. The image,
animation, and SVG decoders work from any variant. The required `Budget` (`peek_io::limits`) names
the shape of the read's memory use, so an unguarded whole-file read doesn't compile. For details,
see [Memory budgets](#memory-budgets). `read_bytes` returns `Bytes`, so the `Memory` arm clones a
reference count. If you want a copy, you must write `.to_vec()` at the call site.

For random access without reading the whole file, `open_byte_source() -> Box<dyn ByteSource>`
returns a seeking handle. `HexMode` reads only the visible window on each scroll. `File` and
`TempFile` seek on each call. `Memory` slices the buffered `Bytes`. `FileRange` wraps a `File`
reader and translates offsets. The `TempFile` byte source carries its own `Arc<NamedTempFile>`
clone, so reads keep working after the source is dropped.

For line streaming, `open_line_source() -> LineSource` (`crates/peek-io/src/lines.rs`) makes one
pass over the input. The pass counts newlines and records sparse byte anchors every 1024 lines.
After that, the source serves windowed line lookups in O(stride) time. `ContentMode` uses it, so
multi-GB text is never loaded into memory. Stdin and files share this path. Stdin is backed by an
`Arc<[u8]>`, so streaming it costs nothing beyond a slice. A file seeks for each chunk through
`FileByteSource`.

peek consumes stdin when the argument is `-`, or when there are no arguments and stdin is piped. In
that case, `peek_io::stdin::read_stdin` reads stdin into a `Memory` source. It then reopens file
descriptor 0 from the controlling terminal, so the event loop can still read keystrokes. It resolves
the terminal through `ttyname()` on stderr or stdout and doesn't open `/dev/tty` directly, because
macOS kqueue rejects `/dev/tty` with `EINVAL`. The command-line decision between a file and stdin
(`build_source`, which needs `Args`) stays in the bin at `src/input.rs`.

Detection runs every source through one `classify` core over a `Probe` in `peek-detect`
(`crates/peek-detect/src/detect.rs`). This covers files, sources resident in memory (stdin and
archive entries), and spooled streams. Detection checks in this order:

1. Name routing.
2. Magic bytes.
3. Content sniffing. A leading `{` or `[` means JSON, using the `Eof` prefix signal from serde. A
   root `<svg>` or `<html>` element means SVG or HTML. A leading `---` means YAML.
4. Plain text.

Strong magic that _contradicts_ a recognized extension overrides it, for example a `.csv` file that
holds a zip archive. Coarse magic that the extension only refines keeps the name, for example zip
with `.docx`, or `%PDF` with `.ai`.

## Key abstractions

Paths in this section are relative to `crates/peek-foundation/src/` unless they have a prefix. A
`src/…` path is in the bin, `crates/peek-theme/src/…` is in the theme crate, and
`crates/peek-types/src/…` is in the readers crate.

### `Mode` trait for interactive views (`viewer/modes/mod.rs`)

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

`render_window` is the single rendering contract. The mode gets a viewport request of
`(scroll, rows)`. It returns the visible slice and the line count of the full source in `total`.
`ViewerState` writes the slice as is and uses `total` for scroll math. Streaming modes, such as
`ContentMode`, honor the window and fetch only what's visible. Fixed-content modes, such as Info,
Help, and About, build their full output and then slice it with `slice_window`. With
`total_lines()`, a mode can report its line count cheaply. `ContentMode` returns
`LineSource.total_lines()` in O(1) time, so a jump to the bottom doesn't force a render.

`is_aux()` marks Help, About, and Hex as auxiliary modes. You reach them only through their own keys
(`h`, `a`, and `x`). The Tab cycle of primary modes skips them, and their key toggles back to
`last_primary`. Info is _not_ auxiliary because it doesn't override `is_aux`. It sits in the Tab
cycle, and `i` is a one-way shortcut to it. With `status_hints`, a mode can add hints on the right
side of the status line when they apply. For example, Hex shows `x:exit hex` only when there is a
mode to return to. `Handled::YesResetScroll` sets the active mode's scroll offset to zero. Modes
return it when an action makes the previous position invalid, for example when `ContentMode`
switches between pretty and raw.

`extract_target`, `select_jump`, `build_descend_frame`, and `jump_position` are the mode side of
recursive peek. They define what the extract key saves and what Enter descends into. The session
side, with the resolution order and the frame stack, is in
[Session stack and recursive peek](#session-stack-and-recursive-peek-srcviewer_sessionframers).

The trait surface has two deliberate asymmetries. Checkup M18 and L4 reviewed them and kept them:

- The `scroll` argument of `render_window` is unused for modes where `owns_scroll()` is `true`.
  These modes keep their own position, such as a byte offset, a wrap-aware line, or a page index,
  and ignore the caller's value. A split into `ScrolledMode` and `OwnsScrollMode` would drop the
  unused parameter, but it would also split the mode vocabulary and the dispatch in `ViewerState`.
  That is too much surface for one ignored argument, and most data modes own their scroll anyway.
- Only `HexMode` reads the `has_return_target` argument of `status_hints`. The parameter stays
  because it is the only channel through which session context can reach a mode in the foundation
  crate. Cargo layering bars modes from calling back into the bin's `ViewerState`, and a setter on
  the mode would mean state that you sync by hand at every switch.

A `Mode` is one view of a file that renders and reacts to input. The interactive viewer drives a
`Vec<Box<dyn Mode>>`. Tab cycles through the modes, and `i`, `h`, and `x` are shortcuts to Info,
Help, and Hex. The current modes are:

| Mode                   | Used by                                                                                                                                                                            | Owns scroll?                  | Reacts to resize? |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- | ----------------- |
| `ContentMode`          | text, source, structured, SVG XML                                                                                                                                                  | **yes**                       | **yes**           |
| `RenderedTextMode<R>`  | whole-document read views (DOCX / ODT / RTF / HTML / PDF text / vCard / iCalendar)                                                                                                 | no                            | **yes**           |
| `PagedTextReadMode<R>` | paged-text read views over a `PagedText` reader: presentation slides + EPUB chapters (per-page search, EPUB adds cover render)                                                     | no                            | **yes**           |
| `ListingMode`          | generic listing engine over a `ListSource`: container TOCs (archive / ISO / PDF / EPUB / DOCX / ODT / audio / comic / SQLite / LMDB / spreadsheet) + filesystem directory listings | **yes**                       | **yes**           |
| `HexMode`              | binary, reachable from any view via `x`                                                                                                                                            | **yes** (byte-aligned)        | **yes**           |
| `ImageRenderMode`      | raster + rasterized SVG                                                                                                                                                            | **yes** (FitWidth/Height pan) | **yes**           |
| `AnimationMode`        | GIF / WebP (drives `next_tick`/`tick`)                                                                                                                                             | **yes**                       | **yes**           |
| `SvgAnimationMode`     | CSS-`@keyframes` SVG (lazy per-frame raster)                                                                                                                                       | **yes**                       | **yes**           |
| `PagedImageMode<R>`    | PDF / CBZ paged image render                                                                                                                                                       | **yes**                       | **yes**           |
| `SpecimenMode`         | font specimen rasterisation (`.ttf` / `.otf` / `.ttc`)                                                                                                                             | **yes**                       | **yes**           |
| `TableMode`            | object file / classfile aligned tables                                                                                                                                             | **yes**                       | **yes**           |
| `RowsTableMode`        | streaming CSV / TSV + SQLite contents + LMDB rows, opt-in row cursor (`Enter` / `e` via `RowSource::extract_target`)                                                               | **yes**                       | **yes**           |
| `InfoMode`             | every file (file metadata)                                                                                                                                                         | no                            | no                |
| `HelpMode`             | every file (keyboard-shortcut listing)                                                                                                                                             | no                            | no                |
| `AboutMode`            | every file (logo, version, palette swatches)                                                                                                                                       | no                            | no                |

### Pipe-mode rendering (`Mode::render_to_pipe`)

`render_to_pipe` is the print-path entry point on every mode. The default implementation builds the
output of `render(ctx)` and writes each line to `PrintOutput`. Two kinds of modes override it: modes
that stream from a `ByteSource`, such as `HexMode`, and modes that need byte-faithful raw output,
such as `ContentMode` without a syntax token. On the pipe path, `RenderCtx` sets `term_cols` to
`$COLUMNS` or 80, and `term_rows` to `usize::MAX`. This way one `render` body serves both contexts
when the mode doesn't need a bounded viewport.

`main` picks the first non-auxiliary mode as the pipe primary. If all modes are auxiliary, it uses
the first one. This happens for binary files, where the stack is `[Hex, Info, About, Help]`.

### `ViewerState` (`src/viewer_session/`)

`ViewerState` is the interactive controller. It is one type split across four files by concern:

- `state.rs` holds the struct, key dispatch, `apply`, and mode switching.
- `frame.rs` holds `SessionFrame` and the recursive-peek stack, descend, and extract.
- `prompt.rs` holds the modal-prompt slot and the confirm dispatch.
- `render.rs` holds the view cache, render-failure recovery, the caller-side scroll math, and
  `draw`.

The state splits into what belongs to one session and what lasts across sessions. Each
`SessionFrame` owns one peek session. It holds the source, the detected type, the `FileInfo`, the
mode list, and the active index. It also holds the `last_primary` slot (the most recent
non-auxiliary mode), the scroll offsets for each mode, a lazy cache of each mode's rendered view,
and a `Position` (the last known logical location). `ViewerState` holds the frame _stack_ and the
state that survives across frames: the theme, the `ScreenBuffer`, the prompt slot, the status flash,
and the `ModeBuilder` closure. `ViewerState` captures the closure at construction, so a descend can
compose modes for a new frame without knowing about `Registry` or `Args`.

`apply()` handles session-level actions, such as scrolling, cycling the theme, switching modes,
extracting, and descending. The event loop first tries the active mode's `scroll()` and `handle()`,
and then falls through to `apply()`. This keeps mode-local actions, such as `r` for raw or pretty
and `b` for background, scoped to the mode. `Action::is_mode_local` declares the split for each
variant. It uses an exhaustive match, so a new variant doesn't compile until you assign it to a
category.

### Session stack and recursive peek (`src/viewer_session/frame.rs`)

The stack is a `Vec<SessionFrame>`, and the active frame is the last entry. The status breadcrumb
joins the frame names, as in `archive.zip > inner.tar > notes.txt`. When a synthetic frame reuses
the parent's source, the frame's `breadcrumb_label` overrides the source name. For example, the
SQLite table view shows the table name instead of showing the database filename twice.

Enter (`Action::Descend`) tries three mode hooks in this order:

1. **`select_jump`** is an in-frame jump. It switches to a sibling mode and seeks it to a
   `Position`, for example from an object-file symbol to its byte offset in Hex. The stack doesn't
   change. The target's `jump_position` runs, so the target can mark the spot where it landed.
2. **`build_descend_frame`** returns a frame that the mode builds itself, bypassing the extract
   pipeline. It serves synthetic views over the _current_ source that would otherwise go to a
   temporary file, such as the row viewer for a SQLite table.
3. **`extract_target`** is the standard path. peek extracts the selection, runs `detect` on the
   result, and runs `resolve_transparent`, so descending into an extracted `.gz` file lands on the
   inner content. It then composes modes through the `ModeBuilder` and pushes the new frame.

A descent from one directory into another _replaces_ the current frame instead of pushing a new one.
This way, browsing sibling subdirectories doesn't build up a stack that you have to back out of.
`Back` (Esc) pops a frame, and at depth 1 it quits. `MAX_STACK_DEPTH` (16) caps the stack, so a
hostile container that resolves to itself can't grow the stack without bound. When a descend fails,
for example because nothing is selected, the type is unsupported, the entry is broken, or the stack
is full, peek flashes the reason on the status line and keeps the current frame active.

### Position tracking

`Position` is `Unknown`, `Byte(u64)`, or `Line(usize)`. On every change of the active mode, peek
captures the position from the outgoing mode and pushes it to the incoming mode. Modes that override
`tracks_position()` take part, and the others pass it through. As a result, detours through Info,
Help, Image, or Animation keep your place. `InputSource` converts between the two forms with
`byte_to_line` and `line_to_byte`, which use a streaming scan in 64 KB chunks.

Pretty-printed structured content has more lines than the raw source, so the displayed line index
doesn't map cleanly to bytes. `ContentMode` opts out while pretty mode is active: its
`tracks_position()` returns `!use_pretty`. When you switch from pretty Content to Hex, Hex stays on
whichever byte it was last on instead of making up a wrong one. The plan is for modes that need an
exact mapping to carry their own table from line to source byte.

### `Registry` (`src/compose.rs`)

`Registry` is a factory that peek builds once from the command-line arguments. It holds the shared
`ThemeManager`, the resolved `PeekTheme`, and the `plain_mode` flag that composition reads. It
provides `compose_modes(source, detected, args)`, the single dispatcher that produces the mode stack
for both the event loop and the pipe path.

`--plain` deliberately does more than `--color plain`. Checkup M12 declined a merge of the two.
`StyleMode::Plain` only drops ANSI escape codes at the encoder. `plain_mode` also turns off
structured pretty-print, skips the syntect pipeline entirely (`syntax_token = None`), and turns off
rendered views such as SVG, HTML, and Markdown. A user who wants plain _colors_ still expects
pretty-print and rendered views to work, so the two flags must stay separate. `cli::parse` computes
the effective encoding once into `args.color`. It is `plain` under `--plain`, and otherwise it is
`--color` with `auto` resolved. `args.color_arg` keeps the value that the user actually chose.

### `HexMode` (`viewer/modes/hex.rs`)

One file holds the layout primitives and the `Mode` implementation built on them. The primitives are
`bytes_per_row` (`14 + 4*bpr` columns, rounded to a multiple of 8), `align_down`, `max_top`, and
`format_row`, whose layout matches `hexdump -C`.

`HexMode` owns a `Box<dyn ByteSource>` and a `top_offset: u64` that is aligned to the current
`bytes_per_row`. Because `owns_scroll()` is `true`, `ViewerState` doesn't apply its line scroll.
Instead, `scroll()` handles ScrollUp, ScrollDown, PageUp, PageDown, Top, and Bottom by bytes.
`on_resize` aligns `top_offset` again to the new column count. `render_to_pipe` streams the whole
file in 4 KB chunks straight to the print sink. It never holds more than one chunk, so multi-GB hex
dumps are first-class.

### `ContentMode` (`viewer/modes/content.rs`)

`ContentMode` streams the raw view from a `LineSource`, an anchor-indexed iterator over an
`InputSource`. Each render fetches only the lines that are visible after a scroll, so multi-GB text
is never loaded into memory. When there is a syntax token, `LineStreamHighlighter`
(`viewer/highlight.rs`) carries the syntect `ParseState` and `HighlightState` across `feed()` calls.
This way, constructs that span lines, such as block comments and here-docs, highlight correctly.

A backward scroll past the highlighter's cursor resets it and replays forward. Reading from top to
bottom is cheap, but an unusual backward jump in a huge file pays a one-time cost. A theme cycle
also resets the state, because the cached styles come from the theme. A color cycle takes effect on
the next `feed()` without a reset.

Pretty-print reads the whole file, with a cap. The cap, `PRETTY_MAX_BYTES`, is the whole-document
budget class. Over the cap, `ContentMode` pushes a warning and clears `use_pretty`, and the streamed
raw view takes over. Below the cap, pretty-print runs lazily on first access. `ContentMode` caches
the parsed text. With a syntax token, it also caches the highlighted pretty form, keyed by
`(theme, color)`, so a cycle invalidates and recomputes it.

If parsing fails, `ContentMode` caches the `Err`, falls back to raw, and queues a one-time warning
through `take_warnings()`. `ViewerState` polls `take_warnings()` after each render and merges new
entries into `FileInfo.warnings`. It also invalidates the cached lines of `InfoMode`, so the next
`i` shows the warning beside any extension-mismatch notices.

On the pipe path, highlighted output ends each line with `\n`, because escape codes are scoped to a
line. Output without highlighting keeps the source's trailing-newline status
(`LineSource.ends_with_newline()`), so it matches `cat` byte for byte.

### Animation (`types/image/animation_mode.rs` and `types/image/pipeline/animate.rs`)

`animate.rs` decodes all GIF and WebP frames up front (`decode_anim_frames`) and exports
`render_frame`. The choice between `AnimationMode` for animated images and `ImageRenderMode` for
static ones lives in `types::image::compose::compose`, so `main.rs` has one uniform interactive
path.

`AnimationMode` owns the frame list, the `current` index, the `playing` flag, the `last_advance`
instant, and an `ImageConfig`. It drives the event loop's timeout through `next_tick()`. That method
returns the time left until the next frame, or `None` when the animation is paused or you're on a
detour to Info, Help, or Hex. When `event::poll` times out, `tick()` advances `current` and signals
a redraw.

### SVG animation (`types/svg/animation_mode.rs` and `types/image/pipeline/svg_anim/`)

resvg and usvg don't evaluate CSS animations. To play an animated SVG, the parser in
`types/image/pipeline/svg_anim/` extracts the timeline from the SVG itself. The parser consists of
`mod.rs`, `scan.rs`, `spec.rs`, `keyframes.rs`, `timeline.rs`, `marker.rs`, and `util.rs`. It scans
`<style>` blocks for `@keyframes` and matches elements with an inline
`style="...animation-name:..."` to those rules.

The parser builds an `AnimatedSvg`, which has two parts:

- A marked SVG string with a `__PEEK_ANIM_<i>__` placeholder at the opening tag of each animated
  element.
- A merged frame timeline, with one entry and its hold delay for each visible transition.

`render_frame(model, idx)` replaces each placeholder with `transform="..."` to produce a complete
SVG for frame N that resvg can rasterize.

`SvgAnimationMode` has the same controls as `AnimationMode`: play and pause, frame navigation, fit,
and scroll. It rasterizes each frame lazily through `render::prepare_svg_bytes`. A bounded
`VecDeque<(CacheKey, PreparedImage)>` of size 64 holds the recent composited frames, keyed by
`(frame_idx, cols, rows, margin, ascii, fit)`. Once the cache reaches a steady state, replaying the
full loop costs nothing. The mode clears the cache when you toggle the mode, background, or fit,
because the prepared grid no longer matches.

Composition happens in `types::svg::compose::compose`. For an SVG, it first tries
`svg_anim::try_parse`. If that finds a model, it pushes `SvgAnimationMode`. Otherwise, it falls back
to a static `ImageRenderMode`. `--no-svg-anim` skips the parsing.

For the memory profile, an analysis of the latency of the first loop, and options that aren't
implemented yet, see [svg-anim-perf.md](svg-anim-perf.md). Phase 1 is the working baseline, and the
performance doc is the queue of further work.

### `ImageConfig` (`types/image/pipeline/mod.rs`)

`ImageConfig` bundles the image rendering parameters (mode, width, background, margin, and color
mode) into one struct that the pipeline passes along.

### `PeekTheme` (`theme/`)

The theme code is split by concern:

- `name.rs` holds `PeekThemeName` and the embedded `.tmTheme` data.
- `style_mode.rs` holds `StyleMode` and the helpers that map RGB to a palette.
- `peek_theme.rs` holds the `PeekTheme` struct, the paint helpers, and `lerp_color`.
- `manager.rs` holds `ThemeManager`, which owns the shared `SyntaxSet` and `ThemeSet` and the active
  `PeekTheme`.

The semantic roles derive automatically from syntect `.tmTheme` files. All colored output goes
through `PeekTheme::paint()`. For continuous scales, such as file size, age, and resolution,
`lerp_color()` interpolates between colors.

`PeekTheme` carries a `StyleMode`, which is `TrueColor`, `Ansi256`, `Ansi16`, `Grayscale`, or
`Plain`. The `StyleMode` owns the conversion from RGB to the bytes that go to the terminal. Callers
always paint with truecolor RGB, and the mode picks 24-bit, 256-color, 16-color, luminance, or no
escape codes. Image rendering uses the same conversion through `StyleMode::write_fg` and
`write_fg_bg`. `--color` or `PEEK_COLOR` sets the mode, and you can cycle it with `c`. Cycling
invalidates the line cache of every mode, so the UI repaints in the new encoding.

`viewer::ranges_to_escaped_trim_newline` is the shared escape walker for syntect's `LineRanges`. It
replaces syntect's `as_24_bit_terminal_escaped`, which always emits 24-bit color, and routes the
output through `StyleMode::fg_seq`.

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

**The order matters:** resize _before_ you composite. Otherwise, the checkerboard doesn't line up
with the glyph grid at the final resolution.

**Windowed render:** under `FitWidth` or `FitHeight`, the prepared grid can be larger than the
terminal. The renderer never builds full lines and slices them afterward, because taking a
horizontal substring of a styled string means parsing ANSI escape codes. Instead, the inner cell
loops iterate over the sub-range from `GridWindow`, so the emitted strings are already windowed.
`ImageRenderMode::owns_scroll()` is `true`, and the mode tracks `scroll_x` and `scroll_y`. The pipe
path and `--print` always render with `Contain`.

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

`redraw` calls `state.ensure_active_rendered()` to render the mode lazily. It then composes the
status line from the name, the mode label, the status segments, and the theme, and calls
`state.draw()`.

**Render-failure fallbacks.** `ensure_active_rendered` never lets bad input stop the viewer. When
the active mode's `render_window` returns an error, peek tries two recoveries before it gives up:

1. `retry_frame_detection` detects the type again with `detect_ignore_name`, which uses only magic
   bytes and ignores the path. If the file's extension was wrong, this rebuilds the frame with the
   correct type.
2. If the retry doesn't apply, because it already ran or the new detection agrees, peek falls back
   to `degrade_active_to_hex`. The active mode points at the Hex view, which is always present. peek
   pushes the decode cause onto `FileInfo.warnings` and flashes it. The cause is the deepest error
   in the chain, for example a PNG `CRC error`. If `last_primary` pointed at the broken mode, peek
   clears it.

The error propagates only when nothing safer remains: when the failed mode _is_ Hex, or when there
is no Hex mode, as with directories. The pipe path in `main.rs` works the same way. If the primary
mode's `render_to_pipe` fails, peek falls back to Hex and prints the cause on stderr. As a result, a
corrupt image, a truncated archive, or a malformed payload shows a hex dump and a warning instead of
crashing.

### Modal prompt overlay (`viewer/ui/prompt.rs`)

`ViewerState` has a single `Option<(Prompt, PromptKind)>` slot. While it is `Some`, raw key events
go to the `Prompt`, which is a readline-style input, and the status line shows the prompt. Global
keys and mode keys do nothing until the prompt closes. `PromptKind` is the work to run on confirm,
so one overlay serves two flows:

- `Extract` asks for the path to save to and writes the `Extracted` payload.
- `Search` hands the query to the active mode's `set_search`.

Esc cancels. Confirming an empty query clears the search.

### Text search (`viewer/search.rs`)

`/` opens the `Search` prompt, and `Ctrl-R` toggles between literal and regular expression search.
peek remembers the choice across searches. On confirm, peek compiles a `SearchQuery` and calls
`Mode::set_search(Some(&query))` on the active mode. If the regular expression is invalid, peek
flashes the parse error instead.

`SearchQuery` is the one compiled matching primitive. It is either `Literal`, an exact substring and
the default, or `Regex`, which uses the linear-time `regex` engine. Every scan site runs against it,
so regular expressions work for all file types without wiring for each type. Searchable modes scan
their lines into a `SearchState`, which holds every match and the `n` and `p` cursor, and they arm
the highlight overlays.

`search.rs` holds the shared pieces:

- `smart_case_sensitive` makes the search case-sensitive if the query has any uppercase letter.
- `find_matches` finds non-overlapping byte ranges of an exact substring. It is the backend of the
  `Literal` arm.
- `SearchQuery::find` dispatches to the literal or regular expression search for each line.
- `overlay_matches` paints `search_match` or `search_current` onto a line that already has SGR
  (Select Graphic Rendition) styling. It drops the syntax color under a match.
- `SearchState` itself.

The scan is one full pass over the lines of the active view, capped at `MAX_MATCHES` (100 000).

`set_search` returns the line of the first match for the caller to scroll to. Modes that own their
scroll ignore it and scroll themselves. `n` and `p` go through `step_search`, a `handle` helper that
every caller-scrolled searchable mode shares. It steps the `SearchState` cursor and returns
`Handled::YesScrollTo`. `ContentMode`, the rendered HTML view, and the EPUB, DOCX, ODT, RTF, and PDF
text read views implement `set_search`. The default trait implementation does nothing, so modes
without text opt out at no cost. A mode drops its `SearchState` when the set of scanned lines
changes, for example on a raw or pretty toggle, a chapter step, or a resize.

### View cycle: `Tab`, `i`, `h`, `x`, `a`

Tab cycles through the file's view modes. It includes every mode except the auxiliary overlay modes
(`Help` and `About`) and `Hex`, which has its own key. Some examples of the cycle:

- SVG: `ImageRender → ContentMode (XML source) → Info`.
- Text and source code: `Content → Info`.
- Animated image: `AnimationMode → Info`.

Binary files are the exception, because `Hex` _is_ the data view there. When the stack has no
non-auxiliary mode, `cycle_view` includes `Hex`, so Tab still toggles `Hex ↔ Info`.

You can reach the auxiliary modes (`Help`, `Hex`, and `About`) only through their own keys: `h` or
`?`, `x`, and `a`. The mode itself declares that it is auxiliary through `Mode::is_aux()`. Nothing
is hardcoded, so a new auxiliary mode needs only one overridden method and no changes to
`ViewerState`. `h`, `x`, and `a` share `ViewerState::toggle_aux(target_id)`. If the active mode _is_
the target, it returns to `last_primary`. Otherwise, it enters the target. `i` (`SwitchInfo`) is a
one-way jump to Info.

`r` is local to `ContentMode`, where it toggles between pretty and raw for structured JSON, YAML,
TOML, and XML. Modes that don't consume `r` ignore it, and there is no global fallback.

`last_primary` updates whenever the active mode lands on a non-auxiliary mode. Moving between
auxiliary modes, as in Hex → Info → Hex, leaves it alone. This way, the path back to your actual
work survives any detours. If you go from Hex to Info and then press Tab, you return to the original
primary mode, not to Hex.

For binary files, the stack is `[Hex, Info, About, Help]` and has no primary mode, so `last_primary`
stays `None`. Leaving an auxiliary mode falls back to mode 0, which is Hex. As a result, `x` from
standalone hex does nothing, and Tab toggles `Hex ↔ Info` through the binary branch in `cycle_view`.

## Memory budgets

> For the strategy, see [memory-streaming.md](memory-streaming.md). It covers the threat model, the
> stream, cap, and spill mechanisms, and the decision rule that every new read path follows. This
> section is the _index_ of budget classes.

Every size gate takes its number from one of three budget classes in `peek-io::limits`. Foundation,
types, and the bin name it directly as `peek_io::limits`. Each class is named after the shape of its
memory use. A constant at a call site aliases a class and keeps its domain name and local rationale.
A limit belongs to a class because of its rationale, not its number. A byte limit that guards a
different shape, such as a cap per record, a pixel ceiling, or a count cap, stays local to its site.

| Class                 | Size   | Shape                                                                                 | Members                                                                                                                                                                                                                                        |
| --------------------- | ------ | ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `WHOLE_DOC_BYTES`     | 32 MB  | materialize **and transform** (5–20× expansion, blocks the UI during parse+highlight) | `RENDER_MAX_BYTES` (rendered views + `read_zip_entry` payloads), `PRETTY_MAX_BYTES` (structured pretty-print), the UTF-16 CSV transcode (`csv/parse.rs`)                                                                                       |
| `SIDECAR_PARSE_BYTES` | 64 MB  | whole-text read, small derived output                                                 | `SIDECAR_TEXT_LIMIT` (markdown / SQL / CSS info), the UTF-16 text-stats decode (`text/info_gather.rs`), `DMG_PLIST_MAX_BYTES`                                                                                                                  |
| `BULK_WALK_BYTES`     | 256 MB | one bounded pass over untrusted / unbounded data, nothing proportional retained       | `MAX_EXTRACT_BYTES` (per archive entry), `SEARCH_SCAN_MAX_BYTES` (raw-content + table-cell search), `STATIC_LIB_SUMMARY_CAP` (`ar` object-member summary: materialized whole but held for one pass. Real `.a` files exceed the sidecar budget) |

The transparent-decompression path (`resolve_transparent` → `decompress_to_source`) does _not_ alias
a class. It streams the compressed input and spills the decompressed output to a temporary file once
it passes `DECOMPRESS_SPOOL_THRESHOLD` (16 MB). RAM use stays bounded by the spill threshold
regardless of the inner size. So an arbitrarily large bare-codec file, such as `bigdb.sqlite.xz`,
opens the same way as the identical entry inside a `.tar.xz` file. Disk capacity bounds the spilled
path. The 16 MB threshold matches the spool threshold for archive extraction
(`extract.rs::SPOOL_THRESHOLD`).

Use one of these gate helpers instead of writing your own check:

- `InputSource::read_bytes(Budget)` and `read_text(Budget)` (`peek-io/source.rs`). The required
  `Budget` (`peek_io::limits`) is the gate. `Budget::WholeDoc`, `Sidecar`, and `BulkWalk` refuse
  inputs over the matching class through a cheap `byte_len` check before any allocation. They serve
  parse paths that have no streaming option, such as `object::File`, the EPS header, and the
  notebook JSON. With them, compose and info can degrade instead of reading a multi-GB file. They
  show only Info, a streaming source view, or a section less. `Budget::Unbounded("why-safe")` is the
  only escape hatch, for sources that are bounded by construction. You can grep for its `&str`
  reason.
- `ensure_under_render_cap(len, what)` and `render_cap_exceeded(len, what)`
  (`viewer/modes/rendered_text.rs`) refuse or warn before a whole-document read.
- `read_zip_entry` (`types/archive/reader.rs`) reads a zip entry's payload behind a gate that checks
  both the declared size and the actual size.
- `gather_capped_text` (`types/text/info_gather.rs`) reads whole text with a cap, for sidecar
  parsers.
- `SearchState::scan_capped` (`viewer/search.rs`) scans a streaming source within a byte budget.

Some limits stay local on purpose, because they guard different shapes and aren't class members:

- CSV `MAX_RECORD_BYTES` and `MAX_RECORD_LINES`, which apply to each record.
- `MAX_MATCHES`, which is a count.
- The animation budgets for each frame and in total.
- `PDFIUM_RENDER_CAP_PX` and `SVG_RASTER_CAP_PX`, which are pixel ceilings.
- `MAX_FIT_CELLS` and `MAX_FORCED_WIDTH_CELLS`, which are ceilings on the image cell grid
  (`types/image/pipeline/render.rs`).
- ISO `MAX_DIR_BYTES`, which is a sanity bound on metadata.

## Adding a new file type

For the complete checklist of the files that a type owns and the sites to wire, see
[conventions.md → File types](conventions.md#file-types). In short:

1. Add a `FileType` variant in `crates/peek-detect/src/detect.rs` and wire up detection. The format
   enum and the helpers for extension, MIME, and content sniffing live in
   `crates/peek-detect/src/types/<x>.rs`, in `peek-detect` and **not** in the reader. Re-export the
   format enum from the reader root (`crates/peek-types/src/types/<x>/mod.rs`:
   `pub use peek_detect::types::<x>::<X>Format;`), so reader code keeps a local
   `crate::types::<x>::<X>Format` path. Cargo makes sure that detection stays free of readers.
2. Create `crates/peek-types/src/types/<x>/` and build the type's `Mode` implementations there. The
   generic reusable modes, such as `ContentMode`, `RenderedTextMode`, `PagedImageMode`, and
   `ListingMode`, live in `peek-foundation` (`viewer/`). Wrap one of them if you can, instead of
   writing a custom `Mode`. If a mode must be toggleable by ID, add a `ModeId` variant. Override
   `render_to_pipe` if the default wastes memory or breaks byte fidelity. The default builds the
   whole output and then writes it.

   **You must gate every read of a whole file or a whole payload.** `read_bytes` and `read_text`
   take a required `Budget`, so pick the class that matches the shape of the read. Render and
   container paths have their own helpers: `ensure_under_render_cap` for renders,
   `gather_capped_text` for sidecar parses, and `read_zip_entry` for container payloads. Use
   `Budget::Unbounded("why")` only for sources that are bounded by construction.

3. Add `types/<x>/compose.rs` with a `compose()` function that pushes the type's modes. Then add
   **one arm in `src/compose.rs`** (`Registry::compose_modes`) that delegates to it. peek appends
   Hex, Info, About, and Help automatically. Pipe mode picks the first non-auxiliary mode, or the
   first mode if all are auxiliary. The per-type `compose()` lives in `peek-types`, and only the
   dispatch arm lives in the bin.
4. Add `types/<x>/info_gather.rs`, with a `gather_extras(...)` function that returns `Extras`, that
   is, `Box::new(<Stats>)`. Also add `types/<x>/info_render.rs`, with one `impl_info_extras!` row
   that binds the stats struct to `InfoExtras`. `info::render` dispatches through the trait, so
   there is no render match for each type. Wire **one arm in `src/gather/mod.rs`** that calls
   `gather_extras`. A tiny type can combine gather and render in one `info.rs`.

   Build the section in one of **three modes**. In order of preference:
   - **Derive, for regular sections. This is the default.** Define a view struct with
     `#[derive(serde::Serialize, peek_foundation::info::InfoView)]` (`info/section.rs`), so the
     print output and the `--info --json` output both come from one definition. Use
     `#[info(label/nest/skip/title/title_from)]` for the print tree and `serde` attributes for JSON.
     Paint each field through `InfoValue`. Cells hold the semantic `Value` (`Size`, `Count`,
     `Timestamp`, `Text`, and so on, in `info/value.rs`), which peek paints by role. `Muted`,
     `Accent`, and `Warn` are wrapper types for strings in other colors. When a leaf's print text
     and JSON value **differ**, use `Value::split(text, Role, json)` or
     `Value::labelled(label, token)` and not a custom wrapper type. Examples are a leaf that prints
     `ELF` but serializes as `"elf"`, and a joined string that serializes as an array. There are two
     ways to wire this mode:
     - **The stats struct derives directly.** Put `InfoView` and `Serialize` on the gathered struct,
       with no separate view. Use `impl_info_extras!(<Stats>, json = "<key>")`. That is one row and
       no free functions.
     - **A separate view through `From<&Stats>`.** The gathered struct projects into a private view
       struct. `crate::info_section!(<Stats>, <View>, "<key>")` in `info_render.rs` generates the
       standard `render_section` and `json_section` pair, private to the view. Wire the pair with
       the three-argument form `impl_info_extras!(<Stats>, ...::render_section, ...::json_section)`.
       The 23 migrated types, such as `svg`, `css`, and `image`, use this form.
   - **`InfoRow`, for irregular sections that are still made of rows.** Use this mode for
     dispatching on enum variants, or when one print row maps to several JSON keys. The derive walks
     struct _fields_, so it can't express a `Vec<enum>` whose variants lay out differently. Build a
     `Vec<InfoRow>` for each entry (`info/rows.rs`). Each row carries an optional print label, an
     optional JSON key, and a `Value` cell. The constructors are `InfoRow::new`, `text`, `count`,
     `int`, `muted`, and others, plus `print_only` and `json_only`. One list feeds both outputs:
     `push_rows` builds the themed lines, and `rows_to_json` builds the object, so the two can't
     drift apart. `cert` and `font` are the worked examples. You still build the section frame by
     hand in `render_section` and `json_section`, including which blocks exist and any custom
     headers.
   - **Fully custom, which is rare.** For a layout that neither of the other modes covers, such as
     pre-painted composite cells or nested partition tables, wrap the gathered struct. Then
     implement `InfoView::info_nodes` by hand, building the `InfoNode` tree and capturing renderers
     based on `lines` as `Line` nodes. Implement `serde::Serialize` by hand as well.

   The `InfoRow` mode, the custom mode, and the derive case with a `From` projection all wire
   through the free-function `render_section` and `json_section` form of `impl_info_extras!`. Only
   the projection case generates that pair through `info_section!`. The other two write it by hand.

5. For a container type, add `types/<x>/extract.rs`, which returns the `Extracted` or `ExtractError`
   types from `peek_foundation::extract`. Then add **one arm in `src/extract/extract.rs`**.

A new type is therefore an entry in `peek-detect`, a module in `peek-types`, and up to three
one-line dispatch arms in the bin, for compose, gather, and extract. For details, see
[conventions.md → File types](conventions.md#file-types).

### Why the dispatch arms stay explicit (no `FileTypeRegistry`)

Checkup M6 considered and declined a `trait FileTypeRegistry` that wires all per-type dispatch in
one place. The decision came after the sixth file type showed that the cost of explicit arms is
acceptable:

- **Detection can't join.** `detect.rs` _produces_ the `FileType` from magic bytes, the extension,
  and content sniffing. There is no `FileType` value to dispatch on yet, so a registry from
  `FileType` to implementation can never absorb all the wiring sites.
- **Format sub-enums break a one-to-one map from type to implementation.** `Archive(fmt)`,
  `Document(fmt)`, `Audio(fmt)`, and `Cert(fmt)` also dispatch on the inner format, so a uniform
  per-type trait fits awkwardly.
- **The explicit `match` makes the compiler check completeness.** A missing compose, extract, or
  gather arm is a compile error. A registry trait with default methods would silently do nothing.
  `compose_modes` also keeps the whole dispatch table readable in one file.

Touching about six sites for each new type is mechanical work that the compiler guides, so the
cognitive load is low. Revisit this only if a dispatcher could someday fall through silently and
ship a bug. Today a missing `mime` or info arm degrades visibly, with a `?` MIME type or a missing
Info section, and never silently.

As an example, here is the PDF composition (`crates/peek-types/src/types/pdf/compose.rs`):

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

The user gets Tab cycling through `page render → text extract → embed listing → Info`. `n` and `p`
step through pages, `e` extracts attachments from the listing, `x` switches to hex, and `i` switches
to Info. peek loads Pdfium dynamically from `libpdfium.*` beside the binary.
`pdf::package::locate_bindings` searches for the library in this order:

1. The directory of the executable.
2. The dev fallback `.pdfium/{lib,bin}`. The Windows Pdfium tarball ships the DLL under `bin/`, and
   the Unix tarballs ship the dylib under `lib/`.
3. The system library path.

## Adding a new theme

1. Add `crates/peek-theme/themes/<name>.tmTheme`.
2. Add a `PeekThemeName` variant in `crates/peek-theme/src/name.rs`.
3. Wire up `include_str!()`, `cli_name()`, `tmtheme_source()`, `next()`, and `help_text()`.

The `PeekTheme` semantic roles derive automatically from syntect, so you don't need to define them.
