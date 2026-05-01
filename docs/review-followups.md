# Review follow-ups (2026-04-26)

This document captures items from a thorough architectural review that
were **not** fixed in the same pass — either because they are
architectural decisions you should make, feature work rather than
bug-fixing, or low-impact polish. Each item lists the symptom, where it
lives, why it wasn't auto-applied, and a sketch of how to address it so
you can decide what to take on next.

For context: the review's clear-cut bugs and small cleanups were landed
in the same change-set:

- `with_alternate_screen` panic-safety via `Drop` guard
- `Mode::handle` → `Handled` enum so ContentMode pretty/raw can reset
  scroll
- Env-var test refactored to a pure helper (no `unsafe set_var`)
- `syntax_token_for` deduped between `Registry` and `SyntaxViewer`
- Animation render path now reuses `render::render_decoded`
- `gather_text_extras` streams via `open_byte_source` instead of
  loading the full file
- `Mode::is_aux()` and `Mode::status_hints()` replace hardcoded
  `ModeId` matches in `ViewerState` / `interactive`
- B5 — `compose_status_line` / `strip_ansi_width` / `truncate_ansi` now
  covered by unit tests in `viewer/ui/mod.rs` (fits, hints-truncated,
  left-truncated, CJK widths, trailing-escape preservation)
- C3 — `format_unix_permissions` now emits `ls -l`-style output: type
  prefix (`-` / `d` / `l` / `b` / `c` / `p` / `s`) and setuid/setgid/
  sticky overlays (`s` / `S` / `t` / `T`). `paint_permissions`
  accommodates the new chars and 10-char layout.
- C2 — `decode_anim_frames` and `anim_frame_count` now accept the
  upstream `magic_mime`; they short-circuit to the matching `AnimFormat`
  when it's `image/gif` / `image/webp`, skipping the redundant
  extension/sniff path used pre-fix.
- B2 — `--info` is now an unconditionally non-paginated path. New
  `Output::direct()` constructor; main's `--info` branch uses it.
  Architectural framing: `--info` is a fixed-size summary; if you
  want to scroll it, use the interactive viewer's Info mode. (The
  original "press q on a tiny info screen" papercut was a non-issue
  — minus's static-mode short-circuits when content fits — but
  `--info` shouldn't be paginated regardless.)

Everything below is what's left.

---

## A. Architectural decisions

### A1. Stream the text, syntax, and structured viewers

**Severity:** medium-high — the architecture doc's "Stream, don't load"
north star is currently aspirational for any text path. Real-world impact
is hitting OOM or long startup on multi-GB log files.

**Where:**
- `viewer/mod.rs:215` — `text_content_mode` calls `source.read_text()`
- `viewer/syntax.rs:55` — `SyntaxViewer::render` calls `source.read_text()`
- `viewer/structured.rs:37` — `StructuredViewer::render` calls
  `source.read_text()`
- `viewer/text.rs:19` — `TextViewer::render` calls `source.read_text()`
- (`gather_text_extras` was already converted in this pass)

**Why not auto-fixed:** this is a real refactor, not a tweak. It
touches:

- `syntect::HighlightLines` is line-stateful (each line's highlight
  depends on the previous line's parse state), so to stream
  highlighting you keep the highlighter live and feed it lines as they
  arrive. Not hard, but it changes the shape of `highlight_lines` and
  the `Vec<String>` line cache that `ViewerState` already builds for
  every mode.
- For `ContentMode`, the cache strategy needs to become windowed — keep
  N lines around the current scroll offset, reload on big jumps. That's
  a real design decision (LRU? strict window? on-disk index?).
- Pretty-printing structured data legitimately needs the whole document
  (you can't pretty-print streaming JSON without buffering anyway), so
  `StructuredViewer` and ContentMode's pretty branch should stay
  whole-file but with an explicit size cap + "show raw with warning"
  fallback for huge files.

**Suggested approach (rough):**

1. Tighten `highlight_lines` to take an iterator of lines and yield an
   iterator of highlighted lines. Keep the existing whole-string call
   site as a thin wrapper for now.
2. Introduce a `LineSource` abstraction on top of `InputSource` that
   yields lines in 64 KB chunks (UTF-8 boundaries handled like
   `gather_text_extras_streaming` does today).
3. Replace `ContentMode`'s `raw: String` with a `LineSource` plus a
   bounded line cache. Total scroll/length comes from a one-pass line
   count (which can also live in `gather_text_extras_streaming`'s
   already-running scan).
4. Either keep `StructuredViewer` whole-file (with a documented size
   cap and graceful fallback to raw streaming highlight on overflow)
   or, longer-term, introduce a streaming pretty-printer for JSON/YAML
   only.

**Effort:** ~1–2 days of careful work. Touches the line-cache layer
that both interactive and piped paths share, so plenty of testing
needed.

---

### A2. Unify the `Viewer` (piped) and `Mode` (interactive) systems

**Severity:** medium — the duplication shows up as subtle drift
hazards. Two pipelines means two ways to remember to update the
syntax-token logic, the warning behavior, the pretty-print fallback,
etc. Most of the easy duplication has been deduped (see
`syntax_token_for`, `render_decoded`, the structured `pretty_print`
sharing), but `viewer_for` and `compose_modes` are still two parallel
dispatchers.

**Where:**
- `viewer/mod.rs:113` — `Registry::viewer_for(file_type)`
- `viewer/mod.rs:137` — `Registry::compose_modes(...)`

**Why not auto-fixed:** there's a genuine design choice here. Two
shapes are reasonable:

- **Option α — drop `Viewer`:** the piped path renders the first
  non-aux mode's `render()` output to stdout once. Pros: one
  dispatcher, one render contract. Cons: modes were designed for
  interactive use (terminal-size-aware sizing, colored escapes for a
  TTY), and you'd need to teach them how to size for pipe output.
- **Option β — keep `Viewer` but generate it from the mode:** add a
  `Mode::render_for_pipe(&self, output) -> Result<()>` default method
  that delegates to `render()`. Easier path, less benefit.

**Suggested approach:** start by collapsing the dispatch — make
`viewer_for` itself call `compose_modes` and pluck the first non-aux
mode. Inside that mode's `render()`, decide based on a `for_pipe: bool`
flag (or a separate `RenderCtx::pipe`) whether to size to terminal,
emit colors, etc. Whole change is mostly mechanical; the only
genuinely new code is figuring out what `term.rows`/`term.cols` should
mean in pipe mode (probably: rows = unbounded, cols = `$COLUMNS` or
default).

**Effort:** ~half a day if Option α is chosen and you accept the
"render once and output" shape. Most modes already work in piped
contexts because they produce ANSI strings either way.

---

### A3. Long-line horizontal scrolling in the interactive viewer

**Severity:** medium for users whose content is real (long-log JSON,
minified source). The terminal wraps the line, the wrap consumes
unbudgeted rows, and `draw_screen`'s row math goes out of sync — the
status bar can scroll up out of view, or content bleeds past it.

**Where:**
- `viewer/ui/state.rs:480` — `draw_screen` writes lines verbatim
- `ContentMode` produces width-independent lines

**Why not auto-fixed:** this is a feature, not a bug. The architecture
doesn't currently promise horizontal navigation. A fix changes user
expectations.

**Suggested approach:** add a `horiz_scroll: usize` per non-owns-scroll
mode in `ViewerState`, plus `Action::ScrollLeft/Right` (e.g. `<` / `>`
or shift-arrows). When rendering, slice each line by visible-column
window using `unicode-width`-aware truncation (reuse
`truncate_ansi`). Indicate truncation in the status bar.

**Effort:** ~half a day including key bindings and tests.

---

## B. Real bugs / quirks worth ironing out

### B1. SVG `<animate>` is silently ignored

**Severity:** low — unlikely to surface for typical files, but
discovering it via "the gif works but my svg doesn't animate" is a
papercut.

**Where:** `viewer/mod.rs:158` — `compose_modes` only calls
`decode_anim_frames` for `FileType::Image`, never `FileType::Svg`. SVG
files always go through `ImageRenderMode` (one rasterized frame).

**Suggested approach:** longer-term, rasterize an SVG's animation
timeline in `viewer/image/svg.rs` to produce `Vec<AnimFrame>` and route
through `AnimationMode` like GIF/WebP. Short-term, at least add a
`FileInfo` warning when the SVG contains `<animate>`/`<animateMotion>`
so the user knows the static render isn't the whole story.

**Effort:** few hours for the warning; full animation support is a
bigger project (resvg's animation API surface is limited).

---

### B3. `is_terminal()` is checked twice in two locations

**Severity:** trivial duplication. Currently:
- `main.rs:31` — `is_tty = stdout().is_terminal()` for the
  `use_pager` flag
- `output/pager.rs:16` — `Output::new` recomputes `use_pager`

**Suggested approach:** pass the decision in. `Output::new(args, use_pager)`
or `Output::Direct` / `Output::Pager` constructors that don't
re-decide. Falls naturally out of B2 if you do that.

**Effort:** ~10 minutes.

---

### B4. `Registry` still stores `forced_language` after the dedup

**Severity:** trivial — the field is only read by
`Registry::syntax_token_for`, which now just delegates to the free
function `syntax_token_for(forced_language, source, file_type)`. The
same value also lives in `SyntaxViewer.forced_language`.

**Where:** `viewer/mod.rs:74,98,269`

**Suggested approach:** drop `Registry.forced_language` entirely.
`compose_modes` already has access to `&Args`, so the call site can
pass `args.language.as_deref()` directly to `syntax_token_for`.

**Effort:** ~5 minutes.

---

### B6. `gather_extras` for images still has separate path / bytes
variants

**Severity:** trivial — same shape as the text extras code that just
got unified, but for the `FileType::Image` branch the path
(`gather_image_extras`) and stdin (`gather_image_extras_from_bytes`)
still split.

**Where:** `info/gather.rs:142-199`

**Suggested approach:** factor an `image_decoder_for(source)` that
returns `Box<dyn ImageDecoder>` for either an `InputSource::File` (via
`ImageReader::open`) or `InputSource::Stdin` (via
`ImageReader::new(Cursor::new(...))`). Then both extras-builders
collapse into one.

**Effort:** ~20 minutes.

---

## C. Polish / micro-perf

### C1. `paint()` allocates per call; hot in hex rendering

**Severity:** minor — `format_row` does ~33 `String` allocations per
row (`paint(offset)` + per-byte hex `paint` + per-byte ascii `paint`),
times ~24 rows = ~800 allocations per redraw. A redraw on every
keystroke. Probably invisible in profile but worth knowing about.

**Where:** `viewer/hex.rs:115-155`, `theme.rs:167-184`

**Suggested approach:** add `theme.paint_into(buf: &mut String, text:
&str, color: Color)` that writes ANSI escape + text + reset directly
into a caller-owned buffer. `format_row` then builds one `String` per
row.

**Effort:** ~30 minutes if you also want a microbenchmark.

---

## How to use this list

These are listed in roughly recommended order — A items shape the
codebase, B items are pure cleanup that might be batched together, C
items are nice-to-haves you can drop into a quiet afternoon.

Nothing in this list is blocking. The codebase passes its tests and
clippy clean today.
