# peek — Feature Specification

Status legend: ✅ implemented · ◐ partial · ☐ planned · ❓ idea / open

## Operating Modes

### Viewer Mode ◐

Full-screen interactive console view. User exits manually (`q` / `Esc`). Keyboard interaction for
toggling options, scrolling, searching, and switching between views.

Works for all file types via the mode-stack architecture: text/source/structured `ContentMode`,
`ImageRenderMode` for raster + rasterized SVG, `AnimationMode` for GIF/WebP, plus universal
`HexMode` / `InfoMode` / `HelpMode` / `AboutMode`. Scrolling, Tab/i toggle to file info, help (`h`/
`?`), about (`a`), live theme cycle (`t`), color-encoding cycle (`c`), `r` (raw/pretty for
structured; primary-cycle for SVG rasterized↔XML). Image-specific: `b` cycles background, `m` cycles
render mode. Animation: `p` play/pause, `n`/`N` and Left/Right step frames. `l` toggles the
line-number gutter in text views. Search not yet.

### Print Mode ◐

Direct stdout, no interactivity (`cat`-like). Default output by file type:

- **Text / source code** — syntax-highlighted (unless `--plain`)
- **Structured data** — pretty-printed + highlighted; `--raw` emits verbatim source (still
  highlighted unless `--plain`)
- **Images** — ASCII art at contain ratio
- **SVG** — rendered preview (ASCII art)
- **Documents** — extracted text content
- **Binary / unknown** — hex dump (streaming, `hexdump -C` layout, terminal-width aware)

Active when `--print` / `-p` is set or stdout isn't a TTY.

### Mode Selection ◐

- `--viewer` / `-v` forces viewer.
- `--print` / `-p` forces print.
- **Default:** if output exceeds the console size, viewer; else print.
- **Binary / unknown** files default to printing file info and exiting; `--viewer` forces the
  interactive viewer.
- All data types should support both modes where it makes sense.

TTY detection and `--print` / `-p` work. Binary files default to the hex-dump viewer (interactive in
TTY, streamed for pipes); `--plain` / `-P` still uses hex for binary (plain text would corrupt
non-UTF-8 bytes). No content-length-based auto-selection yet (currently TTY → viewer, non-TTY →
print).

### Input ✅

peek is a single-file viewer: at most one positional argument. Stdin: pass `-` explicitly, or pipe
with no file argument. Stdin is auto-detected by magic bytes (images, binary) and content sniffing (
JSON, YAML, XML/SVG); plain text falls back to `--language` for syntax highlighting.

To view several files, run peek once per file. No `cat`-style batch — concatenating images,
structured data, and binary into one stream rarely produces useful output.

| Scenario         | Stdin is TTY                     | Stdin is piped            |
|------------------|----------------------------------|---------------------------|
| `peek` (no args) | Show short help                  | Read stdin, render        |
| `peek -`         | Read stdin (blocks until Ctrl-D) | Read stdin, render        |
| `peek file.rs`   | View file normally               | View file (stdin ignored) |

After consuming piped stdin, peek reopens fd 0 from the controlling terminal (resolved via
`ttyname()` to the real device path, not `/dev/tty`, since macOS kqueue can't register the latter)
so the interactive viewer's keyboard input still works.

Implemented for all viewers — text, source code, structured data, raster images (PNG/JPEG/WebP/…),
animated images (GIF/WebP), and SVG.

## Supported File Types

Not exhaustive — additions over time.

### Source Code ✅

All standard languages supported by syntect with `two-face`/bat extended definitions. Covers 100+
languages including Rust, Python, JavaScript, TypeScript, C, C++, Java, Go, Ruby, Shell, TOML,
Dockerfile.

Features: syntax-colored source with theme support; toggleable line numbers (✅, `--line-numbers` /
`-n` / `l`).

### Structured Data / Config Files

| Format | Extensions      | Status |
|--------|-----------------|--------|
| JSON   | `.json`         | ✅      |
| JSONC  | `.jsonc`        | ☐      |
| JSON5  | `.json5`        | ☐      |
| YAML   | `.yaml`, `.yml` | ✅      |
| TOML   | `.toml`         | ✅      |
| XML    | `.xml`          | ✅      |
| HTML   | `.html`, `.htm` | ✅      |
| CSV    | `.csv`, `.tsv`  | ☐      |

JSONC and JSON5 need parsers that handle comments and extended syntax. HTML may benefit from both
highlighted source and a rendered text view. CSV/TSV could render as a formatted table with column
alignment.

Two viewing sub-modes (toggle with `r`; CLI `--raw`):

- **Pretty** (default) — reformatted with syntax highlighting
- **Raw** — verbatim source with syntax highlighting only

`--plain` / `-P` disables all styling.

### Markup / Documentation ☐

| Format   | Extensions |
|----------|------------|
| Markdown | `.md`      |
| SQL      | `.sql`     |

Markdown: rendered view (styled headings, bold, lists) + highlighted source, cyclable with Tab. SQL:
syntax highlighting.

### Image Files ✅

Raster formats rendered as ASCII art. Supported via the `image` crate:

| Format  | Extensions             |
|---------|------------------------|
| PNG     | `.png`                 |
| JPEG    | `.jpg`, `.jpeg`        |
| GIF     | `.gif`                 |
| BMP     | `.bmp`                 |
| WebP    | `.webp`                |
| TIFF    | `.tiff`, `.tif`        |
| ICO     | `.ico`                 |
| AVIF    | `.avif`                |
| PNM     | `.pnm`, `.pbm`, `.pgm` |
| TGA     | `.tga`                 |
| OpenEXR | `.exr`                 |
| QOI     | `.qoi`                 |
| DDS     | `.dds`                 |

Five ASCII-art rendering modes (cyclable with `m`; CLI `--image-mode`):

| Mode      | Description                                            |
|-----------|--------------------------------------------------------|
| `full`    | All glyphs (block, quadrant, extended)                 |
| `block`   | Block / quadrant elements + ASCII subset               |
| `geo`     | Block / quadrant elements + line segments only         |
| `ascii`   | Legacy luminance-based density ramp                    |
| `contour` | Sobel + Otsu edge detection rendered as line-art       |

In viewer mode, Tab switches between the ASCII art and the file info screen. 24-bit truecolor;
status line shows the active mode.

#### SVG ✅

SVG (`.svg`) is vector; the `image` crate doesn't handle it. Rasterized via `resvg`.

Two viewing modes (toggle with `r`):

- **Rendered preview** (default) — rasterize, render through the image pipeline
- **Source view** — syntax-highlighted XML (pretty or raw)

Re-renders on terminal resize.

##### SVG Animation ☐

`<animate>` / `<animateMotion>` elements are silently ignored — SVG always goes through
`ImageRenderMode` as one rasterized frame. Short term: emit a `FileInfo` warning when these
elements are present so the static render isn't misread as the whole picture. Longer term:
rasterize the animation timeline in `viewer/image/svg.rs` into `Vec<AnimFrame>` and route through
`AnimationMode` like GIF / WebP. (`resvg`'s animation API surface is limited, so the full version
is a bigger project.)

#### Transparency Handling ◐

Images with transparency (PNG, SVG, WebP, GIF) need a compositing background before ASCII rendering.
Without one, transparent regions default to black, making dark content invisible against dark
terminal backgrounds.

| Background     | Description                                       |
|----------------|---------------------------------------------------|
| `none`         | No compositing — transparent regions render as-is |
| `black`        | Solid black                                       |
| `white`        | Solid white                                       |
| `checkerboard` | Classic Photoshop-style pattern                   |

Auto-detection: dark content → white bg, light content → black bg. `--background` flag and `b` key
cycling work. Checkerboard uses 8×8 pixel gray. Compositing is always applied when an alpha channel
is present (no per-image opt-out).

#### Image Sizing Modes ◐

| Mode        | Behavior                                                    |
|-------------|-------------------------------------------------------------|
| `contain`   | Fit within both width and height — whole image always shown |
| `portrait`  | Constrain to console height (may overflow width)            |
| `landscape` | Constrain to console width (may overflow height)            |

Default: `contain`. `contain` works; portrait/landscape and keyboard cycling not yet.

#### Zoom ☐

`+`/`-` to scale up/down from the current sizing baseline. Height overflow is naturally handled by
viewer scrolling. Width overflow is the open question — terminals typically wrap or truncate long
lines, which can look messy.

**Open question:** how to handle width overflow. One approach: viewport-based rendering, where only
the visible portion is rendered (output is always exactly terminal-sized) and the user pans with
arrow keys. Fits naturally with the interactive image viewer (own event loop, handles resize). A
position indicator (`[3,2]/[5,4]`) could show viewport location. Under this model, zoom + pan would
be interactive-only — print mode wouldn't support it. Other options: truncation with an indicator,
or capping zoom so width never exceeds the terminal.

### Animated Images (GIF, WebP) ✅

Auto-plays at native frame rate. `p` toggles play/pause; `n`/`N` and Left/Right step frames; `b`
cycles background. Status line shows frame counter and play/pause. Print mode renders the first
frame. Frame count appears in the file info screen. Transparency handling applies.

### Video Files ❓

Render video as ASCII art in real-time — decode frames and run through the image pipeline. Stretch
goal; may not be practical due to decode performance and terminal refresh-rate limits. Would need an
ffmpeg binding.

In print mode: file metadata (duration, resolution, codec, bitrate), possibly a single frame.

### Document Files ☐

| Format        | Extensions |
|---------------|------------|
| PDF           | `.pdf`     |
| Word (OOXML)  | `.docx`    |
| Excel (OOXML) | `.xlsx`    |

Modern XML-based Office formats only — legacy `.doc` / `.xls` not planned.

Document files should support multiple modes (cyclable with Tab):

- **Text extraction** — primary mode; show what's in the document as plain text.
- **Source browsing** — for OOXML (which is ZIP + XML), browse the internal XML files. Useful for
  debugging or inspecting structure.
- **Rendered preview** (PDF only) — render PDF pages to images, convert to ASCII art. Mostly
  novelty, but useful for seeing page layout at a glance.

File info screen should show document-specific metadata: page count, word count, author, creation
date.

### Binary and Archive Files ◐

For files peek doesn't have a specialized viewer for — ISOs, DMGs, compressed archives,
executables — the baseline shows the **file info screen**:

- File type / MIME (detected via magic bytes through the `infer` crate)
- Size (exact + human-readable)
- Filesystem metadata (permissions, timestamps)

`infer` provides MIME only (e.g. `application/x-iso9660-image`, `application/x-apple-diskimage`) —
no deeper metadata. Format-specific details (ISO volume label, partition table, archive listing,
executable architecture) could be added later with dedicated parsers.

Binary files open in the hex-dump viewer by default (`hexdump -C`-style, terminal-width aware,
streaming via `ByteSource`). File info reachable via Tab / `i` from within hex, and via `--info`.
`--plain` / `-P` still uses hex for binary. No format-specific deep metadata yet.

#### Hex Dump Mode ✅

Reads bytes from disk on demand (no full-file slurp). Layout: `hexdump -C`-compatible — 8-digit
offset, two hex columns of N/2 bytes separated by an extra space, then a printable-ASCII column
between `|`s. Bytes-per-row scales with terminal width: `14 + 4*bpr` columns (rounded down to a
multiple of 8, minimum 8). Pipe mode honors `$COLUMNS` (≥ 24) or falls back to 16.

Reachable from any view with `x`. The viewer maintains a logical `Position` (byte offset or line
index) captured on switch-out from any position-tracking mode and restored on switch-in. Entering
hex from a text view positions the top at the byte offset corresponding to the current line (via
`InputSource::line_to_byte`, approximate for pretty-printed content); returning to text re-aligns
the line scroll. Modes that don't track position (Info, Help, Image preview, Animation) leave the
saved position untouched, so detours preserve where you were.

Pressing `x` again returns to the user's last primary mode (most recent non-aux), regardless of
intervening detours. When hex is the default for a binary file, no primary exists — `x` is a no-op
there.

## Viewer Features

### Color Modes ✅

`--color` / `-C`, or `PEEK_COLOR`. Five modes:

| Mode        | Encoding                                      |
|-------------|-----------------------------------------------|
| `truecolor` | 24-bit RGB (`\x1b[38;2;r;g;bm`) — default     |
| `256`       | xterm 256-color palette (`\x1b[38;5;Nm`)      |
| `16`        | 16 base ANSI colors (`\x1b[3Nm` / `\x1b[9Nm`) |
| `grayscale` | 24-bit luminance only — preserves shading     |
| `plain`     | no escapes — strip all color from the output  |

`c` cycles modes interactively; the rendered-lines cache invalidates on each cycle so the whole UI
repaints in the new encoding.

All callers paint truecolor RGB; the `ColorMode` enum on `PeekTheme` owns the conversion and is the
single point where the encoding is decided. Image rendering routes the same way via
`ColorMode::write_fg` / `write_fg_bg`. Plain mode emits text content with zero ANSI escapes (no SGR
resets), so piped output is safe to compose with other tools.

### File Info Screen ✅

Reachable via Tab (cycle content / info) or `i` (jump to info). Available for every file type via
`--info` and Tab/`i` interactively. Semantic coloring throughout (age-based timestamps, size-based
colors, per-character permission coloring).

- **General** — file name, size (exact + human-readable, e.g. `59,521,024 bytes (56.74 MiB)`), MIME,
  permissions, timestamps
- **Images** — dimensions, megapixels, color mode, bit depth, ICC profile, HDR detection, animation
  stats, EXIF, XMP
- **Documents/text** — line/word/char counts, blank lines, longest line, line endings, indent style,
  encoding, shebang
- **Structured data** — top-level kind, key/element count, max nesting depth, total node count, XML
  root + namespaces
- **SVG** — viewBox, declared dimensions, element counts (paths, groups, rects, circles, text),
  script / external-href flags, plus source text stats
- **Binary** — detected format from magic (Mach-O, ELF, PE, ZIP, SQLite, …)

EXIF: camera make/model, lens, orientation, resolution/DPI, exposure, aperture, ISO, focal length,
flash, white balance, date taken, GPS, artist, copyright. ICC profile name parsed from the embedded
profile's `desc` / `mluc` tag. Animation stats (frame count, total duration, average FPS, loop
count) come from header-walking GIF chunks and parsing WebP RIFF ANIM/ANMF chunks. XMP metadata
scraped from head bytes for Dublin Core / XMP fields (title, subject, description, creator, rights,
rating, label). Structured-data stats from a parse pass. Text stats from a single streaming pass
that also detects BOM-based encoding. HDR detection scans for Ultra HDR gain map markers.

### Text Search ☐

`/` opens search prompt; type pattern, Enter searches. `n` / `N` jump to next / previous. Matches
highlighted in content. Regex is desirable; plain text is the minimum. Applies to all text-based
views (source, structured, document text, file info).

### Line Numbers ✅

Toggleable line numbers for text-based views (ContentMode: source, structured raw/pretty, plain
text, SVG XML). Off by default; `--line-numbers` / `-n` enables at startup, `l` toggles in the
viewer. Gutter is right-aligned with a minimum width of 2 digits and painted in the theme's gutter
color. In pretty mode the numbers count visible pretty-printed lines (the lines actually shown), not
source byte lines.

### Line Wrapping ☐

Long lines (minified JSON, log lines, prose without hard breaks) currently render verbatim — the
terminal wraps them into extra visual rows, consuming row budget that `draw_screen`'s math doesn't
account for, so the status line can scroll out of view and content can bleed past it.

Planned: opt-in soft wrap that pre-slices each visible logical line into visual rows of width
`term_cols`, counts wrapped rows against the row budget, and marks wrapped continuations in the
gutter. Scroll unit stays logical-line; partial wraps at the top/bottom edge are fine.

Toggle with `w`; CLI `--wrap` / `--no-wrap`. Default off (matches `less` / `bat` muscle memory and
keeps source-code alignment intact).

### Horizontal Scrolling ❓

Companion to wrap-off mode: `<` / `>` (or shift-arrows) pan a fixed-width viewport across long
lines, with a truncation indicator in the status bar. Useful for tables and code where wrap would
ruin alignment. Follow-up to line wrapping; both can coexist (wrap toggle wins when on).

### Large File Safeguards ☐

For large files: viewer mode defaults to the file info screen instead of loading full contents.
Display a size warning. Keyboard shortcut to opt in to loading. File info (size, type) obtainable
without reading the whole file.

### Help Screen ◐

`h` / `?` opens the help screen. Shows keyboard shortcuts and the active theme. Shortcut list is
composed per file type from the union of global actions and each loaded mode's extras — so an SVG
file's help shows the background-cycle shortcut, while a JSON file's doesn't. Per-active-mode
filtering (showing only the active mode's extras) not yet done.

### About Screen ✅

`a` shows the gradient peek logo, version, tagline, the active theme's full palette as colored
swatches, and a short list of pointers (homepage, license, common keys). Doubles as a theme
showcase — cycling themes with `t` while on About previews how each theme paints the full palette.

## Keyboard Shortcuts

All for viewer mode. Keys marked *(context)* are file-type-specific.

### Navigation

| Key                   | Action       |
|-----------------------|--------------|
| `q` / `Esc`           | Quit         |
| `Up` / `k`            | Scroll up    |
| `Down` / `j`          | Scroll down  |
| `Page Up`             | Page up      |
| `Page Down` / `Space` | Page down    |
| `Home`                | Go to top    |
| `End`                 | Go to bottom |

### Views and Modes

| Key       | Action                                      |
|-----------|---------------------------------------------|
| `Tab`     | Toggle content / file info                  |
| `i`       | Jump to file info screen                    |
| `h` / `?` | Toggle help screen                          |
| `t`       | Cycle theme                                 |
| `c`       | Cycle output color mode                     |
| `x`       | Toggle hex dump (no-op when hex is default) |
| `a`       | Toggle about / status screen                |

### Search

| Key | Action                |
|-----|-----------------------|
| `/` | Open search prompt    |
| `n` | Next search match     |
| `N` | Previous search match |

### Text Views *(context)*

| Key | Action                                            |
|-----|---------------------------------------------------|
| `l` | Toggle line numbers                               |
| `w` | Toggle line wrapping                              |
| `r` | Toggle pretty-print vs raw (structured data only) |

### Image Views *(context)*

| Key        | Action                                           |
|------------|--------------------------------------------------|
| `m`        | Cycle rendering mode (full/block/geo/ascii)      |
| `b`        | Cycle background (none/black/white/checkerboard) |
| `s`        | Cycle sizing mode (contain/portrait/landscape)   |
| `+` / `=`  | Zoom in                                          |
| `-`        | Zoom out                                         |
| Arrow keys | Pan (when zoomed beyond terminal size)           |

### Animated Image Views *(context: GIF, WebP)*

| Key              | Action                                            |
|------------------|---------------------------------------------------|
| `p`              | Play / pause animation                            |
| `Left` / `Right` | Previous / next frame (when paused)               |
| `n` / `N`        | Next / previous frame (alternative, always works) |

When zoomed in, Left/Right are used for panning (image takes priority); use `n`/`N` for stepping.
These mirror the search navigation keys used in text views.

These bindings are initial suggestions and may be revised. The help screen (`h`) is the
authoritative in-app reference.

## Color and Rendering

### Theme Selection ✅

`--theme` / `PEEK_THEME`. Default `idea-dark`. Four custom embedded `.tmTheme` themes:

- **idea-dark** — JetBrains IDEA default Dark (default)
- **vscode-dark-modern** — VS Code Dark Modern
- **vscode-dark-2026** — VS Code Dark 2026
- **vscode-monokai** — VS Code Monokai

`t` cycles themes live in the interactive viewer.

### Theme Architecture ✅

Syntect themes provide colors for syntax highlighting scopes (keywords, strings, comments) and ~30
editor UI color slots (foreground, background, selection, gutter, find highlight, accent). peek
needs colored output beyond syntax highlighting — file info screens, help text, `--help`, status
indicators, line-number gutters, search highlights, and other UI all need consistent theming.

`PeekTheme` defines semantic color roles:

| Role           | Purpose                                | Derived from (syntect)               |
|----------------|----------------------------------------|--------------------------------------|
| `foreground`   | Default text color                     | `settings.foreground`                |
| `background`   | View background                        | `settings.background`                |
| `heading`      | Section headings, titles               | scope `keyword` or `accent`          |
| `label`        | Field names, option names              | scope `entity.name`                  |
| `value`        | Field values, literals                 | scope `string`                       |
| `accent`       | Emphasis, highlights                   | `settings.accent` or scope `keyword` |
| `muted`        | Secondary text, comments, descriptions | scope `comment`                      |
| `warning`      | File size warnings, errors             | scope `invalid` or red               |
| `gutter`       | Line numbers                           | `settings.gutter_foreground`         |
| `search_match` | Search result highlighting             | `settings.find_highlight`            |
| `selection`    | Selected / active item                 | `settings.selection`                 |

Layers:

1. **Syntect theme** — loaded from custom embedded `.tmTheme` files. Provides syntax scope colors
   and editor UI slots.
2. **peek theme roles** — derived automatically from the syntect theme. Semantic colors for all
   non-syntax UI output.
3. **All colored text output** routes through a common rendering layer: syntect (syntax-highlighted
   code) or peek roles (everything else).
4. **Override support** — custom peek themes could override individual roles if the auto-derived
   mapping doesn't look right for a particular syntect theme. Format and mechanism TBD.

Also serves as the integration point for color compatibility modes — the rendering layer can
downgrade colors from 24-bit to 256/16/none.

`PeekTheme` derives the roles from the active syntect theme. All non-syntax UI (info screens, help,
`--help`) uses these via `PeekTheme::paint()`. `.tmTheme` files embedded at compile time via
`include_str!`. The gutter role drives the line-number column in ContentMode; the search-highlight
role is defined but unused (search not implemented yet).

### Compatibility Modes ◐

Two rendering axes:

| Axis      | Modes                                                                  | Status                                                                  |
|-----------|------------------------------------------------------------------------|-------------------------------------------------------------------------|
| Color     | truecolor, 256, 16, grayscale, plain                                   | ✅ (see [Color Modes](#color-modes-))                                    |
| Character | Full Unicode, ASCII-only (image rendering only — `--image-mode ascii`) | ◐ image side done; UI/glyph fallback for non-Unicode terminals not done |

Color is handled by `ColorMode` — all callers paint truecolor RGB and the active mode decides the
wire form. Image rendering routes through the same point via `ColorMode::write_fg` / `write_fg_bg`.
Character compatibility is partial: `--image-mode ascii` falls back to a luminance density ramp for
terminals without block/quadrant glyphs, but the rest of the UI (status line, info screen) still
uses Unicode box-drawing and dashes.

For library-produced output (syntect), `viewer::ranges_to_escaped` replaces syntect's hardcoded
24-bit `as_24_bit_terminal_escaped` with one routed through `ColorMode::fg_seq`, so
syntax-highlighted code is downgraded along with everything else.

## CLI Options

| Option           | Short | Description                                                   | Status |
|------------------|-------|---------------------------------------------------------------|--------|
| `--help`         | `-h`  | Show help screen and exit (short / long forms)                | ✅      |
| `--version`      | `-V`  | Show version info and exit                                    | ✅      |
| `--viewer`       | `-v`  | Force viewer mode                                             | ☐      |
| `--print`        | `-p`  | Force print mode (direct stdout)                              | ✅      |
| `--plain`        | `-P`  | Disable syntax highlighting and pretty-printing               | ✅      |
| `--raw`          | `-r`  | Output verbatim source (no pretty-print)                      | ✅      |
| `--theme`        | `-t`  | Syntax highlighting theme                                     | ✅      |
| `--color`        | `-C`  | Output color encoding (truecolor/256/16/grayscale/plain)      | ✅      |
| `--language`     | `-l`  | Force syntax language                                         | ✅      |
| `--width`        |       | Image rendering width in characters                           | ✅      |
| `--image-mode`   |       | Image rendering mode                                          | ✅      |
| `--info`         |       | Show file info instead of contents                            | ✅      |
| `--utc`          |       | Show timestamps in UTC (default: local + offset)              | ✅      |
| `--background`   |       | Image transparency background (auto/black/white/checkerboard) | ✅      |
| `--margin`       |       | Image margin in transparent pixels                            | ✅      |
| `--line-numbers` | `-n`  | Enable line numbers (toggle with `l` in the viewer)           | ✅      |
| `--wrap`         |       | Soft-wrap long lines (`--no-wrap` to force off)               | ☐      |
| `--sizing`       |       | Image sizing mode                                             | ☐      |

`--plain` and `--raw` are orthogonal. `--raw` preserves original file structure (no pretty-printing)
but still applies colors and font styles. `--plain` disables all console enhancements (colors, bold,
italic) but doesn't change structure. Combinable: `--plain --raw` gives completely unmodified
content with no styling.

`--print` / `-p` forces print mode. `--plain` / `-P` disables syntax highlighting and
pretty-printing.

### `--help` Screen ✅

`-h` (short) and `--help` (long) produce two custom-themed screens — not the default clap output.

- **`-h` (concise)** — gradient logo, version + tagline, usage line, common options. The 90% case
  without the wall of options.
- **`--help` (full)** — everything in `-h`, plus rarely-used options (theme, color, language, width,
  image-mode, background, margin, utc) and the full theme listing with the active marker.

Both share the gradient-painted logo (small-slant style):

```
                 __  
   ___  ___ ___ / /__
  / _ \/ -_) -_)  '_/
 / .__/\__/\__/_/\_\ 
/_/                  
```

Entire output styled with the active theme — headings, option names, descriptions.
`--help --theme <name>` works as a theme preview / showcase.

### `--version` ✅

`--version` / `-V` prints a single line: `peek X.Y.Z`. Unstyled, suitable for shell scripts (
`peek --version | awk ...`). Themed logo banner is intentionally omitted — for a styled banner with
version info, use `-h` / `--help` or the `a` view in the interactive viewer.

## Distribution ✅

Release artifacts (prebuilt binaries) on GitHub Releases for macOS (`aarch64`, `x86_64`), Linux (
`aarch64`, `x86_64`), and Windows (`x86_64`). POSIX `install.sh` at the repo root fetches the right
archive, verifies SHA256, installs to `$HOME/.local/bin` (or `$PEEK_INSTALL_DIR`). Windows users
download the `.zip` manually. Releases are cut by dispatching `.github/workflows/release.yml`; the
workflow reads the version from `Cargo.toml`, refuses to run if `vX.Y.Z` already exists on `origin`,
and creates+pushes the tag itself.

## Future / Optional Features

### Block Collapsing / Folding ❓

Collapse blocks (objects, arrays, nested structures) in the interactive viewer. Mainly for
structured data (JSON, YAML, TOML, XML) but could extend to code (folding functions, blocks).

**Challenges:** the current pipeline produces a flat `Vec<String>` of ANSI-escaped lines with no
structural metadata. Folding would require:

- Line metadata layer (fold level, block boundaries, visibility state) replacing bare `String` lines
- Virtual line mapping so scroll offsets work with collapsed regions
- Preserving fold state across re-renders (theme toggle, raw/pretty toggle)
- For structured data: retaining parsed structure or using indentation heuristics
- For code: language-aware block detection via syntect scopes (significantly harder,
  language-dependent)

Indentation-based folding for structured data (JSON/YAML) would be the most practical starting
point — pretty-printed output has reliable indentation levels.
