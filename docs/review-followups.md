# Review follow-ups (2026-04-26)

Items from a thorough architectural review that were **not** fixed in the same pass — architectural
choices, feature work, or low-impact polish. Each has the symptom, location, why it wasn't
auto-applied, and how to address it.

For context, the review's clear-cut bugs and small cleanups were landed in that change-set:

- `with_alternate_screen` panic-safety via `Drop` guard
- `Mode::handle` → `Handled` enum so ContentMode pretty/raw can reset scroll
- Env-var test refactored to a pure helper (no `unsafe set_var`)
- `syntax_token_for` deduped between `Registry` and `SyntaxViewer`
- Animation render path now reuses `render::render_decoded`
- `gather_text_extras` streams via `open_byte_source` instead of slurping
- `Mode::is_aux()` and `Mode::status_hints()` replace hardcoded `ModeId` matches in `ViewerState` /
  `interactive`
- B5 — `compose_status_line` / `strip_ansi_width` / `truncate_ansi` covered by unit tests in
  `viewer/ui/mod.rs` (fits, hints-truncated, left-truncated, CJK widths, trailing-escape
  preservation)
- C3 — `format_unix_permissions` now emits `ls -l` output: type prefix (`-` / `d` / `l` / `b` /
  `c` / `p` / `s`) and setuid/setgid/sticky overlays (`s` / `S` / `t` / `T`). `paint_permissions`
  accommodates the new chars and 10-char layout.
- C2 — `decode_anim_frames` and `anim_frame_count` accept the upstream `magic_mime`; short-circuit
  to the matching `AnimFormat` when `image/gif` / `image/webp`, skipping the redundant
  extension/sniff path.
- B2 — `--info` is unconditionally non-paginated. New `Output::direct()` constructor; main's
  `--info` branch uses it. Framing: `--info` is a fixed-size summary; for scrolling, use the
  interactive viewer's Info mode. (The original "press q on a tiny info screen" papercut was a
  non-issue — minus's static mode short-circuits when content fits — but `--info` shouldn't be
  paginated regardless.)
- B6 — image extras unified behind a single `gather_image_extras(&InputSource, magic_mime)`. New
  `image_decoder_for(&InputSource) -> Option<Box<dyn ImageDecoder>>` branches between
  `ImageReader::open` and `ImageReader::new(Cursor::new(Arc::clone(data)))`; head bytes come from
  `source.open_byte_source()`. The path/bytes split survives only inside `animation::*` for the
  GIF streaming reader.
- B4 — dropped `Registry.forced_language` and `Registry::syntax_token_for`. `text_content_mode`
  passes `args.language.as_deref()` straight to the free `syntax_token_for`; `SyntaxViewer` keeps
  its own copy as before.
- B3 — `Output::new(&Args)` replaced with `Output::new(use_pager: bool)`. `main` computes
  `use_pager = !args.print && stdout().is_terminal()` once and passes it in; `Output` no longer
  re-checks the TTY or `--print`.
- C1 — `format_row` builds one `String` per row instead of ~33. New
  `PeekTheme::paint_into / push_fg / push_reset` write into a caller-owned buffer; new
  `ColorMode::write_fg_seq` formats the SGR escape directly via `write!` so per-byte color sequences
  no longer allocate (TrueColor / Ansi256 / Grayscale / Plain). Ansi16 still returns its lookup
  string. Microbenchmark skipped — the change is mechanical and the alloc-count drop is visible from
  inspection.
- A1 — `ContentMode` raw view streams from a new `LineSource` (one-pass anchor scan over
  `InputSource`, anchors every 1024 lines for O(stride) random access). With a syntax token,
  highlighting goes through a stateful `LineStreamHighlighter` driven forward across the visible
  window; backward scrolls past the cursor reset and replay. Pretty-print branch stays whole-file
  with a 16 MB cap — above the cap a warning lands in `FileInfo.warnings` and the streamed raw view
  takes over. `Mode::render` was replaced by `Mode::render_window(scroll, rows) -> Window`; modes
  pre-slice their output, so `ViewerState` caches the visible window keyed on `(scroll, rows)` and
  `draw_screen` writes the slice verbatim. Pipe path: byte-identical to pre-A1 across all 11
  test-data fixtures × pretty/raw/plain. Deferred sub-items: sparse syntax-state checkpoints (would
  make backward jumps on huge files cheap), streaming pretty-print for JSON/YAML (probably never —
  the cap is the right answer), jump-to-bottom progress hint when syntax replay touches a lot of
  lines.
- A2 — `Viewer` trait, `Registry::viewer_for`, and the six print-mode impls (`SyntaxViewer`,
  `StructuredViewer`, `TextViewer`, `HexViewer`, `ImageViewer`, `SvgViewer`) are gone. New
  `Mode::render_to_pipe(ctx, &mut PrintOutput)` is the print-path entry on every mode (default impl
  materializes `render(ctx)`; `HexMode` streams in 4 KB chunks, `ContentMode` preserves byte-fidelity
  for un-highlighted text). `RenderCtx` carries `term_cols` / `term_rows` so the same `render` body
  serves both interactive and pipe — pipe injects `$COLUMNS-or-80` and `usize::MAX`. `compose_modes`
  is the single dispatcher; `main` picks the first non-aux mode (or first, for binary) for pipe
  output. Pipe smoke-diffs are byte-identical to the pre-change output for source / JSON pretty /
  JSON raw / plain text / hex / plain-binary. Images (PNG, GIF first-frame, SVG raster) now render
  at the correct cells-aspect ratio: the previous path called `crossterm::terminal::size()` which
  fell back to `(80, 24)` whenever stdout was redirected, clamping piped images into 24 rows and
  distorting aspect; the new `usize::MAX` rows in pipe mode lets `contain_size` pick the
  fit-to-width branch and produce aspect-correct output.

Everything below is what's left.

---

## A. Architectural decisions

### A3. Long-line horizontal scrolling

**Severity:** medium for users with real content (long-log JSON, minified source). Terminal wraps
the line, the wrap consumes unbudgeted rows, `draw_screen`'s row math goes out of sync — status bar
can scroll out of view, content can bleed past it.

**Where:**

- `viewer/ui/state.rs:480` — `draw_screen` writes lines verbatim
- `ContentMode` produces width-independent lines

**Why not auto-fixed:** feature, not a bug. Architecture doesn't currently promise horizontal
navigation. A fix changes user expectations.

**Suggested approach:** add `horiz_scroll: usize` per non-owns-scroll mode in `ViewerState`, plus
`Action::ScrollLeft/Right` (`<` / `>` or shift-arrows). Render: slice each line by visible-column
window using `unicode-width`-aware truncation (reuse `truncate_ansi`). Indicate truncation in the
status bar.

**Effort:** ~half a day with key bindings + tests.

---

## B. Real bugs / quirks worth ironing out

### B1. SVG `<animate>` is silently ignored

**Severity:** low — unlikely for typical files, but discovering via "the gif works but my svg
doesn't animate" is a papercut.

**Where:** `viewer/mod.rs:158` — `compose_modes` only calls `decode_anim_frames` for
`FileType::Image`, never `FileType::Svg`. SVG always goes through `ImageRenderMode` (one rasterized
frame).

**Suggested approach:** longer term, rasterize an SVG's animation timeline in `viewer/image/svg.rs`
to produce `Vec<AnimFrame>` and route through `AnimationMode` like GIF/WebP. Short term, at least
add a `FileInfo` warning when the SVG contains `<animate>`/`<animateMotion>` so the user knows the
static render isn't the whole story.

**Effort:** few hours for the warning. Full animation support is a bigger project (resvg's animation
API surface is limited).

---

## How to use this list

Roughly recommended order. A items shape the codebase. B items are pure cleanup that might be
batched. C items are nice-to-haves for a quiet afternoon.

Nothing here is blocking — the codebase passes its tests and clippy clean today.
