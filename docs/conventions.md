# Coding conventions

## Writing (docs + comments)

All prose follows the `plain-writing` skill, which uses the
[Google developer documentation style guide](https://developers.google.com/style). If the skill
isn't loaded, follow the guide and these points:

- Address the reader as "you." Use the active voice and the present tense. Write full sentences with
  one idea each.
- A comment gives the reason for the code: an invariant, a constraint, or a rejected alternative.
  Don't retell what the code says.
- When you edit a doc, a `//!` header, or a comment, rewrite the prose you change to match.

`mise run lint-prose` checks the docs (README, `manual/src/` and `docs/`) with Vale. CI runs the
same check. `mise run prose` also shows suggestions. Code comments aren't linted.

## Errors

- Use `anyhow::Result` for app errors and `thiserror` for library-style typed errors.
- Don't call `unwrap()` outside tests. Propagate errors with `?`.

## Command-line interface

- All arguments live on one clap-derive `Args` struct in `cli.rs`.
- If stdout is a TTY, peek starts the interactive viewer. If stdout is a pipe, peek writes the
  output directly.

## Color

- Send all colored output through `PeekTheme::paint()`. Never hand-write ANSI escapes (`\x1b[...m`).
  `StyleMode` picks the wire form in one place: 24-bit, 256, 16, grayscale, or plain.
- Use the semantic roles: `heading`, `label`, `value`, `accent`, `muted`, and `warning`. To get
  shades, use `lerp_color()`. Don't hardcode RGB values.
- Target truecolor, and degrade gracefully on terminals that lack it.
- **Sanitize file-controlled text before you paint it.** Any string that comes from file bytes must
  pass through `peek_io::sanitize_terminal_controls` _before_ peek wraps it in its own SGR codes.
  This covers content lines, metadata, entry and file names, and rendered prose. Otherwise, raw
  escape and operating system command (OSC) sequences in the file reach the terminal, which allows
  clipboard writes, title spoofing, and hyperlink injection.

  There is no single global choke point. Once peek's own escapes and the content's escapes mix,
  nothing can tell them apart. So sanitize at the input of each producer, for example the line
  decoder, the `InfoValue` text path, `emit_styled`, and each listing's `flat_line`. A new renderer
  that paints file text must do the same.

## Themes

- `.tmTheme` files live in `themes/`, and `include_str!()` embeds them.
- `PeekTheme` roles derive from the syntect theme settings.
- To add a theme, follow these steps:
  1. Drop the `.tmTheme` file into `themes/`.
  2. Add a `PeekThemeName` variant.
  3. Wire it into `include_str!`, `cli_name`, `tmtheme_source`, `next`, and `help_text`.

## Modes

One abstraction serves two output paths.

- **The `Mode` trait** (`viewer/modes/mod.rs`) is the single rendering contract. Each file type
  composes a `Vec<Box<dyn Mode>>` through `Registry::compose_modes`. For the mode table and the
  trait shape, see [architecture.md](architecture.md#key-abstractions).
- **Interactive output** runs in the bin, in `viewer_session::interactive::run`. On each redraw, the
  event loop calls `Mode::render_window(ctx, scroll, rows) -> Result<Window>`. Streaming modes honor
  the window. Fixed-content modes materialize their content and slice it with `slice_window`. The
  Info, Help, and About modes work this way.
- **Pipe output** runs in `main`. peek renders the first non-aux mode with
  `Mode::render_to_pipe(ctx, &mut PrintOutput)`. For binary files, every mode is aux, so peek uses
  the first mode. The default implementation renders the full viewport and writes each line.
  Override it when streaming or byte-faithful output matters. For example, `HexMode` streams chunks,
  and `ContentMode` preserves the trailing newline.

To add a file type, add a `Mode` implementation, or reuse `ContentMode`, and add a line in
`compose_modes`. Modes that re-render on resize override `rerender_on_resize`. Modes that own their
scroll position, such as the byte offset in Hex, override `owns_scroll` and `scroll`.

## File types

Each type is a self-contained directory at `crates/peek-types/src/types/<name>/`. It owns the reader
side: reading the input, gathering the info, rendering the info, and the view modes.

**Detection** lives in `peek-detect` at `crates/peek-detect/src/types/<name>.rs`, _not_ in the
reader. Detection covers the format enum and the extension, MIME, and content sniffing. Keeping it
there makes detection independent of the reader and the viewer, and Cargo enforces that split. The
reader re-exports its format enum at the module root, in `mod.rs`:
`pub use peek_detect::types::<name>::<Name>Format;`. Code outside `types/<name>/` only **wires** the
type in.

### Files the type module owns

- `mod.rs` holds the module declarations and a brief overview. It has no logic.
- `info_gather.rs` has `gather_extras(...)`, which returns the metadata as `Extras`
  (`Box<dyn InfoExtras>`) through `Box::new(<Stats>)`. It's the single entry point that the
  `info::gather` dispatch calls. Tiny types can combine gathering and rendering in one `info.rs`.
- `info_render.rs` renders the info section for both outputs: the themed terminal output
  (`render_section`) and `--info --json` (`json_section`).
  - Normally one `#[derive(serde::Serialize, InfoView)]` view struct drives both outputs.
  - When the view projects from a separate stats struct through `From<&Stats>`, the `info_section!`
    macro generates the `render_section` and `json_section` pair. You don't write it by hand.
  - Irregular sections use `InfoRow` or a hand-written implementation. For the three build modes,
    see "Adding a new file type" in `architecture.md`.
  - One `impl_info_extras!` row in `info_impls.rs` binds the section to `InfoExtras`. `info::render`
    dispatches dynamically, so there is no per-type render match.
- `reader.rs` or `backends/` (optional) parses the format. It streams where it can, following
  "Stream, don't load" in `CLAUDE.md`.
- `mode.rs` or `animation_mode.rs` (optional) holds the `Mode` implementations, which are wired into
  `compose_modes`.

### Central wiring

All wiring lives in these central sites. Never duplicate it inside `types/<name>/`.

- `crates/peek-detect/src/detect.rs` holds `FileType::<Variant>` and the orchestrator wiring.
  `crates/peek-detect/src/types/<name>.rs` holds the format enum and the sniff helpers.
- `src/gather/mod.rs` dispatches `FileType` to `types::<name>::info_gather::gather_extras`.
- `crates/peek-types/src/types/info_impls.rs` has one
  `impl_info_extras!(<Stats>, ...::render_section)` row that binds the stats struct to `InfoExtras`.
  `info::render` dispatches dynamically.
- `src/compose.rs` (`Registry::compose_modes`) dispatches `FileType` to the mode stack.
- `src/extract/extract.rs` dispatches `FileType` to `types::<name>::extract::extract`. Only
  container types need this.

### Adding a type

1. Create the directory.
2. Add the wiring from the preceding list: detection, the gather arm, and one `info_impls.rs` row.
3. Fill in the gather and render code.

A mode is optional. Text-like types reuse `ContentMode`.

Avoid this anti-pattern: a `match file_type` inside `types/<name>/`, or anywhere other than the
wiring sites. Branch on the file type only at a wiring site, and keep the body of each arm in the
type module.

## Crate imports

Imports form three blocks separated by blank lines: `std`, then external crates, then `crate`,
`super`, and `self`. Sort the imports within each block. Use one `use` for each module path. Items
from the same module share one brace list, and braces never nest.

`rustfmt.toml` enforces this with `group_imports` and `imports_granularity = "Module"`. Both options
need nightly `rustfmt`, so format with `mise run format`. It runs two passes because one pass
doesn't always converge. CI checks with `cargo +nightly fmt --check`. Never place or split a `use`
by hand.

Name dependency crates directly: `peek_io::`, `peek_detect::`, `peek_theme::`, and
`peek_foundation::{viewer, info, …}`. Don't add an in-crate re-export façade or a `use peek_x as y`
alias to avoid the crate name. The path should say which crate an item comes from and agree with
`Cargo.toml`. A re-export of a stable leaf buys nothing. It adds one more jump-to-definition hop,
and the path hides the real source.

Re-export from a dependency **only** when the bare path really can't work. State the reason at the
re-export site. These are the valid cases:

- **The dependency type is part of this crate's public API.** `peek-detect` re-exports
  `peek_io::compression::CompressionFormat` because the public `FileType::Compressed(..)` embeds it.
  The per-type format-enum re-export (`pub use peek_detect::types::<name>::<Name>Format`) is the
  same case. The reader surfaces the enum it's built around as its own root API.
- **Proc-macro surface.** `info::InfoView` re-exports the derive from `peek-foundation-derive`,
  because proc-macro crates can export only macros.
- **Macro hygiene.** `peek-foundation` keeps `pub use peek_theme as theme` only so that the
  `::peek_foundation::theme::PeekTheme` path in the `#[derive(InfoView)]` expansion resolves. It
  resolves through `peek-foundation` because every deriving crate depends on it. Don't use this
  alias in hand-written code.

## Module organization

- **Split a file before it gets unwieldy.** A file past about 400 lines that mixes unrelated
  concerns is a signal to refactor. For example, `info::gather` and `types/image` both split from
  large `mod.rs` files. `viewer/ui/screen.rs` moved out of `state.rs` once the frame-buffer cache
  became a third concern beside mode-stack management and key dispatch.
- **Keep `mod.rs` small.** It holds module declarations, re-exports, and small glue types. Topic
  logic lives in its own file, named for its concern: `exif.rs`, `xmp.rs`, `animation_mode.rs`,
  `mode.rs`, or `screen.rs`.
- **Group code by concern, not by trait.** All SVG rasterization lives in
  `types/image/pipeline/svg.rs`, so a reader asking "how does SVG work" finds one file. Grouping by
  the shape of the abstraction scatters knowledge about a topic.
- **Don't let mechanism leak across concerns.** Sometimes a type has fields that exist only to serve
  a separate concern. An example is a frame-cache `Vec<String>` on a mode-stack struct that exists
  only so `draw` can diff it. In that case, the cache and its draw method belong in their own type
  with a narrow API. The original type loses surface area.
- **Split only when it cuts what the reader must hold in their head.** Don't split a 200-line file
  that does one thing well. Split when one file demands several unrelated mental models.

## Arithmetic

Inline arithmetic is fine for simple things: a sum, an average, one `min` or `max` clamp, or a ratio
of two or three steps. Once a calculation takes a multi-step shape, it earns a named helper.
Examples of multi-step shapes:

- An effective grid from a base size times a zoom factor.
- A viewport clamp plus the maximum scroll.
- A projection from cells to pixels.
- An anchored zoom around the center.
- A packed-bit decode.
- A conversion from a byte offset to a line and column.

Follow these rules:

- **Name the calculation, not the formula.** `zoomed_view.pixel_roi(...)` beats
  `let crop_w = x1.saturating_sub(x0).max(1);` repeated in three places.
- **Lift the helper on the first duplicate, not the third.** Two parallel sites with the same
  formula is the threshold. By the third, you've already paid the cost of drift. This matters most
  when both sites _must_ agree. For example, the pan bounds that one site reports must match the
  clamp that the other applies. The compiler doesn't enforce agreement between parallel
  calculations, but a shared helper does.
- **Pin formulas with tests in the helper module**, not in each caller. When you extract a helper,
  move its inline tests over.
- **Choose between a free function and a method.** Use a method (`ZoomedView::pixel_roi`) when the
  calculation reads several struct fields. Use a free function (`anchor_zoom_change`) when it
  operates on values that the caller supplies.

In practice, extract any calculation block that runs over about 5 lines, repeats across files, or
uses casts, `saturating_*`, and `.max(1)` to defend edge cases. That pattern signals math with
invariants worth naming.

## Tests

- **New `info::gather`, `info::render`, or `input::detect` features need fixture-based tests.**
  - Full pipeline tests, from `detect` to `gather`, live in `src/gather/tests.rs`. Per-type parser
    tests live in the owning `crates/peek-types/src/types/<name>/` module.
  - Use real files in `test-images/` or `test-data/`.
  - The fixtures sit at the **workspace root**, so the path depends on the depth of the crate. The
    root bin uses `env!("CARGO_MANIFEST_DIR")`. Member crates, such as `peek-types` and
    `peek-foundation`, are two levels deeper, so they use
    `concat!(env!("CARGO_MANIFEST_DIR"), "/../..")`. Relative `include_bytes!` paths shift by the
    same depth.
  - Each test runs the pipeline and asserts facts that are known to be true, such as dimensions,
    kind, indent style, or root element.
  - These layers are thin wrappers over external parsers, such as `image`, `exif`, `quick-xml`, and
    `serde_json`. Fixture tests catch regressions in those parsers and pin the extraction to ground
    truth.
- **Keep the synthetic streaming-pass tests.** UTF-8 chunk-boundary cases in `info::gather::text`
  are easier to assert against tiny synthetic inputs. Fixture tests complement these tests and don't
  replace them.
- **Add a fixture if you need one.** `test-data/` and `test-images/` are first-class parts of the
  repository, and extending them is part of the task.

## Standards

- Use only MIME types that are registered with the Internet Assigned Numbers Authority (IANA). RFC
  6648 deprecates the `x-` prefix, so don't use it. Languages without a registered type fall back to
  `text/plain`.
