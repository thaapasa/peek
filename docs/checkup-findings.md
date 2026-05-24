# Checkup findings

IDs are stable. Resolved items are deleted but remaining IDs keep their numbers
so commit / PR references stay valid. Add new IDs at the end of each section
(don't renumber).

## High

### H2. Search `handle` arm bodies still duplicated across 6 modes

Data-layer dedup is done (`SearchState`, `step_search`, `overlay_matches`,
`reveal_h_scroll` all shared). What's left is per-mode `Mode::handle`
ceremony — same three arms in:

- `ContentMode` (content.rs:552-598)
- `ListingMode` (listing/mode.rs:359-397)
- `TableMode` (table/mode.rs:247-296)
- `CsvTableMode` (csv/table_mode.rs:807-887)
- `EpubReadMode` (epub/read_mode.rs:237-271)
- `RenderedTextMode` (rendered_text.rs:145-155)

Pattern is byte-identical: `Back if self.search.is_some() => clear`,
`NextMatch => step`, `PrevMatch => step`. Same for the
`status_segments` search-segment push. `set_search` itself stays
per-mode (scan source differs: `LineSource` / `Vec<String>` / row vec /
record stream).

Direction: `search_handle(action, &mut self.search) -> Option<Handled>`
+ `search_status_segment(&self.search, theme)` helpers. Each mode's
trait body loses ~10 lines and the pattern is documented once.

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

### L5. `docs/features.md` missing whole file-type sections

Not just a stale TOC — the body is also missing top-level sections for
shipped types. Section list (grep `^### ` between *Supported File
Types* and *Viewer Features*): Source Code, Structured Data, Image,
Audio, Animated Images, Object Files, Java Classfiles, Binary /
Archive. No section for PDF, EPUB, CSV, Document (DOCX/ODT/RTF), HTML,
Markdown, CSS, SQL, Disk Image, Comic (CBZ). Either the doc's scope
shrank without an explicit decision, or sections need adding.

### L6. `directory/mode.rs` opts out of HexMode at the dispatcher

`viewer/mod.rs:348` wraps `HexMode::new` push in
`if !matches!(file_type, FileType::Directory)` with a paragraph comment.
Alternative: `HexMode::new` returns `Err` for sources that can't be
byte-sourced and the dispatcher unconditionally `if let Ok(m) = …`.
Current spot has the comment explaining "why"; relocating doesn't reduce
complexity, just moves it. Note only.
