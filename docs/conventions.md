# Coding Conventions

## Error handling

- Use `anyhow::Result` for application errors, `thiserror` for library-style typed errors.
- No `unwrap()` in non-test code; propagate errors with `?`.

## CLI

- CLI args defined via clap derive macros on a single `Args` struct in `main.rs`.
- When stdout is a TTY: interactive viewer. When piped: direct output.

## Color output

- **All colored terminal output must go through `PeekTheme::paint()`** — never write
  raw ANSI escape sequences (`\x1b[...m`). This ensures future color mode support
  (256-color, 16-color, monochrome) can be applied in one place.
- Target 24-bit true color; degrade gracefully when terminal doesn't support it.
- Use semantic color roles from `PeekTheme` (`heading`, `label`, `value`, `accent`,
  `muted`, `warning`) rather than hardcoded colors. Derive context-appropriate shades
  via `lerp_color()` when needed.

## Themes

- Themes are `.tmTheme` files embedded at compile time via `include_str!()`.
- `PeekTheme` semantic roles are derived automatically from syntect theme settings.
- Adding a theme: create the `.tmTheme` file, add a `PeekThemeName` variant, wire
  the `include_str!`, `cli_name`, `tmtheme_source`, `next`, and `help_text` methods.

## Viewers and modes

Two parallel abstractions, one per output path:

- **`Viewer` trait** (`viewer/mod.rs`) — one-shot rendering for piped output.
  Implemented by `SyntaxViewer`, `StructuredViewer`, `TextViewer`, `HexViewer`,
  `ImageViewer`, `SvgViewer`. Used only when stdout is not a TTY (or `--print`).
- **`Mode` trait** (`viewer/modes/mod.rs`) — interactive views that participate
  in the viewer's mode stack. Implemented by `ContentMode`, `HexMode`,
  `ImageRenderMode`, `AnimationMode`, `InfoMode`, `HelpMode`. Each file type
  composes a `Vec<Box<dyn Mode>>` via `Registry::compose_modes`; the unified
  event loop in `viewer::interactive::run` drives them.

Adding a new file type means adding a `Mode` impl (or reusing `ContentMode`)
and a line in `compose_modes`. A `Viewer` impl is only needed if the piped
path needs custom rendering — without one, the piped fallback is `TextViewer`
or `HexViewer`.

Modes that re-render based on terminal size override
`Mode::rerender_on_resize() -> true` (image render and animation do; line-based
views don't). Modes that own their scroll position (Hex's byte-aligned offset)
override `owns_scroll` + `scroll`; others let `ViewerState` manage line scroll.

## Module organization

- **Split modules before they get unwieldy.** A file pushing past ~400 lines
  with multiple unrelated concerns is a refactor signal. `info::gather` and
  `viewer::image` are the worked examples — both started as a single mod.rs
  that grew to mix 4–8 different topics, and both got split into a directory
  of focused files. Keep `mod.rs` small: module declarations, re-exports, and
  small "glue" types only. Type-specific logic lives in its own file, named
  for the concern it owns (`exif.rs`, `xmp.rs`, `animation.rs`, `mode.rs`,
  `viewer.rs`).
- **Colocate by concern, not by trait.** All SVG code lives in
  `viewer/image/svg.rs` — the rasterization helpers and the `SvgViewer`
  trait impl share that file because they're the same concern (handling
  SVG content), even though the trait impl pattern is shared with
  `ImageViewer` in `viewer.rs`. A reader who comes in asking "how does SVG
  work" finds one file, not two. Resist the urge to group by abstraction
  shape (every Viewer in `viewers.rs`) — that scatters topic knowledge.
- **Splitting earns its keep when it reduces what the reader has to hold
  in their head.** Don't split a 200-line file that does one thing well.
  Don't split for line count alone. Split when one file is asking the
  reader to track multiple unrelated mental models at once.

## Tests

- **New `info::gather` / `info::render` / `input::detect` functionality
  needs fixture-based tests.** Put them in `src/info/gather/tests.rs` (or
  the equivalent `tests` submodule) and use the real files in
  `test-images/` and `test-data/` as fixtures via
  `PathBuf::from(env!("CARGO_MANIFEST_DIR"))`. Each test loads a fixture
  through the full `detect` → `gather` pipeline and asserts a small set
  of known-true facts about the extracted metadata (dimensions, top-level
  kind, indent style, root element, etc.). Reasoning: these layers are
  thin wrappers over external parsers (image, exif, quick-xml,
  serde_json, …) and their behaviour is hard to assert against synthetic
  inputs alone — fixture tests catch upstream regressions and pin our
  field-extraction logic to ground truth.
- **Synthetic streaming-pass tests stay where they are** (e.g. UTF-8
  edge cases in `info::gather::text`). Fixture tests complement them,
  they don't replace them — a 4-line synthetic input is the right tool
  for "does CRLF detection work at a chunk boundary".
- **Add a fixture if you need one that isn't present.** `test-data/`
  and `test-images/` are first-class — extending them is part of the
  task, not a side errand.

## Standards

- Use IANA-registered MIME types only (RFC 6648 — no `x-` prefixes). Languages
  without registered types fall back to `text/plain`.
