# Architecture Review

Review of the codebase after initial feature implementation (2026-04-08).
Overall the codebase is well-structured — clear separation of concerns, good
trait-based extensibility, and a principled semantic color system. Areas below
are worth addressing as the project matures.

## High Priority

### 1. ~~Duplicated event loop logic~~ (done)

Resolved: shared state and key handling extracted into `src/viewer/ui.rs`
(`ViewerState`, `ScrollState`, `KeyAction`). Both `interactive.rs` and `animate.rs`
use the shared types. Net reduction of ~156 lines.

### 2. ~~Large functions~~ (done)

Resolved: event loops simplified from 220/258 lines to ~50/70 lines each by
delegating common key handling to `ViewerState::handle_key()`.

## Medium Priority

### 3. Home-grown XML pretty-printing

`structured.rs` has hand-rolled XML indentation that doesn't handle CDATA,
comments, or processing instructions. Since `quick-xml` is already an indirect
dependency (via resvg), it could be used directly for proper formatting.

### 4. Parameter explosion for image rendering

`ImageMode`, `Background`, `margin`, `forced_width` are threaded through 5+
function signatures. A single config struct would clean up the API:

```rust
struct ImageConfig {
    mode: ImageMode,
    background: Background,
    width: u32,
    margin: u32,
}
```

### 5. Eager viewer initialization

`Registry::new()` creates all viewers (syntax, structured, image, SVG, text) up
front even though typically only one is used per invocation. Lazy initialization
would be cleaner.

## Low Priority

### 6. Scroll offset management

Three separate `usize` variables (`content_scroll`, `info_scroll`, `help_scroll`)
with repeated `scroll_mut()`/`current_scroll()` dispatch. A `ScrollState` struct
with `get(mode)`/`set(mode, val)` methods would reduce boilerplate.

### 7. Color painting functions in info.rs

14+ specialized functions (`paint_filename`, `paint_size`, `paint_timestamp`, etc.)
that each do lerp+paint. Could consolidate with a table-driven or enum-based
approach, though the current approach is readable enough.

## What's Working Well

- **Semantic color system** — all output through `PeekTheme::paint()`, no hardcoded ANSI
- **Trait-based viewers** — `Viewer` trait makes adding new formats straightforward
- **Embedded themes** — `include_str!()` means zero runtime dependencies
- **Image pipeline** — resize→composite→render order is correct, glyph matching via Hamming distance is elegant
- **Closure-based re-rendering** — `ContentRenderer` enables theme switching without re-reading files
