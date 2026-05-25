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

### M12. `--plain` mutates `args.color` — wontfix, kept as analysis record

Original finding suggested either (a) dropping `--plain` as "`--color
plain` + a derived tweak" or (b) deriving `plain_mode` from the resolved
`StyleMode`. Both miss the semantic distinction. Three effects of
`--plain` are *not* derivable from `StyleMode::Plain`:

1. Disables structured pretty-print (`viewer/mod.rs:247`:
   `pretty_target = None`). `--color plain` alone still reformats JSON.
2. Skips the syntect pipeline entirely (`syntax_token = None`).
   `--color plain` still parses and styles, just drops the ANSI at
   the encoder.
3. Suppresses rendered views for SVG / HTML / Markdown
   (`svg|html|markdown/compose.rs`). `--color plain` still composes them.

A user passing `--color plain` to get sterile output expects pretty-print
and rendered views to keep working. Conflating the two flags would
break that.

The one real wart is `main.rs:24-25` mutating `args.color` to
`StyleMode::Plain` when `args.plain` is set — the mutation is correct
but hides the user's actual `--color` choice. A cleaner shape would
compute `effective_color = if args.plain { Plain } else { args.color }`
at theme construction without mutating args. Tiny win, not worth a
commit on its own; fold in next time `main.rs` argument plumbing gets
touched.

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

