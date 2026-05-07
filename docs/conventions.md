# Coding Conventions

## Errors

- `anyhow::Result` for application errors; `thiserror` for library-style typed errors.
- No `unwrap()` outside tests. Propagate with `?`.

## CLI

- All args on a single clap-derive `Args` struct in `main.rs`.
- TTY stdout → interactive viewer. Pipe → direct output.

## Color

- All colored output goes through `PeekTheme::paint()`. Never hand-write ANSI escapes
  (`\x1b[...m`) — `ColorMode` decides the on-the-wire form (24-bit / 256 / 16 / grayscale / plain)
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

- **`Mode` trait** (`viewer/modes/mod.rs`) is the single rendering contract. Impls: `ContentMode`,
  `HexMode`, `ImageRenderMode`, `AnimationMode`, `InfoMode`, `HelpMode`, `AboutMode`. Each file type
  composes a `Vec<Box<dyn Mode>>` via `Registry::compose_modes`.
- **Interactive path** (`viewer::interactive::run`) drives the stack through an event loop, calling
  `Mode::render(ctx) -> Vec<String>` per redraw and slicing into the visible viewport.
- **Pipe path** (`main`) picks the first non-aux mode (or first mode for binary, where all are aux)
  and calls `Mode::render_to_pipe(ctx, &mut PrintOutput)`. Default impl materializes `render(ctx)`;
  override when streaming or byte-faithful output matters (HexMode streams chunks, ContentMode
  preserves trailing-newline fidelity for un-highlighted text).

Adding a file type: add a `Mode` impl (or reuse `ContentMode`) and a line in `compose_modes`.

Modes that re-render on resize override `rerender_on_resize`. Modes that own scroll position
(Hex's byte-aligned offset) override `owns_scroll` + `scroll`.

## File types

Each file type is a self-contained subdirectory under `src/types/<name>/`. The directory
owns **all** type-specific logic — input read, info gather, info render, view mode(s).
Code outside `types/<name>/` only **wires** the type into the central dispatchers.

Owned by the type module:

- `mod.rs` — module declarations + brief overview comment. No logic.
- `info_gather.rs` — `gather_extras(...)` returns the type's `FileExtras::<Variant>`
  payload. Single entry point called from `info::gather` dispatch. (Tiny types may
  combine gather + render into one `info.rs`.)
- `info_render.rs` — `render_section(...)` takes the matching `FileExtras::<Variant>`
  and theme, returns themed lines. Single entry point called from `info::render`.
- `reader.rs` / `backends/` (optional) — format-specific parsing, streaming where
  possible (see CLAUDE.md "Stream, don't load").
- `mode.rs` / `animation_mode.rs` (optional) — `Mode` impl(s), wired into
  `compose_modes`.

Wired in (centralized — never duplicated inside `types/<name>/`):

- `input/detect.rs` — extension + magic-byte detection → `FileType::<Variant>`.
- `info/mod.rs` — `FileType` + `FileExtras` enum variants.
- `info/gather/mod.rs` — dispatches `FileType` → `types::<name>::info_gather::gather_extras`.
- `info/render/mod.rs` — dispatches `FileExtras` → `types::<name>::info_render::render_section`.
- `viewer/mod.rs::compose_modes` — dispatches `FileType` → mode stack.

Adding a new type: create the directory, add the four enum/dispatch wiring entries
above, fill in gather + render. Mode is optional (text-like types reuse `ContentMode`).

Anti-pattern: a `match file_type` inside `types/<name>/` or anywhere besides the four
wiring sites. If logic needs to branch on the active file type, the dispatch belongs
at a wiring site and the per-arm body belongs in the corresponding type module.

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

## Tests

- **New `info::gather` / `info::render` / `input::detect` functionality needs fixture-based tests.**
  Live under `src/info/gather/tests.rs` (or equivalent `tests` submodule). Use the real files in
  `test-images/` and `test-data/` via `PathBuf::from(env!("CARGO_MANIFEST_DIR"))`. Each test runs
  the full `detect` → `gather` pipeline and asserts a small set of known-true facts (dimensions,
  top-level kind, indent style, root element). Reasoning: these layers are thin wrappers over
  external parsers (image, exif, quick-xml, serde_json) — fixture tests catch upstream regressions
  and pin field-extraction to ground truth.
- **Synthetic streaming-pass tests stay where they are** — UTF-8 chunk-boundary cases in
  `info::gather::text` are easier to assert against tiny synthetic inputs. Fixture tests complement,
  don't replace.
- **Add a fixture if you need one.** `test-data/` and `test-images/` are first-class; extending them
  is part of the task.

## Standards

- IANA-registered MIME types only (RFC 6648 — no `x-` prefixes). Languages without registered types
  fall back to `text/plain`.
