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

## Viewers

- All viewer implementations implement the `Viewer` trait.
- The interactive viewer receives a `Fn(PeekThemeName) -> Result<Vec<String>>` closure
  so content can be re-rendered on theme change.
- Image viewers pass `rerender_on_resize: true`; text-based viewers pass `false`.

## Standards

- Use IANA-registered MIME types only (RFC 6648 — no `x-` prefixes). Languages
  without registered types fall back to `text/plain`.
