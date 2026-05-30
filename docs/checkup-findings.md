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

### M15. `InfoMode::render_window` re-builds every styled line per call

`src/viewer/modes/info.rs:22-27`. Calls `crate::info::render(...)` to
build every themed line from scratch each invocation — every scroll
keystroke re-themes character data. Only Mode that re-themes on every
`render_window`. `ViewerState` invalidates the view-cache via
`f.views[i] = None` on theme changes, but the underlying styled lines
get rebuilt regardless.

As more file types push async warnings (audio, PDF page-extract failures,
CSV malformed counts), the `state.rs:851-856` warnings-edit path clears
the InfoMode cache and next view triggers a full re-paint.

Direction: cache keyed by `(PeekThemeName, StyleMode, warnings_len)` —
same shape `RenderedTextMode` already uses.

### M16. `Action::{Next,Prev}{Frame,Chapter,Face,Match}` six-variant family leaks mechanism

`src/viewer/ui/keys.rs:131-148` and `src/viewer/ui/state.rs:485-510`.
Six variants all bind to `n`/`p`; every consumer matches exactly the one
variant it cares about; the giant `Action::Next* | ...` arm in
`state.rs::apply` lists them only so exhaustiveness fires.

The `keys.rs:120-130` comment defends the split on "semantic clarity at
the call site". Reasonable when there were two pairs. With six (NextFace
added in font work), the marginal cost is: every new `n`/`p` consumer
needs two Action variants, two `bindings()` arms, two more
fallthrough-list entries, and the mode still does a one-line match.

Direction: collapse to one `Action::Next` / `Action::Prev` pair. Each
consumer's `handle()` matches one variant. Help text per-mode already
names the stepped thing ("Next / previous chapter"), so semantic clarity
lives at the help layer not the action layer.

Counter (existing comment's defence): a single `Next` loses "skim
`match action` and see what the mode does on `n`". True for a reader
scanning the global match, but the mode's `handle` already has *one*
`Action::Next` arm — its body names what's stepped (`self.anim.step(...)`
/ `step_paged(...)` / `step_search(...)`). Clarity loss is small.

### M17. `ContentMode::set_search` streams the entire file per query

`src/viewer/modes/content.rs:716-725`. Each `/`-then-Enter pulls every
line through `LineSource::iter_all().map(...)` into `SearchState::scan`.
`MAX_MATCHES = 100_000` caps match storage but the streaming read isn't
capped — `'scan: for ... break 'scan` in `search::SearchState::scan`
exits only after the match cap, so a zero-hit query on a 1 GB log walks
the whole file every search. Violates the spirit of "stream, don't load"
even while streaming — the cost is paid per query, not per session.

Two directions:
- Cheap: cap by *bytes scanned* in addition to match count, so a no-match
  search on a multi-GB file degrades cleanly with a status warning.
- Invasive: move scanning to a background thread that streams matches in.

### M18. `Mode::render_window`'s `_scroll` parameter is dead for half the modes

`src/viewer/modes/content.rs:452,599` declares `owns_scroll() = true` and
ignores caller's `scroll`. Same pattern in `HexMode`, `TableMode`,
`RowsTableMode`, `PagedImageMode`, `ListingMode`. Trait surface still
passes `_scroll`; readers must learn that `owns_scroll`-true modes
silently discard it and use their internal scroll instead.

Two reasonable directions:
- Split `Mode` into `ScrolledMode` / `OwnsScrollMode`, drop the dead
  parameter, add trait-discrimination in `ViewerState`.
- Keep as-is, document the contract more loudly on the trait.

Not urgent. Flag as refactor candidate — every new owns-scroll mode adds
another `_scroll` underscore.

### M19. `state.rs::apply`'s 25-arm mode-local fall-through is ceremony

`src/viewer/ui/state.rs:485-510`. Lists every mode-local action
explicitly so non-exhaustive-match flags new variants. Works, but list
hit 25 entries; every new action adds a line in a file that does nothing
with it.

Direction: small `Action::category()` (or `is_mode_local()`) on the enum
itself plus one catch-all arm. Compiler still forces new variants to be
categorised — author declares the category at the enum site instead of
the global dispatcher. Not a bug; arm-count smell as action set grows.

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

### L7. `viewer/paged.rs` at 919 lines mixes four concerns

Holds `PageCacheKey`, `CachedRender`, `render_cached`, `step_paged`,
`cycle_image_config`, the `PageRenderer` trait, `PagedImageMode<R>` impl,
plus three integration tests. Past the conventions ~400-line refactor
signal (`docs/conventions.md:84`) for mixed-concern files. The
multi-page CBZ regression tests (lines 580-918) want to live next to
the real renderer rather than the generic shell.

Direction: lift `PagedImageMode<R>` + its tests into `viewer/paged/mode.rs`;
keep `paged/mod.rs` as primitives (`PageCacheKey`, `render_cached`,
`step_paged`, `pipe_walk_pages`, `cycle_image_config`, help constants,
`PageRenderer` trait).

### L9. `Action::ZoomPreset(n)` help/handler pin test missing

`src/viewer/paged.rs:620-644` has `image_config_help_pinned_to_handler`
pinning the `CYCLE_*_HELP` rows to `cycle_image_config`. Same gap exists
for the `Zoom 1×-9×` help row in every image mode's `EXTRA_ACTIONS` and
the `Action::ZoomPreset(n)` bindings — adding `ZoomPreset(10)` to
`bindings()` while forgetting the help row would slip through. Pattern's
good; needs one more application.

