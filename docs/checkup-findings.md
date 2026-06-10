# Checkup findings

IDs are stable. Resolved items are deleted but remaining IDs keep their numbers
so commit / PR references stay valid. Add new IDs at the end of each section
(don't renumber).

## Medium

### M6. The per-type dispatch hubs are a `match file_type` family — wontfix, kept as analysis record

Original finding: adding a file type touches ≥5 dispatch tables (detect,
gather, render, compose, extract); flag when a 6th wiring point appears,
then a `trait FileTypeRegistry` registering all of them at one site
beats the explicit dispatches.

**Trigger fired, remedy declined.** The `Notebook` type (2026-05-30) was
the 6th-type proof. One new type touched six sites:
`crates/peek-detect/src/detect.rs` (variant + `classify_by_name` + content
sniff), `crates/peek-detect/src/mime.rs` (mime arm + extensions arm),
`src/compose.rs` `compose_modes`, `src/extract/extract.rs`, `src/gather/mod.rs`,
and `crates/peek-foundation/src/info/render/mod.rs` + the gather `Extras`
plumbing. On inspection the registry does not pay:

1. **Detection can't join.** `detect.rs` *produces* the `FileType` from
   magic / extension / content sniff — there is no `FileType` value to
   dispatch on yet. A `FileType → impl` registry can absorb at most
   4–5 of the 6 sites, never detect.
2. **Format sub-enums break a 1:1 type→impl map.** `Archive(fmt)`,
   `Document(fmt)`, `Audio(fmt)`, `Cert(fmt)` dispatch on the inner
   format too; a uniform per-type trait fits these awkwardly.
3. **`render` dispatched on `Extras`, not `FileType`** — a different
   axis. ~~Folding it in needs `gather` to return a boxed trait object
   instead of the `FileExtras` enum, losing that enum's exhaustiveness.~~
   **Superseded 2026-06-05.** This axis *was* converted: `gather` now
   returns `Extras` (`Box<dyn InfoExtras>`) and `info::render` dispatches
   through the trait, per-type impls collected in `types/info_impls.rs`.
   Exhaustiveness was *not* lost — `render_section` is a *required* trait
   method, so a type with no impl fails to compile (the same guarantee
   the enum gave; contra point 4's "silent no-op" worry, which applies to
   *defaulted* methods, not this one). Done to break the `info → types`
   cycle that blocks hoisting `types/` into its own crate. The other axes
   (detect, compose, extract, the `gather` `FileType` hub) still decline
   the registry for the reasons in 1–2 and 4.
4. **The explicit form's assets outweigh the typing cost.** The
   non-exhaustive `match` in `compose`/`extract`/`gather` is
   compiler-enforced completeness — the `extract.rs` arm was *forced* by
   a compile error during the notebook work, not forgotten. A trait with
   defaulted methods would silently no-op instead. `compose_modes` also
   stays a single-file overview of the whole dispatch table; a wide
   trait with no-op defaults (most types' extract = `Unsupported`)
   trades that map for scattered impls + click-through.

The cost the finding feared (touch 6 sites) is mechanical and
compiler-guided — low cognitive load. The registry would *add* load.
Revisit only if a dispatcher can silently fall through and ship a bug;
today the compiler blocks that on `compose`/`extract`/`gather`, and a
missing `mime`/`render` arm surfaces as a visible `?` / absent Info
section, not a crash.

**On the cited "drift" — not a defect.** `info/render` needs no
`Compressed` case because `Compressed → gather → binary` produces a
`BinaryInfo` payload, which renders through its `InfoExtras` impl like any
binary. `Compressed` is a transparent-resolve pseudo-type that degrades to
Binary by design; the dispatchers enumerating different `FileType` sets is
correct, not inconsistent.

### M12. `--plain` mutates `args.color` — wontfix, kept as analysis record

Original finding suggested either (a) dropping `--plain` as "`--color
plain` + a derived tweak" or (b) deriving `plain_mode` from the resolved
`StyleMode`. Both miss the semantic distinction. Three effects of
`--plain` are *not* derivable from `StyleMode::Plain`:

1. Disables structured pretty-print (`compose.rs` passes the pretty branch,
   suppressed under plain). `--color plain` alone still reformats JSON.
2. Skips the syntect pipeline entirely
   (`viewer/mod.rs:84-88`: `syntax_token = None` when `plain_mode`).
   `--color plain` still parses and styles, just drops the ANSI at
   the encoder.
3. Suppresses rendered views for SVG / HTML / Markdown
   (`svg|html|markdown/compose.rs`). `--color plain` still composes them.

A user passing `--color plain` to get sterile output expects pretty-print
and rendered views to keep working. Conflating the two flags would
break that.

The one real wart is `src/main.rs:28-29` mutating `args.color` to
`StyleMode::Plain` when `args.plain` is set — the mutation is correct
but hides the user's actual `--color` choice. A cleaner shape would
compute `effective_color = if args.plain { Plain } else { args.color }`
at theme construction without mutating args. Tiny win, not worth a
commit on its own; fold in next time `main.rs` argument plumbing gets
touched.

### M10. `ContentMode` is 736 lines, near the conventions refactor signal

`crates/peek-foundation/src/viewer/modes/content.rs` already shed
`content_rendering`, `content_pipe`, `content_tests`, `pretty_view`,
`gutter`, `wrap_scroll`. What's left is five concerns: window prepare
(raw catch-up + pretty cache refresh, `prepare_window`), visual-row emission
with overlay/wrap (`emit_window`, `emit_visual_rows`, `usable_width`), search
wiring, `Mode` impl, keyboard/state plumbing. The window-prepare + emit pair
is its own concern — a `WindowRenderer` taking `(rendering, line_source,
highlighter, gutter, wrap, search)` borrows would let `ContentMode`'s impl fit
on screen. (The old "`clamp_top` called twice per render" note no longer
holds — a plain `render_window` pass clamps once, at the end; the second clamp
only happens on a scroll/search action that itself re-clamps before rendering.)

### M13. `gather_capped_text` reads the file twice

`crates/peek-types/src/types/text/info_gather.rs:45-53`:
`gather_text_stats(source)?` streams the byte source for stats, then
`source.read_text()?` re-walks from offset 0 to build the String for the
language-specific gather. For an 8 MB CSS file that's 16 MB of I/O for one
info screen. Every caller of `gather_capped_text` inherits the double-read —
code/markdown/sql/css all route through it (e.g.
`markdown/info_gather.rs:20-26`), so the scope is wider than the original
code/markdown-only framing. Either expose `gather_text_stats_with_body`
returning `(TextStats, String)`, or skip the stats pass when the body read
will walk the whole text anyway.

### M15. `InfoMode::render_window` re-builds every styled line per call

`crates/peek-foundation/src/viewer/modes/info.rs:28-34`. Calls
`crate::info::render(...)` to build every themed line from scratch each
invocation — every scroll keystroke re-themes character data. Only Mode that
re-themes on every `render_window` (and `rerender_on_resize` returns true).
`ViewerState` invalidates the view-cache via `f.views[i] = None` on theme
changes, but the underlying styled lines get rebuilt regardless.

As more file types push async warnings (audio, PDF page-extract failures,
CSV malformed counts), the warnings-edit path clears the InfoMode cache and
next view triggers a full re-paint.

Direction: cache keyed by `(PeekThemeName, StyleMode, warnings_len)` —
same shape `RenderedTextMode` already uses.

### M17. `ContentMode::set_search` streams the entire file per query

`crates/peek-foundation/src/viewer/modes/content.rs:708-735` (raw-branch
scan at `722-725`). Each `/`-then-Enter pulls every line through
`self.line_source.iter_all().map(...)` into `SearchState::scan`.
`MAX_MATCHES = 100_000` caps match *storage* but the streaming read isn't
capped — `'scan: for ... break 'scan` in `search::SearchState::scan` exits
only after the match cap, so a zero-hit query on a 1 GB log walks the whole
file every search. Violates the spirit of "stream, don't load" even while
streaming — the cost is paid per query, not per session.

Two directions:

- Cheap: cap by *bytes scanned* in addition to match count, so a no-match
  search on a multi-GB file degrades cleanly with a status warning.
- Invasive: move scanning to a background thread that streams matches in.

### M18. `Mode::render_window`'s `_scroll` parameter is dead for owns-scroll modes

Trait signature at `crates/peek-foundation/src/viewer/modes/mod.rs:228`;
`owns_scroll` default `false` at `mod.rs:265-267`. Six modes return
`owns_scroll() = true` and ignore the caller's `scroll`: ContentMode
(`content.rs:596`), HexMode (`hex.rs:96`), TableMode (`table/mode.rs:213`),
RowsTableMode (`table/rows_mode.rs:739`), PagedImageMode (`paged.rs:432`),
ListingMode (`listing/mode.rs:317`). Trait surface still passes `_scroll`;
readers must learn that `owns_scroll`-true modes silently discard it and use
their internal scroll instead.

Two reasonable directions:

- Split `Mode` into `ScrolledMode` / `OwnsScrollMode`, drop the dead
  parameter, add trait-discrimination in `ViewerState`.
- Keep as-is, document the contract more loudly on the trait.

Not urgent. Flag as refactor candidate — every new owns-scroll mode adds
another `_scroll` underscore.

### M20. architecture.md no longer describes the session layer it documents

`docs/architecture.md` has fallen behind the two biggest changes to the
interactive core:

- The **ViewerState section** (line ~222) still describes a
  single-session controller ("mode list, active index, last_primary slot,
  per-mode scroll offsets…"). The recursive-peek `SessionFrame` stack —
  `descend` / `build_descend_frame` / in-frame `select_jump`, breadcrumb,
  `MAX_STACK_DEPTH`, dir→dir frame collapse
  (`src/viewer_session/state.rs:66-137, 540-665`) — is now the
  centrepiece of the session layer and appears nowhere in the design doc
  (features.md and the manual cover it; the builder-facing doc doesn't).
- The **Mode trait snippet** (lines 141-171) is missing the whole
  descend/jump/extract surface added since: `extract_target`,
  `select_jump`, `build_descend_frame`, `jump_position`,
  `position`/`set_position`, `render_flat_to_pipe` — and still shows the
  trait as `pub(crate)` from its pre-workspace-split days.

CLAUDE.md lists architecture.md as must-stay-in-sync; anyone extending
descend behaviour from the doc will design against a contract that no
longer exists. Fix: refresh the trait snippet from
`crates/peek-foundation/src/viewer/modes/mod.rs:209-420` and add a short
"Session stack / recursive peek" subsection to the ViewerState part.

### M21. `viewer/ui/mod.rs` breaks the project's own "mod.rs stays small" rule

`crates/peek-foundation/src/viewer/ui/mod.rs` (852 lines, ~540 non-test)
is a grab-bag living in a `mod.rs`: alternate-screen lifecycle,
status-line composition, theme construction, terminal-size + test
override, **and** the entire SGR-aware string family (`expand_tabs`,
`strip_ansi_width`, `wrap_styled`, `wrap_styled_words`,
`hard_split_into`, `count_wrap_segments`, `take_cols`, `slice_styled_h`,
`truncate_ansi`). The conventions doc explicitly says mod.rs is
declarations/re-exports only, and the styled-string walkers are one
coherent concern with their own invariants (escape-skipping, wide-char
boundaries, style re-emission). Lift them into `ui/styled.rs` (tests
along). While there: `truncate_ansi` (line 460) is functionally
`slice_styled_h(s, 0, max)` minus the trailing reset and style
normalisation — three separate ANSI-walking truncation loops is one more
than the family needs; folding `truncate_ansi` onto `slice_styled_h` (or
documenting why its no-reset output is required by the status line)
closes the drift window.

### M22. `viewer_session/state.rs` is 4× past the split threshold and holds five concerns

`src/viewer_session/state.rs` (1534 lines, ~1100 non-test) now mixes:
session-stack management (`SessionFrame`, push/pop/collapse),
extract/descend orchestration (`descend`, `push_extracted`,
`push_direct_frame`, `start_extract`), prompt plumbing (`PromptKind`,
`handle_prompt_key`), render-failure recovery (`retry_frame_detection`,
`degrade_active_to_hex`), and caller-side scroll math. The conventions
name this exact signal (~400 lines mixing concerns) and the file already
shed `ScreenBuffer` once for the same reason. The cleanest cut is the one
the file's own section banners suggest: move `SessionFrame` + the
stack/descend/extract block (state.rs:54-137, 509-665 plus the
prompt-confirm dispatch) into a sibling `viewer_session/` module, leaving
`state.rs` as mode dispatch + render cache + drawing. Soft refactor
candidate — the code is healthy, the file is just past the point where a
reader must hold all five models at once.

## Low

### L1. `viewer/hex.rs` (primitives) and `viewer/modes/hex.rs` (Mode impl)

Two files for one concept. The four primitive functions in
`crates/peek-foundation/src/viewer/hex.rs` (`bytes_per_row`, `align_down`,
`max_top`, `format_row`) are called only from the Mode in
`viewer/modes/hex.rs`. Split is intentional but hex layout has been stable;
folding back into one file would reduce nothing semantically but shorten a
click-through. Net minor.

### L3. Image-config help consts re-listed across image modes — largely resolved

The 4-element literal block the finding targeted was extracted into three
named consts (`CYCLE_BACKGROUND_HELP`, `CYCLE_IMAGE_MODE_HELP`,
`CYCLE_FIT_HELP`) at `crates/peek-foundation/src/viewer/paged.rs:192-203`.
What remains is each image mode re-listing those three consts inline in its
`EXTRA_ACTIONS`: `paged.rs:217-219`, `image/mode.rs:79-81`,
`image/animation_mode.rs:56-58`, `svg/animation_mode.rs:77-79`,
`font/specimen_mode.rs:78-80` and `91-93`, and `ebook/epub/read_mode.rs:61-69`
(epub deliberately diverges with "cover image" labels). That's a reference to
shared consts, not a copied array — low value to dedup further. Accept the
small re-list or add a build helper; close if not worth it.

### L4. `Mode::status_hints(has_return_target)` parameter only read by `HexMode`

Trait sig + default at `crates/peek-foundation/src/viewer/modes/mod.rs:300`
(default ignores `_has_return_target`). Only reader: `modes/hex.rs:164`
(returns `x:exit hex`); every other impl ignores it (e.g. `paged.rs:542`,
`ebook/epub/read_mode.rs:295`). Caller threads the bool at
`src/viewer_session/state.rs:240-243`. Move the bool method-side:
`ViewerState::has_return_target_for(mode_id)` and let HexMode call it,
dropping the parameter from the trait. Minor surface-area reduction.

### L7. `viewer/paged.rs` at 735 lines mixes four concerns

`crates/peek-foundation/src/viewer/paged.rs` holds `PageCacheKey` (44),
`CachedRender` (67), `render_cached` (114), `step_paged` (139),
`cycle_image_config` (159), the `PageRenderer` trait (286), the
`PagedImageMode<R>` impl (352-553), plus the in-file `#[cfg(test)] mod tests`
(554-735). Past the conventions ~400-line refactor signal
(`docs/conventions.md`) for mixed-concern files. The multi-page CBZ
regression tests want to live next to the real renderer rather than the
generic shell.

Direction: lift `PagedImageMode<R>` + its tests into `viewer/paged/mode.rs`;
keep `paged/mod.rs` as primitives (`PageCacheKey`, `render_cached`,
`step_paged`, `pipe_walk_pages`, `cycle_image_config`, help constants,
`PageRenderer` trait).

### L9. `Action::ZoomPreset(n)` help/handler pin test missing

`crates/peek-foundation/src/viewer/paged.rs:593` has
`image_config_help_pinned_to_handler` pinning the `CYCLE_*_HELP` rows to
`cycle_image_config`. Same gap exists for the `Zoom 1×-9×` help row in every
image mode's `EXTRA_ACTIONS` and the `Action::ZoomPreset(n)` bindings (handled
on a different path, `image_render/zoom_pan.rs:79`) — adding `ZoomPreset(10)`
to `bindings()` while forgetting the help row would slip through. Pattern's
good; needs one more application.

### L13. 16 MB render cap also bounds in-container image payloads

`RENDER_MAX_BYTES` was sized for text payloads ("a 16 MB
`document.xml`"), but the shared `read_zip_entry` gate now also bounds
CBZ page and EPUB image reads
(`crates/peek-types/src/types/comic/cbz/package.rs:67`,
`crates/peek-types/src/types/ebook/epub/read_mode.rs:562`). A legitimate
>16 MB archival scan refuses to render inside the container while the
identical file opened standalone renders fine (the image type reads
`read_bytes()` uncapped). Not a defect — the safety rationale
(alloc-abort is uncatchable, unlike a render `Err`) holds for images
too, the degrade path is soft (warning line; TOC / Info / hex / extract
all still work), and typical pages run 1–5 MB. Act only if a real
oversize page/image surfaces; the fix is a second, larger image-payload
cap passed into `read_zip_entry` per call — not gate removal.
