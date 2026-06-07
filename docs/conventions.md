# Coding Conventions

## Errors

- `anyhow::Result` for application errors; `thiserror` for library-style typed errors.
- No `unwrap()` outside tests. Propagate with `?`.

## CLI

- All args on a single clap-derive `Args` struct in `cli.rs`.
- TTY stdout → interactive viewer. Pipe → direct output.

## Color

- All colored output goes through `PeekTheme::paint()`. Never hand-write ANSI escapes
  (`\x1b[...m`) — `StyleMode` decides the on-the-wire form (24-bit / 256 / 16 / grayscale / plain)
  in one place.
- Use semantic roles (`heading`, `label`, `value`, `accent`, `muted`, `warning`). Derive shades with
  `lerp_color()`. Don't hardcode RGB.
- Target truecolor; degrade gracefully.

## Themes

- `.tmTheme` files in `themes/`, embedded via `include_str!()`.
- `PeekTheme` semantic roles derive automatically from syntect theme settings.
- Adding a theme: drop the `.tmTheme` file, add a `PeekThemeName` variant, wire `include_str!` /
  `cli_name` / `tmtheme_source` / `next` / `help_text`.

## Modes

One abstraction, two output paths.

- **`Mode` trait** (`viewer/modes/mod.rs`) is the single rendering contract. Each file type
  composes a `Vec<Box<dyn Mode>>` via `Registry::compose_modes` — see
  [architecture.md](architecture.md#key-abstractions) for the full mode table and trait shape.
- **Interactive path** (`viewer_session::interactive::run`, in the bin) drives the stack through an event loop,
  calling `Mode::render_window(ctx, scroll, rows) -> Result<Window>` per redraw. Streaming modes
  honour the requested window; fixed-content modes (Info/Help/About) materialise their output
  and slice via `slice_window`.
- **Pipe path** (`main`) picks the first non-aux mode (or first mode for binary, where all are aux)
  and calls `Mode::render_to_pipe(ctx, &mut PrintOutput)`. Default impl asks `render_window` for
  the full viewport and writes each line; override when streaming or byte-faithful output matters
  (HexMode streams chunks, ContentMode preserves trailing-newline fidelity for un-highlighted text).

Adding a file type: add a `Mode` impl (or reuse `ContentMode`) and a line in `compose_modes`.

Modes that re-render on resize override `rerender_on_resize`. Modes that own scroll position
(Hex's byte-aligned offset) override `owns_scroll` + `scroll`.

## File types

Each file type is a self-contained subdirectory under `crates/peek-types/src/types/<name>/`.
The directory owns the reader-side logic — input read, info gather, info render, view mode(s).
**Detection** (the format enum + the extension/MIME/content-sniff helpers) lives in the
`peek-detect` crate at `crates/peek-detect/src/types/<name>.rs`, *not* the reader — that
keeps detection independent of the reader/viewer layer (a compile-time guarantee via the
crate split). The reader module re-exports its format enum at the module root
(`mod.rs`: `pub use peek_detect::types::<name>::<Name>Format;`). Code outside `types/<name>/`
only **wires** the type into the central dispatchers.

Owned by the type module:

- `mod.rs` — module declarations + brief overview comment. No logic.
- `info_gather.rs` — `gather_extras(...)` returns the type's metadata as
  `Extras` (`Box<dyn InfoExtras>`), built with `Box::new(<Stats>)`. Single entry
  point called from `info::gather` dispatch. (Tiny types may combine gather +
  render into one `info.rs`.)
- `info_render.rs` — the type's info section, both outputs: themed terminal
  (`render_section`) and `--info --json` (`json_section`). Normally a single
  `#[derive(serde::Serialize, InfoView)]` view struct drives both; irregular
  sections use the `InfoRow` runtime model or hand-impl the traits (see
  architecture.md → "Adding a new file type" for the three modes). Bound to the
  `InfoExtras` trait by one `impl_info_extras!` row in `info_impls.rs`;
  `info::render` invokes it dynamically — there is no per-type render match.
- `reader.rs` / `backends/` (optional) — format-specific parsing, streaming where
  possible (see CLAUDE.md "Stream, don't load").
- `mode.rs` / `animation_mode.rs` (optional) — `Mode` impl(s), wired into
  `compose_modes`.

Wired in (centralized — never duplicated inside `types/<name>/`):

- `crates/peek-detect/src/detect.rs` — the `FileType::<Variant>` + orchestrator wiring;
  `crates/peek-detect/src/types/<name>.rs` — the format enum + per-type sniff helpers.
- `src/gather/mod.rs` (the bin's gather hub) — dispatches `FileType` →
  `types::<name>::info_gather::gather_extras`.
- `crates/peek-types/src/types/info_impls.rs` — one `impl_info_extras!(<Stats>, ...::render_section)`
  row binding the stats struct to the `InfoExtras` trait. Replaces the old
  `FileExtras` enum variant + render match; `info::render` dispatches dynamically.
- `src/compose.rs` (the bin's `Registry::compose_modes`) — dispatches `FileType` → mode stack.
- `src/extract/extract.rs` (the bin's extract hub, container types only) — dispatches
  `FileType` → `types::<name>::extract::extract`.

Adding a new type: create the directory, add the wiring entries above (detection,
the gather arm, one `info_impls.rs` row), fill in gather + render. Mode is optional
(text-like types reuse `ContentMode`).

Anti-pattern: a `match file_type` inside `types/<name>/` or anywhere besides the
wiring sites above. If logic needs to branch on the active file type, the dispatch
belongs at a wiring site and the per-arm body belongs in the corresponding type module.

## Module organization

- **Split before unwieldy.** A file past ~400 lines mixing unrelated concerns is a refactor signal.
  Worked examples: `info::gather` and `types/image` both started as fat `mod.rs` files and got
  split into per-concern directories. `viewer/ui/screen.rs` was lifted out of `state.rs` once the
  frame-buffer cache + per-row diff loop became a third concern next to mode-stack management and
  key dispatch.
- **`mod.rs` stays small** — module declarations, re-exports, small glue types only. Topic-specific
  logic lives in its own file named for the concern (`exif.rs`, `xmp.rs`, `animation_mode.rs`,
  `mode.rs`, `screen.rs`).
- **Colocate by concern, not by trait.** All SVG rasterization helpers live in
  `types/image/pipeline/svg.rs` because they're one concern. A reader asking "how does SVG work"
  finds one file. Resist grouping by abstraction shape — that scatters topic knowledge.
- **Mechanism doesn't leak across concerns.** When one type's fields exist solely to support a
  separate concern's behavior (e.g. a frame-cache `Vec<String>` living on a mode-stack struct just
  so `draw` can diff against it), the cache and its draw method belong in their own type with a
  narrow API. The original struct loses surface area; readers learning the secondary concern don't
  need to load the primary one.
- **Splitting earns its keep when it reduces what the reader has to hold in their head.** Don't
  split a 200-line file that does one thing well. Split when one file demands tracking multiple
  unrelated mental models.

## Arithmetic

Inline arithmetic is fine for simple things — a sum, an average, one
`min` / `max` clamp, a 2- or 3-step ratio. The moment a calculation
crosses into a multi-step shape (effective grid from base × zoom,
viewport clamp + max-scroll, cell→pixel projection, anchored
zoom-around-centre, packed-bit decode, byte-offset → line-and-column,
…) it earns a named helper.

- **Name the calculation, not the formula.** `zoomed_view.pixel_roi(...)`
  beats `let crop_w = x1.saturating_sub(x0).max(1);` repeated three
  places. The reader doesn't have to derive intent from operator
  arithmetic.
- **Lift on the first duplicate, not the third.** Two parallel sites
  with the same formula is the threshold — by the third you've already
  paid the drift cost. Especially when both sites *must* agree (e.g.
  pan-bounds reported by one site must match the clamp the other
  applies); the compiler doesn't enforce parallel arithmetic agreement,
  a shared helper does.
- **Pin the formulas with tests in the helper module**, not in each
  caller. Move the existing inline tests over when extracting.
- **Free fn vs method**: methods on a state-bearing struct
  (`ZoomedView::pixel_roi`) when the calculation reads several of its
  fields; free fn (`anchor_zoom_change`) when it operates on values
  the caller hands in and doesn't need a struct shape.

Concretely: any block that takes more than ~5 lines, repeats across
files, or uses casts plus `saturating_*` plus `.max(1)` to defend
against edge cases — extract it. The cast/saturating/floor pattern
is a signal the math has invariants worth naming.

## Tests

- **New `info::gather` / `info::render` / `input::detect` functionality needs fixture-based tests.**
  Full `detect` → `gather` pipeline tests live at `src/gather/tests.rs` (the bin's gather hub);
  per-type parser tests live in the owning `crates/peek-types/src/types/<name>/` module. Use the
  real files in `test-images/` and `test-data/`. Fixtures sit at the **workspace root**, so the
  path depends on the crate: the root bin uses `env!("CARGO_MANIFEST_DIR")` directly, but a member
  crate (peek-types / peek-foundation) is two levels deeper — use
  `concat!(env!("CARGO_MANIFEST_DIR"), "/../..")` (and the relative `include_bytes!` depth shifts
  the same way). Each test runs the pipeline and asserts a small set of known-true facts
  (dimensions, top-level kind, indent style, root element). Reasoning: these layers are thin
  wrappers over external parsers (image, exif, quick-xml, serde_json) — fixture tests catch
  upstream regressions and pin field-extraction to ground truth.
- **Synthetic streaming-pass tests stay where they are** — UTF-8 chunk-boundary cases in
  `info::gather::text` are easier to assert against tiny synthetic inputs. Fixture tests complement,
  don't replace.
- **Add a fixture if you need one.** `test-data/` and `test-images/` are first-class; extending them
  is part of the task.

## Standards

- IANA-registered MIME types only (RFC 6648 — no `x-` prefixes). Languages without registered types
  fall back to `text/plain`.
