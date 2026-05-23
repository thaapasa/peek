# Checkup findings

Codebase review notes from `/checkup` run on 2026-05-23. Architecture has good
bones — `compose_modes` is a real dispatch table, `InputSource::ByteSource`
streams, `Mode` trait carries `render_window` + `render_to_pipe` from one place.
Recent commits (paged-image unification, content-mode config struct, RenderCtx
dedup) show ongoing cleanup. Below: where parallel structure accreted real
duplication, plus abstractions earning less than they cost.

Finding IDs: `T1`–`T5` (top), `M1`–`M10` (medium), `L1`–`L7` (low).

## Top 5 highest-leverage findings

### T1. `DirectoryMode` and `ListingMode` are two implementations of the same view

`viewer/listing/mode.rs:554-637` vs `types/directory/mode.rs:295-417`. Separate
row state, separate viewport, separate `paint_perms` / `size_color` /
`paint_size`. ~400 lines duplicated formatting and one extra Mode to maintain.

Sketch: extend `ListingMode` with a "flat one-level entry list" construction
mode (no tree connectors, no sticky parents, `..` allowed as parent row). Drop
`DirectoryMode`.

### T2. Three near-identical image read modes

- `ImageRenderMode` — `types/image/mode.rs`
- `AnimationMode` — `types/image/animation_mode.rs`
- `SvgAnimationMode` — `types/svg/animation_mode.rs`

All share: `prepare → max_scroll → GridWindow → render_prepared` shape,
saved-fit pipe path, `cycle_image_config` wiring, status/extract pattern.
Differences: source of `PreparedImage` (raster / decoded-frame / lazily
rasterized from keyframes) + whether to cache.

Same shape `PagedImageMode<R>` already solved for PDF/CBZ. Generalize to
`ImageMode<R>` over an `ImagePreparer` trait. Today's `PagedImageMode<R>`
becomes one variant of that, not its own parallel structure.

### T3. DOCX/ODT + RTF `render.rs` duplicate word-wrap + SGR bracketing

`types/document/render.rs:159-243` and `types/document/rtf/render.rs:108-130`
have byte-identical `split_words` and `visible_width`. SGR open/close
bracketing in `emit_run` vs `emit` is parallel-structure.

AST shapes are real (Doc-with-Runs vs flat painter-tagged stream) but wrap
engine is one concern. Extract generic `WrapEmitter` taking a stream of styled
tokens.

### T4. ZIP-TOC compose dance copied across 4 types

EPUB, CBZ, DOCX, ODT each call
`archive::reader::list_entries(source, ArchiveFormat::Zip)` and convert
`Result` into entries + warnings + `ListingMode` push.

- `types/ebook/compose.rs:30-36`
- `types/comic/compose.rs:33-39`
- `types/document/compose.rs:57-63`

Three copies of `(label, Err(e) → format!("Failed to list {label}: {e:#}")`.
Add `viewer::listing::push_zip_toc(modes, source, label)` helper.

### T5. `Registry::compose_modes` has an inline arm

Every other arm delegates to `types::<x>::compose::compose`, but
`FileType::SourceCode | FileType::Structured` is inlined as
`modes.push(ctx.text_content_mode(...))`. Inconsistency leaks the convention
rule. Either give source/structured their own `types::source/compose.rs` for
uniformity, or invert the model: every type starts with whatever its compose
says and `compose_modes` always calls per-type entry. The two
structured-vs-source-code paths could share the same `text_content_mode` call
inside their composes.

## High

(Empty — none of above are correctness bugs; architecture / duplication.)

## Medium

### M1. `ListingSearch` duplicates `SearchState`

`viewer/listing/mode.rs:62-66` vs `viewer/search.rs:200-203`. Shape —
`matches: Vec<...>, cursor: usize` + `step_match(delta)` helper — identical;
only row vs (line, range) granularity differs. `SearchState` already handles
multiple ranges per line; reshape `ListingMode` to scan leaf text into a
`SearchState` keyed on row index. Today every searchable mode either reuses
`SearchState` (Content, RenderedText, EPUB, Table) or rolls its own (Listing)
— keep count at zero.

### M2. `ComposeCtx` paid for by every per-type compose even though most don't use it

14 compose entries, only 4 (HTML, SVG, audio, csv) read `ctx` (for
`text_content_mode`). Other 10 take `_ctx: &ComposeCtx`. Real purpose is
`text_content_mode`; other two fields (`theme_manager`, `theme_name`) either
dead in most composes or available from `Registry`. Either (a) make
`text_content_mode` a free function taking args it actually needs and drop
`ComposeCtx`, or (b) accept it's a real shared bundle and stop apologising
for it across 10 signatures.

### M3. `Registry` carries `plain_mode` only to thread into `ComposeCtx.plain_mode`

Read at exactly two places: `svg/compose.rs:24`, `html/compose.rs:22`. Two
`if !plain_mode` branches deciding whether to push rendered-text view. Flag
could live on `Args` (already does — `args.plain`) and two compose sites
read directly. Then `ComposeCtx` shrinks to `theme_manager + theme_name`,
which collapses into `&Rc<ThemeManager>` since `theme_name` is
`tm.active_theme_name()`.

### M4. `ContentMode` is 1275 lines, 17 fields

Mixes streaming raw + pretty branch + wrap geometry + gutter + search +
h-scroll reveal. Recent commits already split `PrettyView`, `Gutter`,
`WrapScroll` out — good. What's left: branch selection (raw/pretty toggle +
`ContentLines`), highlighter caretaking (`LineStreamHighlighter` catch-up +
reset), search reveal (`reveal_match_h` + `step_match`), pipe-mode rendering
with all branches (highlight + gutter + trailing-newline fidelity), key
handling. Pipe-mode body alone is 80 lines with three nested branches —
could be hoisted into `content::pipe::render` free fn taking line source /
pretty branch / gutter / highlighter so test surface is smaller and trait
impl is just orchestration.

### M5. `LineProvider` trait has one production impl

`viewer/wrap_scroll.rs:32`. `ContentLines` in content.rs is the only real
impl, one test impl beyond that. Trait exists to dodge a borrow-checker
pattern, not to support polymorphism — `ContentMode` borrows `wrap` mut while
`ContentLines` borrows `line_source` + `pretty` shared. Reader has to track
abstraction whose only function is borrow-splitting. Inline `ContentLines`
into `WrapScroll`'s API by passing active branch (`Either<&LineSource,
&[String]>` or just two `WrapScroll::with_raw` / `with_pretty` entry points)
and drop trait. Cost of trait > benefit; abstraction's "concept isn't real
yet."

### M6. `PageCacheKey` overloaded with image-config fields not always relevant

`pdf::page_renderer`, `cbz::page_renderer`, `epub::read_mode` all build same
`PageCacheKey` from `ImageConfig` (`viewer/paged.rs:40-50`).
`PageCacheKey::build` exists and is reused — good. But
`EpubReadMode::ensure_rendered` *also* keys on style_mode but doesn't honor
`image_mode` / `background` for non-cover chapters (those keys still
invalidate, just wastefully). Consider letting cache key be a renderer
concern (each `PageRenderer` provides `cache_key_extra()`).

### M7. `audio::package::probe` re-called from three sites

`compose.rs:29`, `info_gather.rs`, `extract.rs`. Comment says "Re-probes per
call (header + tag walk, ms-cheap)" — accepted. But new "stream through
seeking File" optimisation (commit 1343684) doesn't apply uniformly:
extracting an audio embed re-probes through `Memory`, fine; but `info_gather`
runs twice in a single render path (once from main's `--info` if used, once
from interactive's first `Info` render via `ViewerState::new`). Worth a
single `Arc<Probed>` snapshot via `ComposeCtx` if revisited. Today:
acceptable; note "defer until profiling shows a hot path" comment ~50 lines
older than its claim.

### M8. `pretty_view::ensure_parsed` reads entire source for pretty-print

`pretty_view.rs:69-113`. `source.read_text()` slurps. Documented (16 MB cap),
but cap check is `total_bytes > PRETTY_MAX_BYTES` *before* read, so cap
honored. Fine. What's *not* fine: when `read_text` itself fails on a 200 MB
JSON warning says "read failed for pretty-print", but for `Memory` /
`Bytes`-backed sources the read can't fail. Error string lies about what
happened. Tighten to "decode failed" since read fails on UTF-8, not I/O.

### M9. `InputSource::path` confusable with `name`

`source.rs:129-150`. Four variants but `name`/`path` duplicated. `path()`
returns `None` for everything but `File`. Several call sites do
`source.path().and_then(|p| p.file_name())` instead of `source.name()` —
confusable. Drop `path()` from public API or rename to `disk_path()` to make
clear it's literal on-disk file (not user-visible source name). Most callers
want `name()`.

### M10. `docs/features.md` at 1014 lines

Warning sign conventions calls out (~400 lines = refactor signal). Reading
cover-to-cover is only way to find a specific feature. Either break into
per-type pages (mirroring `src/types/<x>/`) or accept it's a reference, not a
doc — add TOC at top.

## Low

### L1. `fallback_syntax_token` arms

`viewer/mod.rs:545-555`. Commented-out file extensions instead of a map. Two
arms for ts variants (`ts/tsx/mts/cts` then `jsx/mjs/cjs`) both → JavaScript
— could be one arm with `|`. Small.

### L2. `render_extras` dispatch could be method

`info/render/mod.rs:36-95`. 19 match arms each calling
`types::<x>::info_render::render_section(lines, X, theme)` — pure dispatch.
Match exists for type safety on `FileExtras` variant. Previous commit
(`db7d97e Merge the two gather_extras dispatch tables`) hinted this could be
a method on `FileExtras`. Worth `FileExtras::render_section(&self, lines,
theme)` if call-site count grows.

### L3. Apologetic comment on document compose match

`types/document/compose.rs:25-40` has `match fmt` for three document formats;
that's the single per-format dispatch conventions allow. Fine, but comment
"Per-format dispatch lives here, the one match" reads like apology —
convention says one match at wiring site is correct. Drop apology.

### L4. `apply()` mode-local Action list grows

`viewer/ui/state.rs:472-491`. Match has explicit list of mode-local Actions
falling through to `Outcome::Unhandled`. Comment says intentional (compile-
error guard when new Action is added). Reasonable, but variant list is 22
entries and growing — when `Action` grows to 40+ becomes chore. Consider
`is_mode_local(action) -> bool` predicate on `Action` that match falls
through to.

### L5. `ModeId` collision risk after `Rendered` split

`viewer/modes/mod.rs:39-53`. 9 variants — `Content`, `Info`, `Help`, `Hex`,
`ImageRender`, `Animation`, `Listing`, `About`, `Rendered`. `Rendered` was
added for generic `RenderedTextMode` — but PDF's text renderer reports
`ModeId::Content` instead (per `text_renderer.rs`), which is awkward. Id is
used for `i:Info` / `x:Hex` jumps and Tab cycling — if two distinct modes
share `ModeId::Content`, `mode_index(Content)` finds only first. Minor today
(PDF only has one Content-id mode), but convention now strained.

### L6. `theme/sgr.rs` length

363 lines for SGR-mechanics; comment says it's the "low-level" file. Long
but coherent. Mentioned because architecture map calls out split (`sgr.rs` /
`style_mode.rs` / `peek_theme.rs` / `manager.rs`) — works well.

### L7. `audio::package` ms-cheap claim now source-dependent

`src/types/audio/package.rs:8-30` module doc says "Re-probes per call (header
+ tag walk, ms-cheap)". Recent commit (1343684) changed File sources to seek
instead of slurp; docstring's "ms-cheap" claim now depends on source variant.
Note in comment so reader doesn't assume same cost for a Memory-backed audio
probe.

## File index

- `src/viewer/listing/mode.rs`
- `src/types/directory/mode.rs`
- `src/types/image/mode.rs`
- `src/types/image/animation_mode.rs`
- `src/types/svg/animation_mode.rs`
- `src/viewer/paged.rs`
- `src/types/document/render.rs`
- `src/types/document/rtf/render.rs`
- `src/types/ebook/compose.rs`
- `src/types/comic/compose.rs`
- `src/types/document/compose.rs`
- `src/viewer/mod.rs`
- `src/viewer/modes/content.rs`
- `src/viewer/wrap_scroll.rs`
- `src/viewer/search.rs`
- `src/viewer/ui/state.rs`
- `src/viewer/modes/pretty_view.rs`
- `src/info/render/mod.rs`
- `src/input/source.rs`
- `docs/features.md`
