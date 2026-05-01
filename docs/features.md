# peek — Feature Specification

## Operating Modes

peek has two operating modes: **viewer mode** and **print mode**.

### Viewer Mode

Full-screen interactive console view. The user must manually quit (e.g. `q`, `Esc`)
to exit. Supports keyboard interaction for toggling options, scrolling, searching, and
switching between views.

**Status: Partially implemented.** Interactive viewing works for all file types via
a mode-stack architecture (text/source/structured `ContentMode`, `ImageRenderMode`
for raster + rasterized SVG, `AnimationMode` for GIF/WebP, plus universal `HexMode`
/ `InfoMode` / `HelpMode` / `AboutMode`). Scrolling (Up/Down/j/k, PgUp/PgDn, Home/End),
Tab/i toggle to file info, help screen (h/?), about screen (a), live theme cycling
(t), color-encoding cycling (c), and `r` (raw/pretty for structured data,
primary-cycle for SVG rasterized↔XML). Image-specific keys: `b` cycles background,
`m` cycles render mode (full/block/geo/ascii). Animation: `p` play/pause, `n`/`N`
and Left/Right step frames. No search or line numbers yet.

### Print Mode

Outputs data directly to the console and exits, similar to `cat`. No interactivity.

Default print mode output by file type:
- **Text / source code:** syntax-highlighted content (unless `--plain`).
- **Structured data:** pretty-printed with syntax highlighting by default. `--raw`
  outputs verbatim source (still highlighted unless `--plain`).
- **Images:** ASCII art at contain ratio.
- **SVG:** rendered preview (ASCII art).
- **Documents:** extracted text content.
- **Binary / unknown:** hex dump (streaming, `hexdump -C` layout, terminal-width aware).

**Status: Partially implemented.** Direct stdout output works when `--print` / `-p` is
set or stdout is not a TTY.

### Mode Selection

- `--viewer` / `-v` forces viewer mode.
- `--print` / `-p` forces print mode.
- **Default behavior:** if output is longer than the console size, start in viewer mode;
  otherwise use print mode.
- **Binary / unknown files** default to printing file info and exiting. `--viewer`
  forces the interactive viewer (for future features like hex dump).
- All data types should support both modes where it makes sense — the same content
  should be viewable interactively or printable to stdout.

**Status: Partial.** TTY detection and `--print` / `-p` implemented. Binary files
default to the hex-dump viewer (interactive in TTY mode, streamed for pipes), and
`--plain` / `-P` still uses hex for binary (since plain text would corrupt
non-UTF-8 bytes). No content-length-based auto-selection yet (currently: TTY →
viewer, non-TTY → print).

### Input

peek is a **single-file viewer**: it accepts at most one positional argument.
Reading from stdin is supported: pass `-` explicitly, or pipe data in with no
file argument. Stdin content is auto-detected by magic bytes (images, binary)
and content sniffing (JSON, YAML, XML/SVG); plain text falls back to
`--language` for syntax highlighting.

To view several files, run peek once per file. There is no `cat`-style batch
mode — concatenating images, structured data, and binary files into one
output stream rarely produces a useful result, and the interactive viewer is
built around a single file at a time.

| Scenario         | Stdin is TTY                     | Stdin is piped            |
|------------------|----------------------------------|---------------------------|
| `peek` (no args) | Show short help                  | Read stdin, render        |
| `peek -`         | Read stdin (blocks until Ctrl-D) | Read stdin, render        |
| `peek file.rs`   | View file normally               | View file (stdin ignored) |

After consuming piped stdin, peek reopens fd 0 from the controlling terminal
(resolved via `ttyname()` to the real device path, not `/dev/tty`, because macOS
kqueue can't register the latter) so the interactive viewer's keyboard input
still works.

**Status: Implemented** for all viewers — text, source code, structured data,
raster images (PNG/JPEG/WebP/…), animated images (GIF/WebP), and SVG.


## Supported File Types

The following file types should be supported. This list is not exhaustive — additional
types may be added over time.

### Source Code

All standard programming languages supported by the syntax highlighting library
(syntect with extended definitions via `two-face`/bat). This covers 100+ languages
including: Rust, Python, JavaScript, TypeScript, C, C++, Java, Go, Ruby, Shell,
TOML, Dockerfile, and many more.

Viewing features:
- Syntax-colored source with theme support.
- Toggleable line numbers.

**Status: Implemented** via syntect with `two-face` extended syntax definitions. Line
numbers not yet implemented.

### Structured Data / Config Files

| Format | Extensions                  | Status      |
|--------|-----------------------------|-------------|
| JSON   | `.json`                     | Implemented |
| JSONC  | `.jsonc`                    | Planned     |
| JSON5  | `.json5`                    | Planned     |
| YAML   | `.yaml`, `.yml`             | Implemented |
| TOML   | `.toml`                     | Implemented |
| XML    | `.xml`                      | Implemented |
| HTML   | `.html`, `.htm`             | Implemented |
| CSV    | `.csv`, `.tsv`              | Planned     |

JSONC and JSON5 need parsers that handle comments and extended syntax. HTML may benefit
from both syntax-highlighted source view and a rendered/extracted text view. CSV/TSV
could be rendered as a formatted table with column alignment.

Structured data files support two viewing sub-modes (togglable with `r` in viewer
mode, selectable via `--raw` CLI flag for print mode):
- **Pretty-printed** (default): reformatted for readability with syntax highlighting.
- **Raw**: verbatim file contents with syntax highlighting only.

**Status: Implemented.** Pretty-printing with syntax highlighting works for JSON, YAML,
TOML, XML, HTML. `--raw` / `-r` outputs verbatim source with highlighting only (no
pretty-printing). `--plain` / `-P` disables all styling. In the interactive viewer,
`r` toggles between pretty-printed and raw views.

### Markup / Documentation

| Format   | Extensions | Status  |
|----------|------------|---------|
| Markdown | `.md`      | Planned |
| SQL      | `.sql`     | Planned |

Markdown could support a rendered view (styled headings, bold, lists, etc.) in addition
to syntax-highlighted source, cyclable with `Tab`. SQL gets syntax highlighting.

**Status: Not implemented.**

### Image Files

Raster image formats rendered as ASCII art. Supported formats (via the `image` crate):

| Format   | Extensions             | Status      |
|----------|------------------------|-------------|
| PNG      | `.png`                 | Implemented |
| JPEG     | `.jpg`, `.jpeg`        | Implemented |
| GIF      | `.gif`                 | Implemented |
| BMP      | `.bmp`                 | Implemented |
| WebP     | `.webp`                | Implemented |
| TIFF     | `.tiff`, `.tif`        | Implemented |
| ICO      | `.ico`                 | Implemented |
| AVIF     | `.avif`                | Implemented |
| PNM      | `.pnm`, `.pbm`, `.pgm` | Implemented |
| TGA      | `.tga`                 | Implemented |
| OpenEXR  | `.exr`                 | Implemented |
| QOI      | `.qoi`                 | Implemented |
| DDS      | `.dds`                 | Implemented |

Four ASCII-art rendering modes (cyclable with `m` in viewer mode):

| Mode    | Description                                      | Status      |
|---------|--------------------------------------------------|-------------|
| `full`  | All available glyphs (block, quadrant, extended) | Implemented |
| `block` | Unicode block/quadrant elements + ASCII subset   | Implemented |
| `geo`   | Block/quadrant elements + line segments only     | Implemented |
| `ascii` | Legacy luminance-based density ramp              | Implemented |

Rendering mode also selectable via `--image-mode` CLI option.

In viewer mode, `Tab` switches between the ASCII art view and the file info screen.

**Status: Implemented.** Four modes work with 24-bit truecolor. Interactive image
viewer supports resize. Tab/i view switching to file info works. `m` cycles between
the four rendering modes; the active mode is shown in the status line.

#### SVG

SVG files (`.svg`) are vector graphics and not supported by the `image` crate. They
require a separate rasterizer (e.g. `resvg`) to convert to a bitmap before rendering.

SVG supports two viewing modes (togglable with `r` in viewer mode):
- **Rendered preview** (default): Rasterize the SVG and render as ASCII art through the
  image pipeline.
- **Source view:** Syntax-highlighted XML source (pretty-printed or raw).

**Status: Implemented.** SVG rasterization via `resvg`, with `r` key to toggle between
rendered preview and XML source. Re-renders on terminal resize.

#### Transparency Handling

Images with transparency (PNG, SVG, WebP, GIF, etc.) need a compositing background
before ASCII rendering. Without one, transparent regions default to black, making dark
content invisible against dark terminal backgrounds.

Available background modes (cyclable via keyboard in viewer mode, selectable via CLI):

| Background     | Description                                       |
|----------------|---------------------------------------------------|
| `none`         | No compositing — transparent regions render as-is |
| `black`        | Solid black background                            |
| `white`        | Solid white background                            |
| `checkerboard` | Classic checkerboard pattern (like Photoshop)     |

**Auto-detection:** For images with an alpha channel, the default background should be
chosen automatically by analyzing the non-transparent pixels. If the visible content is
mostly dark, prefer a light background (white or checkerboard); if mostly light, prefer
black. For images without transparency, the background setting is irrelevant and can be
ignored.

**Status: Partially implemented.** Auto-detection of background color is implemented
(dark content → white bg, light content → black bg). `--background` CLI flag and `b`
key cycling in the interactive viewer are implemented. Checkerboard uses 8×8 pixel gray
pattern. No per-image auto-detection of whether to enable compositing (always applied
when alpha channel is present).

#### Image Sizing Modes

Three sizing modes control how the image fits the terminal:

| Mode        | Behavior                                                    |
|-------------|-------------------------------------------------------------|
| `contain`   | Fit within both width and height — whole image always shown |
| `portrait`  | Constrain to console height (may overflow width)            |
| `landscape` | Constrain to console width (may overflow height)            |

Default should be `contain`. Cyclable via keyboard in viewer mode, selectable via CLI.

**Status: Partially implemented.** `contain` mode works. Portrait and landscape modes
and keyboard cycling not yet implemented.

#### Zoom

Viewer mode should support zoom controls (e.g. `+`/`-` keys) to scale the image up or
down from the current sizing baseline.

Height overflow is naturally handled by the viewer (scrolling). Width overflow is less
clear — terminals typically wrap or truncate long lines, which could look messy.

**Open question:** how to handle width overflow intuitively. One possible approach:
viewport-based rendering, where only the visible portion of the image is rendered at
any given time (output is always exactly terminal-sized) and the user pans across the
image with arrow keys. This would fit naturally with the interactive image viewer, which
already has its own event loop and handles resize. A position indicator (e.g.
`[3,2]/[5,4]`) could show viewport location. Under this model, zoom + pan would be an
interactive-viewer-only feature — print mode would not support it. Other options include
truncation with an indicator, or capping zoom so width never exceeds the terminal. To
be decided.

**Status: Not implemented.**

### Animated Images (GIF, WebP)

Animated GIFs and WebPs should be playable in viewer mode — render each frame as ASCII
art and cycle through them at the original frame rate. Since the image rendering
pipeline already exists, this is primarily a matter of extracting frames and timing
the playback loop.

Viewer mode should also support a single-frame mode where playback is paused and the
user can step through frames using arrow keys or `n`/`N` (the latter also works when
zoomed in, where arrow keys are used for panning).

In print mode, a reasonable default would be to render the first frame.

Transparency handling (see above) applies to animated images as well.

**Status: Implemented.** Auto-plays GIF and WebP animations at native frame rate. `p`
toggles play/pause, `n`/`N` and Left/Right step frames, `b` cycles background. Status
line shows frame counter and play/pause state. Print mode renders first frame. Frame
count shown in file info screen.

### Video Files (tentative)

Render video as ASCII art in real-time in viewer mode — decode frames and pipe them
through the image rendering pipeline. This is a stretch goal and may not be practical
due to decode performance and terminal refresh rate limitations. Would require a video
decoding dependency (e.g. ffmpeg bindings).

In print mode, could show file metadata (duration, resolution, codec, bitrate) and
possibly render a single frame or thumbnail.

**Status: Idea — feasibility uncertain.**

### Document Files

| Format        | Extensions | Status  |
|---------------|------------|---------|
| PDF           | `.pdf`     | Planned |
| Word (OOXML)  | `.docx`    | Planned |
| Excel (OOXML) | `.xlsx`    | Planned |

Only the modern XML-based Office formats (OOXML) are in scope — legacy binary formats
(`.doc`, `.xls`) are not planned.

Document files should support multiple viewing modes (cyclable with `Tab`):

- **Text extraction:** Extract and display the textual content without formatting. This
  is the primary mode — just show what's in the document as plain text so you can
  quickly see the contents.
- **Source browsing:** For OOXML files (which are ZIP archives containing XML), allow
  browsing the internal XML source files. Useful for debugging or inspecting document
  structure.
- **Rendered preview (PDF only):** Render PDF pages to images and convert to ASCII art.
  Primarily a novelty/fun feature, but could be genuinely useful for seeing page layout
  at a glance.

The file info screen (see below) should show document-specific metadata: page count,
word count, author, creation date, etc.

**Status: Not implemented.**

### Binary and Archive Files (fallback)

For any file that peek doesn't have a specialized viewer for — ISOs, DMGs, compressed
archives, executables, and other binary formats — the baseline behavior is to show the
**file info screen** with:

- File type / MIME type (detected via magic bytes, currently using the `infer` crate)
- File size (exact + human-readable)
- Filesystem metadata (permissions, timestamps)

The `infer` crate provides MIME detection only (e.g. `application/x-iso9660-image`,
`application/x-apple-diskimage`) — no deeper metadata. Format-specific details (e.g.
ISO volume label, partition table, archive file listing, executable architecture) could
be added later with dedicated parsers on a per-format basis.

This ensures that peek always has something useful to show for any file, even if it
can't render the contents.

**Status: Implemented.** Binary files open in the hex-dump viewer by default
(`hexdump -C`-style layout, terminal-width aware, streaming reads via
`ByteSource`). The file info screen is still reachable via `Tab` / `i` from
within hex mode and via the `--info` flag. `--plain` / `-P` still uses hex
for binary (plain text mode cannot represent non-UTF-8 bytes). No
format-specific deep metadata (archive listing, executable info) yet.

#### Hex Dump Mode

Binary files default to a hex-dump viewer that reads bytes from disk on demand
(no full-file slurp). Layout is `hexdump -C`-compatible: an 8-digit offset, two
hex columns of N/2 bytes separated by an extra space, then a printable-ASCII
column between `|`s. Bytes-per-row scales with terminal width: `14 + 4*bpr`
columns (rounded down to a multiple of 8, minimum 8). Pipe-mode output honors
`$COLUMNS` (≥ 24) or falls back to a fixed 16 bytes per row.

Hex mode is also reachable from any other view with the `x` key. The viewer
maintains a logical `Position` (byte offset or line index) that's captured
on switch-out from any position-tracking mode and restored on switch-in to
another. So entering hex from a text view positions the top at the byte
offset corresponding to the current line (via `InputSource::line_to_byte`,
approximate for pretty-printed structured content), and returning from hex
to text re-aligns the line scroll. Modes that don't track position (Info,
Help, Image preview, Animation) leave the saved position untouched, so
detours through them preserve where you were.

Pressing `x` again returns to the user's last primary mode (the most
recent non-aux mode), regardless of how many aux detours intervened. When
hex is launched directly as the default for a binary file, there is no
primary to return to — `x` is a no-op there, matching the old behavior.

**Status: Implemented.**


## Viewer Features

Cross-cutting features available in viewer mode regardless of file type.

### Color Modes

The output color encoding is selected via `--color` (`-C`) or the `PEEK_COLOR`
env var. Five modes are supported:

| Mode        | Encoding                                       |
|-------------|------------------------------------------------|
| `truecolor` | 24-bit RGB (`\x1b[38;2;r;g;bm`) — default      |
| `256`       | xterm 256-color palette (`\x1b[38;5;Nm`)       |
| `16`        | 16 base ANSI colors (`\x1b[3Nm` / `\x1b[9Nm`)  |
| `grayscale` | 24-bit luminance only — preserves shading      |
| `plain`     | no escapes — strip all color from the output   |

`c` cycles between modes in the interactive viewer; the cache of rendered
lines is invalidated on each cycle so the whole UI repaints in the new
encoding.

**Status: Implemented.** All callers paint in truecolor RGB; the
`ColorMode` enum on `PeekTheme` owns the conversion and is the single
point where the encoding is decided. Image rendering routes the same
way via `ColorMode::write_fg` / `write_fg_bg`. Plain mode emits the
text content with zero ANSI escapes (including no SGR resets), which
makes piped output safe to compose with other tools.

### File Info Screen

A metadata view available for all files, accessible via `Tab` (cycles between content
view and info screen) or `i` (jump directly to info). Contains details such as:

- **General:** file name, file size (exact byte count and human-readable, e.g.
  `59,521,024 bytes (56.74 MiB)`), MIME type, file permissions, timestamps
- **Images:** dimensions/resolution, color mode, bit depth per pixel, EXIF data
- **Documents/text:** line count, word count, character count, encoding
- **Structured data:** key count, nesting depth, schema summary

Also available in print mode via a CLI flag (e.g. `--info`).

**Status: Partially implemented.** Basic file info (name, path, size, MIME, timestamps,
permissions) works for all file types via `--info` flag and Tab/i in the interactive
viewer. Image extras (dimensions, color type, bit depth, HDR detection, EXIF metadata)
and text extras (line/word/char counts) are included. Info screen uses semantic coloring
(age-based timestamps, size-based colors, per-character permission coloring). HDR
detection scans for Ultra HDR gain map markers. EXIF extraction covers camera
make/model, lens, exposure, aperture, ISO, focal length, flash, white balance, date
taken, GPS coordinates, artist, and copyright. Not yet implemented: structured data key
count/nesting depth, schema summary.

### Text Search

Text search within viewer mode:

- `/` to open search prompt, type pattern, `Enter` to search.
- `n` / `N` to jump to next / previous match.
- Matches should be highlighted in the content.
- Regex support is desirable but plain text search is the minimum.

Applies to all text-based views (source code, structured data, document text
extraction, file info screen).

**Status: Not implemented.**

### Line Numbers

Toggleable line numbers for text-based views (source code, structured data, plain
text). Controllable via CLI flag and keyboard shortcut in viewer mode.

**Status: Not implemented.**

### Large File Safeguards

For large files:

- Viewer mode should default to the **file info screen** instead of attempting to load
  the full file contents.
- A warning about the file size should be displayed.
- A keyboard shortcut should allow the user to opt in to loading the full content.
- File info (size, type, etc.) should be obtainable without reading the entire file.

**Status: Not implemented.**

### Help Screen

An in-app help screen accessible via `h` or `?` that lists all available keyboard
commands and their descriptions. Should reflect the current context (e.g. image-specific
commands only shown when viewing an image).

**Status: Partially implemented.** A global help screen is accessible via `h` or `?`.
It shows keyboard shortcuts and the currently active theme. The shortcut list is
composed per file type from the union of global actions and each loaded mode's
extras — so an SVG file's help shows the background-cycle shortcut, while a JSON
file's doesn't. Per-active-mode filtering (showing only the currently active mode's
extras) is not yet done.

### About Screen

An in-app about screen accessible via `a` that shows the gradient peek logo,
version, tagline, the active theme's full palette as colored swatches, and a
short list of pointers (homepage, license, common keys). Doubles as a theme
showcase — cycling themes with `t` while on the About screen previews how
each theme paints the full palette.

**Status: Implemented** as `AboutMode` in `viewer/modes/about.rs`.


## Keyboard Shortcuts

All shortcuts are for viewer mode. Keys marked *(context)* are only available for
certain file types.

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

| Key  | Action                |
|------|-----------------------|
| `/`  | Open search prompt    |
| `n`  | Next search match     |
| `N`  | Previous search match |

### Text Views *(context)*

| Key | Action                                            |
|-----|---------------------------------------------------|
| `l` | Toggle line numbers                               |
| `r` | Toggle pretty-print vs raw (structured data only) |

### Image Views *(context)*

| Key         | Action                                           |
|-------------|--------------------------------------------------|
| `m`         | Cycle rendering mode (full/block/geo/ascii)      |
| `b`         | Cycle background (none/black/white/checkerboard) |
| `s`         | Cycle sizing mode (contain/portrait/landscape)   |
| `+` / `=`   | Zoom in                                          |
| `-`         | Zoom out                                         |
| Arrow keys  | Pan (when zoomed beyond terminal size)           |

### Animated Image Views *(context: GIF, WebP)*

| Key              | Action                                            |
|------------------|---------------------------------------------------|
| `p`              | Play / pause animation                            |
| `Left` / `Right` | Previous / next frame (when paused)               |
| `n` / `N`        | Next / previous frame (alternative, always works) |

When zoomed in, `Left`/`Right` are used for panning (image view takes priority).
Use `n`/`N` for frame stepping while zoomed. These mirror the search navigation
keys used in text views.

These keybindings are initial suggestions and may be revised. The help screen (`h`)
is the authoritative in-app reference.


## Color and Rendering

### Theme Selection

- Themes are selected via `--theme` CLI option or `PEEK_THEME` environment variable.
- Default theme: `idea-dark`.
- Four custom embedded themes in `.tmTheme` format, compiled into the binary:
  - **idea-dark** — JetBrains IDEA default Dark theme (default)
  - **vscode-dark-modern** — VS Code Dark Modern theme
  - **vscode-dark-2026** — VS Code Dark 2026 theme
  - **vscode-monokai** — VS Code Monokai theme
- Themes can be cycled live in the interactive viewer with `t`.

**Status: Implemented.** Theme selection works via CLI/env var. Four custom embedded
themes replace the previous syntect defaults. Live theme cycling is available in the
interactive viewer.

### Theme Architecture

Syntect themes provide colors for syntax highlighting scopes (keywords, strings,
comments, etc.) and a set of ~30 editor UI color slots (foreground, background,
selection, gutter, find highlight, accent, etc.). However, peek needs colored output
beyond syntax highlighting — file info screens, help text, `--help` output, status
indicators, line number gutters, search highlights, and other UI elements all need
consistent theming.

To support this, peek should define its own **theme abstraction** with semantic color
roles:

| Role           | Purpose                                       | Derived from (syntect)               |
|----------------|-----------------------------------------------|--------------------------------------|
| `foreground`   | Default text color                            | `settings.foreground`                |
| `background`   | View background                               | `settings.background`                |
| `heading`      | Section headings, titles                      | scope `keyword` or `accent`          |
| `label`        | Field names, option names                     | scope `entity.name`                  |
| `value`        | Field values, literals                        | scope `string`                       |
| `accent`       | Emphasis, highlights                          | `settings.accent` or scope `keyword` |
| `muted`        | Secondary text, comments, descriptions        | scope `comment`                      |
| `warning`      | File size warnings, errors                    | scope `invalid` or red               |
| `gutter`       | Line numbers                                  | `settings.gutter_foreground`         |
| `search_match` | Search result highlighting                    | `settings.find_highlight`            |
| `selection`    | Selected / active item                        | `settings.selection`                 |

The mapping above is a starting point — the "Derived from" column shows how each role
could be automatically populated from a syntect theme. Sensible fallbacks should be
provided for themes that don't define all slots (e.g. derive from foreground with
adjusted brightness).

**How the layers work:**

1. **Syntect theme** — loaded from custom embedded `.tmTheme` files. Provides syntax
   scope colors and editor UI slots.
2. **peek theme roles** — derived automatically from the syntect theme. Provides
   semantic colors for all non-syntax UI output.
3. **All colored text output** goes through a common rendering layer that uses either
   syntect (for syntax-highlighted code) or the peek roles (for everything else).
4. **Override support** — custom peek themes could override individual roles if the
   auto-derived mapping doesn't look right for a particular syntect theme. Format and
   mechanism TBD.

This abstraction also serves as the integration point for compatibility modes (see
below) — the rendering layer can downgrade colors from 24-bit to 256/16/none regardless
of the source.

**Status: Implemented.** The `PeekTheme` struct derives semantic color roles from
the active syntect theme. All non-syntax UI output (info screens, help text, `--help`)
uses these roles via `PeekTheme::paint()`. Themes are `.tmTheme` files embedded at
compile time via `include_str!`. Gutter and search highlight roles are defined but not
yet used (line numbers and search are not implemented).

### Compatibility Modes

To support a range of terminal capabilities, the following rendering axes should be
available:

| Axis      | Modes                                                                  | Status                                                                       |
|-----------|------------------------------------------------------------------------|------------------------------------------------------------------------------|
| Color     | truecolor, 256, 16, grayscale, plain                                   | Implemented (see Color Modes above)                                          |
| Character | Full Unicode, ASCII-only (image rendering only — `--image-mode ascii`) | Image side implemented; UI/glyph fallback for non-Unicode terminals not done |

Color encoding is handled by `ColorMode` (see [Color Modes](#color-modes)) — all
callers paint with truecolor RGB and the active mode decides the on-the-wire form.
Image rendering routes through the same point of conversion via `ColorMode::write_fg`
/ `write_fg_bg`. Character compatibility is partial: `--image-mode ascii` falls back
to a luminance density ramp for terminals without block/quadrant glyphs, but the rest
of the UI (status line, info screen, etc.) still uses Unicode box-drawing and dashes.

For library-produced output (syntect), `viewer::ranges_to_escaped` replaces syntect's
hardcoded 24-bit `as_24_bit_terminal_escaped` with one routed through
`ColorMode::fg_seq`, so syntax-highlighted code is downgraded along with everything
else.

**Status: Color axis fully implemented; character-axis fallback for the UI not yet done.**


## CLI Options

Current and planned CLI options:

| Option           | Short | Description                                                   | Status       |
|------------------|-------|---------------------------------------------------------------|--------------|
| `--help`         | `-h`  | Show help screen and exit (short / long forms)                | Implemented  |
| `--version`      | `-V`  | Show version info and exit                                    | Implemented  |
| `--viewer`       | `-v`  | Force viewer mode                                             | Planned      |
| `--print`        | `-p`  | Force print mode (direct stdout)                              | Implemented  |
| `--plain`        | `-P`  | Disable syntax highlighting and pretty-printing               | Implemented  |
| `--raw`          | `-r`  | Output verbatim source (no pretty-print)                      | Implemented  |
| `--theme`        | `-t`  | Syntax highlighting theme                                     | Implemented  |
| `--color`        | `-C`  | Output color encoding (truecolor/256/16/grayscale/plain)      | Implemented  |
| `--language`     | `-l`  | Force syntax language                                         | Implemented  |
| `--width`        |       | Image rendering width in characters                           | Implemented  |
| `--image-mode`   |       | Image rendering mode                                          | Implemented  |
| `--info`         |       | Show file info instead of contents                            | Implemented  |
| `--utc`          |       | Show timestamps in UTC (default: local + offset)              | Implemented  |
| `--background`   |       | Image transparency background (auto/black/white/checkerboard) | Implemented  |
| `--margin`       |       | Image margin in transparent pixels                            | Implemented  |
| `--line-numbers` |       | Enable/disable line numbers                                   | Planned      |
| `--sizing`       |       | Image sizing mode                                             | Planned      |

`--plain` and `--raw` are orthogonal: `--raw` preserves the original file structure
(no pretty-printing) but still applies colors and font styles. `--plain` disables all
console enhancements (colors, bold, italic) but doesn't change structure. They can be
combined: `--plain --raw` gives completely unmodified file content with no styling.

`--print` / `-p` forces print mode (direct stdout). `--plain` / `-P` disables syntax
highlighting and pretty-printing.

### `--help` Screen

`-h` (short) and `--help` (long) produce two different custom-themed screens
— not the default clap-generated output:

- **`-h` (concise)** — gradient logo, version + tagline, usage line, and the
  most common options. Covers the 90% case without the wall of options.
- **`--help` (full)** — everything in `-h`, plus rarely-used options
  (theme, color, language, width, image-mode, background, margin, utc) and
  the full theme listing with the active marker. Ends with a hint to run
  `peek --help` for the longer form when only the short form is shown.

Both screens use the same gradient-painted logo (small-slant style):

```
                 __  
   ___  ___ ___ / /__
  / _ \/ -_) -_)  '_/
 / .__/\__/\__/_/\_\ 
/_/                  
```

The entire help output is styled using the active theme's colors — headings,
option names, descriptions, etc. This means `--help --theme <name>` can be
used to preview and compare themes; the help screen itself doubles as a
theme showcase.

**Status: Implemented.** Concise `-h` and full `--help` are split; both
share the gradient logo and themed rendering. Full help lists themes with
the active marker.

### `--version` Screen

`--version` / `-V` prints a single line: `peek X.Y.Z`. Unstyled, suitable
for shell scripting (`peek --version | awk ...`). The themed logo banner is
intentionally not shown here — for a styled banner with version info, use
the `-h` / `--help` screens or the `a` (about) view in the interactive
viewer.

**Status: Implemented.**


## Distribution

Release artifacts (prebuilt binaries) are published to GitHub Releases for
macOS (`aarch64`, `x86_64`), Linux (`aarch64`, `x86_64`), and Windows
(`x86_64`). A POSIX `install.sh` at the repo root fetches the right archive,
verifies its SHA256, and installs `peek` to `$HOME/.local/bin` (or
`$PEEK_INSTALL_DIR`). Windows users download the `.zip` manually. Releases
are cut by dispatching `.github/workflows/release.yml`; the workflow reads
the version from `Cargo.toml`, refuses to run if the corresponding `vX.Y.Z`
tag already exists on `origin`, and creates+pushes the tag itself.

**Status: Implemented.**


## Future / Optional Features

### Block Collapsing / Folding

Allow collapsing blocks (objects, arrays, nested structures) in the interactive viewer.
Primarily useful for structured data (JSON, YAML, TOML, XML) but could extend to code
viewers as well (folding functions, blocks, etc.).

**Challenges:** The current rendering pipeline produces a flat `Vec<String>` of
ANSI-escaped lines with no structural metadata. Implementing folding would require:

- A **line metadata layer** (fold level, block boundaries, visibility state) replacing
  the bare `String` lines.
- A **virtual line mapping** so scroll offsets work correctly with collapsed regions.
- **Preserving fold state** across re-renders (theme toggle, raw/pretty toggle).
- For structured data: retaining parsed structure or using indentation heuristics to
  detect foldable blocks.
- For code: language-aware block detection via syntect scopes (significantly harder,
  language-dependent).

Indentation-based folding for structured data (JSON/YAML) would be the most practical
starting point, since pretty-printed output has reliable indentation levels.

**Status: Idea — feasible but requires architectural changes to the rendering pipeline.**
