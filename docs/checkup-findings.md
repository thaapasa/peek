# Checkup findings

IDs are stable. Resolved items are deleted but remaining IDs keep their numbers
so commit / PR references stay valid. Add new IDs at the end of each section
(don't renumber).

## Medium

### M2. `ComposeCtx` paid for by every per-type compose even though most don't use it

14 compose entries, only 4 (HTML, SVG, audio, csv) read `ctx` (for
`text_content_mode`). Other 10 take `_ctx: &ComposeCtx`. Real purpose is
`text_content_mode`; other two fields (`theme_manager`, `theme_name`)
either dead in most composes or available from `Registry`. Either (a) make
`text_content_mode` a free function taking args it actually needs and drop
`ComposeCtx`, or (b) accept it's a real shared bundle and stop apologising
for it across 10 signatures.

### M3. `Registry` carries `plain_mode` only to thread into `ComposeCtx.plain_mode`

Read at exactly two places: `svg/compose.rs:24`, `html/compose.rs:22`. Two
`if !plain_mode` branches deciding whether to push rendered-text view.
Flag could live on `Args` (already does — `args.plain`) and two compose
sites read directly. Then `ComposeCtx` shrinks to
`theme_manager + theme_name`, which collapses into `&Rc<ThemeManager>`
since `theme_name` is `tm.active_theme_name()`.

### M4. `Mode::set_search` return contract is mode-dependent in a way the trait doesn't enforce

`ViewerState::handle_prompt_key` (state.rs:317-333) checks `owns_scroll()`
before using the returned line. If an `owns_scroll` mode returned
`Some(line)`, the line would silently drop. Current `owns_scroll` modes
all return `None`, but the trait doesn't say that. Two opposite return
meanings keyed off a separate trait method is begging for a future bug.

Direction: split into `Search { Owned, Scrolled(usize) }` enum, or have
`owns_scroll` modes declare `fn set_search(…) -> ()` via a default impl,
so the trait signature reflects the contract.

### M6. `gather_extras` is the third major `match file_type` chain

Already exists in `compose_modes` (clean wiring) and `extract::extract`.
Now `info/gather/mod.rs:226-298` adds a third. Each new file type touches
≥5 unrelated dispatch tables across 4 directories (detect, gather,
render, compose, extract). Acceptable today; flag if a 6th wiring point
ever appears — at that count a `trait FileTypeRegistry` registering all
of them at one site beats the explicit dispatches.

Concrete drift already present: `FileType::Compressed(_)` short-circuits
to `binary::info::gather_extras` in `info/gather/mod.rs:293`, but
`info/render/mod.rs:36-99` has no `Compressed` arm in the 20-arm match,
and `compose_modes`' compressed block (`viewer/mod.rs:330-338`) is empty
because the path is reached only after `resolve_transparent` fails. The
inconsistency works today but the four dispatchers no longer enumerate
the same set.

### M7. `ContentMode::scroll` empty-doc action set duplicates the wrap-scroll match

`content.rs:604-619` short-circuits an empty-doc by matching against
`ScrollUp | ScrollDown | PageUp | PageDown | Top | Bottom | ScrollLeft | ScrollRight`.
The `match action` below (lines 623-640) handles the same set. Adding a
new scroll action means editing both places or seeing the empty-doc path
silently fall through.

Fix: drop the empty-doc guard and `return false` from inside each arm
when `cl.total() == 0`, or extract an `Action::is_scroll()` helper.

### M8. `viewer/mod.rs` (639 lines) has accreted unrelated concerns

Holds Registry + ComposeCtx (dispatch wiring), `LineStreamHighlighter`
(syntax line feeder), syntax-token resolution, fallback extension table,
content escape-ranges walker. Three independent abstractions in one file.

Direction: split highlighter + token resolution into `viewer/highlight.rs`;
keep Registry + ComposeCtx + compose dispatch in `mod.rs`.

### M9. `PagedImageMode::render_to_pipe` and `EpubReadMode::render_to_pipe` byte-identical

`viewer/paged.rs:297-313` and `types/ebook/epub/read_mode.rs:226-242`.
Both walk total items, save/restore current, call `ensure_rendered()`,
write lines, insert blank between pages. The chapter renderer correctly
isn't a `PagedImageMode<EpubChapterRenderer>` (cache strategy diverges,
documented), but the pipe walk doesn't depend on cache strategy. Lift
`pipe_walk_pages(&mut self, ctx, out, render_page_fn)` into `paged.rs`
and call from both sites.

### M10. `ContentMode` is 744 lines, past the conventions refactor signal

`viewer/modes/content.rs` already shed `content_rendering`, `content_pipe`,
`pretty_view`, `gutter`, `wrap_scroll`. What's left is five concerns:
window prepare (raw catch-up + pretty cache refresh), visual-row emission
with overlay/wrap, search wiring, `Mode` impl, keyboard/state plumbing.
The window-prepare + emit pair (`prepare_window`, `emit_window`,
`emit_visual_rows`, `usable_width`) is its own concern — a
`WindowRenderer` taking `(rendering, line_source, highlighter, gutter,
wrap, search)` borrows would let `ContentMode`'s impl fit on screen.
Current shape leaks `wrap`'s clamp invariants into every render path
(`clamp_top()` called twice per render).

### M12. `--plain` mutates `args.color` instead of being its own intent

`main.rs:24-25`: `if args.plain { args.color = StyleMode::Plain; }`.
Downstream code reads `args.plain` independently of `args.color`:
`viewer/mod.rs:392,404,423,433` thread `args.plain` into
`ComposeCtx.plain_mode`; `text_content_mode` branches on `plain_mode` to
disable syntax tokens. Two sources of truth for the same intent — an
`info_render` that paints accent-color when `args.plain` is set but
`args.color != Plain` would be a bug; today the code is correct only
because every `paint_*` flows through `StyleMode::Plain`. Either drop
`args.plain` (it's `--color plain` + a derived tweak), or have
`compose_ctx` derive `plain_mode` from the resolved `StyleMode`.

### M13. `gather_code_extras` and `gather_markdown_extras` read the file twice

`info/gather/mod.rs:59-88` (code) and lines around 98-99 (markdown):
`gather_text_stats(source)?` streams the bytes for stats, then
`source.read_text()` re-walks from offset 0 to build the String for the
language-specific gather. For an 8 MB CSS file that's 16 MB of I/O for
one info screen. Either expose `gather_text_stats_with_body` returning
`(TextStats, String)`, or skip the stats pass when the language gather
will read the whole text anyway.

## Low

### L1. `viewer/hex.rs` (primitives) and `viewer/modes/hex.rs` (Mode impl)

Two files for one concept. The four "primitive" functions in `hex.rs` are
called only from the Mode in `modes/hex.rs`. Split is intentional but
hex layout has been stable; folding back into one file would reduce
nothing semantically but shorten a click-through. Net minor.

### L2. `COL_SEP_WIDTH` and its compile-time guard are dead

`csv/table_mode.rs:42-46` defines the constant; `csv/table_mode.rs:942`
has `const _: () = { let _ = COL_SEP_WIDTH; };` to keep it alive. Comment
says "for future overflow math". Drop both; resurrect when needed.

### L3. `CYCLE_BACKGROUND_HELP` etc. spliced into every image-mode's `EXTRA_ACTIONS`

`image/mode.rs:70-77`, `image/animation_mode.rs:32-46`,
`svg/animation_mode.rs:53-67`, `paged.rs:175-183`, `epub/read_mode.rs:41-60`.
Constants are shared (good). Each mode rebuilds the same 4-element block
around them. A `const IMAGE_CONFIG_HELP_BLOCK: &[HelpEntry] = &[…]`
spliced via `concat_arrays!`-style would dedupe — but `&'static [HelpEntry]`
concat in const context is friction. Accept the small dup or add a build
helper.

### L4. `Mode::status_hints(has_return_target)` parameter only read by `HexMode`

Every other Mode ignores it. Move the bool method-side:
`ViewerState::has_return_target_for(mode_id)` and let HexMode call it,
dropping the parameter from the trait. Minor surface-area reduction.

### L6. `directory/mode.rs` opts out of HexMode at the dispatcher

`viewer/mod.rs:348` wraps `HexMode::new` push in
`if !matches!(file_type, FileType::Directory)` with a paragraph comment.
Alternative: `HexMode::new` returns `Err` for sources that can't be
byte-sourced and the dispatcher unconditionally `if let Ok(m) = …`.
Current spot has the comment explaining "why"; relocating doesn't reduce
complexity, just moves it. Note only.

### L11. CsvTableMode `build_header_row` / `build_separator_row` near-copies of `_print` variants

`types/csv/table_mode.rs:367-387` vs `:900-931` (header) and `:390-404`
vs `:933-947` (separator). Core loop identical; the interactive variants
use `.enumerate().skip(self.h_col)`, the print variants use plain
`.enumerate()`. Parameterise via `start_col: usize` and have the two
callers pass `self.h_col` or `0`. Same for separator.

