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

## Module organization

- **Split before unwieldy.** A file past ~400 lines mixing unrelated concerns is a refactor signal.
  Worked examples: `info::gather` and `viewer::image` both started as fat `mod.rs` files and got
  split into per-concern directories.
- **`mod.rs` stays small** — module declarations, re-exports, small glue types only. Topic-specific
  logic lives in its own file named for the concern (`exif.rs`, `xmp.rs`, `animation.rs`, `mode.rs`,
  `viewer.rs`).
- **Colocate by concern, not by trait.** All SVG rasterization helpers live in
  `viewer/image/svg.rs` because they're one concern. A reader asking "how does SVG work" finds one
  file. Resist grouping by abstraction shape — that scatters topic knowledge.
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
