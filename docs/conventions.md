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

## Standards

- Use IANA-registered MIME types only (RFC 6648 — no `x-` prefixes). Languages
  without registered types fall back to `text/plain`.
