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

### 3. ~~Home-grown XML pretty-printing~~ (done)

Resolved: replaced hand-rolled XML indentation with `quick-xml` Reader/Writer
which properly handles CDATA, comments, processing instructions, self-closing
tags, and namespaces. `quick-xml` was already a direct dependency.

### 4. ~~Parameter explosion for image rendering~~ (done)

Resolved: introduced `ImageConfig` struct bundling `mode`, `width`, `background`,
`margin`. Used by `ImageViewer`, `SvgViewer`, `view_animated`, `render_frame`.

### 5. Eager viewer initialization

`Registry::new()` creates all viewers (syntax, structured, image, SVG, text) up
front even though typically only one is used per invocation. Lazy initialization
would be cleaner.

## Low Priority

### 6. ~~Scroll offset management~~ (done)

Resolved: `ScrollState` struct with `get(mode)`/`get_mut(mode)` methods replaces
the three separate variables and free-function dispatchers. Part of the `ui.rs`
extraction (#1).

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
