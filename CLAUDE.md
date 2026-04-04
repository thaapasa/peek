# peek

Modern terminal file viewer with syntax highlighting, structured data pretty-printing, and image rendering.

## Build & Run

```sh
cargo build                  # debug build
cargo build --release        # release build
cargo run -- [args]          # run with arguments
cargo test                   # run all tests
cargo clippy                 # lint
```

No external runtime dependencies. Image rendering is built-in.

## Architecture

```
src/
  main.rs          — CLI entry point (clap), file type dispatch
  viewer/
    mod.rs         — Viewer trait and registry
    text.rs        — Plain text passthrough
    syntax.rs      — Syntax-highlighted source code (syntect)
    json.rs        — JSON pretty-printer
    yaml.rs        — YAML pretty-printer
    toml.rs        — TOML pretty-printer
    xml.rs         — XML pretty-printer
    image.rs       — Image → ASCII art with density mapping + true color
  detect.rs        — File type detection (extension + magic bytes)
  pager.rs         — Built-in pager (minus crate) with TTY detection
  theme.rs         — Color theme management, true color output
  error.rs         — Error types
```

## Conventions

- Use `anyhow::Result` for application errors, `thiserror` for library-style typed errors
- CLI args defined via clap derive macros on a single `Args` struct in main.rs
- All viewer implementations implement the `Viewer` trait
- Target 24-bit true color; degrade gracefully when terminal doesn't support it
- When stdout is a TTY: use built-in pager. When piped: write directly to stdout
- No unwrap() in non-test code; propagate errors with `?`
