# peek — Feature Specification

## Operating Modes

peek has two operating modes: **viewer mode** and **print mode**.

### Viewer Mode

Full-screen interactive console view. The user must manually quit (e.g. `q`, `Esc`)
to exit. Supports keyboard interaction for toggling options, scrolling, searching, and
switching between views.

**Status: Partially implemented.** Interactive viewing works for all file types with
scrolling (Up/Down/j/k, PgUp/PgDn, Home/End), Tab/i view switching (content ↔ file
info), help screen (h/?), live theme cycling (t), and raw/pretty toggle (r) for
structured data. No search, line numbers, or image-specific keybindings yet.

### Print Mode

Outputs data directly to the console and exits, similar to `cat`. No interactivity.

Default print mode output by file type:
- **Text / source code:** syntax-highlighted content (unless `--plain`).
- **Structured data:** pretty-printed with syntax highlighting by default. `--raw`
  outputs verbatim source (still highlighted unless `--plain`).
- **Images:** ASCII art at contain ratio.
- **SVG:** rendered preview (ASCII art).
- **Documents:** extracted text content.
- **Binary / unknown:** file info (type, size, metadata).

**Status: Partially implemented.** Direct stdout output works when `--print` / `-p` is
set or stdout is not a TTY.

### Mode Selection

- `--viewer` / `-v` forces viewer mode.
- `--print` / `-p` forces print mode.
- **Default behavior:** if output is longer than the console size, start in viewer mode;
  otherwise use print mode.
- All data types should support both modes where it makes sense — the same content
  should be viewable interactively or printable to stdout.

**Status: Partial.** TTY detection and `--print` / `-p` implemented.
No content-length-based auto-selection yet (currently: TTY → viewer, non-TTY → print).

### Input

peek operates on a single file at a time. The file path is given as a positional
argument. Reading from stdin is supported by passing `-` as the file path.

**Status: Partially implemented.** Single file works. Multiple files are supported
sequentially. Stdin (`-`) is declared but not yet functional.


## Supported File Types

The following file types should be supported. This list is not exhaustive — additional
types may be added over time.

### Source Code

All standard programming languages supported by the syntax highlighting library
(syntect). This covers a wide range including but not limited to: Rust, Python,
JavaScript, TypeScript, C, C++, Java, Go, Ruby, Shell, etc.

Viewing features:
- Syntax-colored source with theme support.
- Toggleable line numbers.

**Status: Implemented** via syntect's built-in language definitions. Line numbers not
yet implemented.

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

**Status: Partially implemented.** Four modes work with 24-bit truecolor. Interactive
image viewer supports resize. Tab/i view switching to file info works. No `m` mode
cycling in viewer yet (only via `--image-mode` CLI).

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
(dark content → white bg, light content → black bg). Manual background selection via
keyboard or CLI (`--background`) is not yet implemented.

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

### Animated GIFs

Animated GIFs should be playable in viewer mode — render each frame as ASCII art and
cycle through them at the original frame rate. Since the image rendering pipeline
already exists, this is primarily a matter of extracting frames and timing the playback
loop.

Viewer mode should also support a single-frame mode where playback is paused and the
user can step through frames using arrow keys or `n`/`N` (the latter also works when
zoomed in, where arrow keys are used for panning).

In print mode, a reasonable default would be to render the first frame.

Transparency handling (see above) applies to animated GIFs as well.

**Status: Planned.**

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

**Status: Partially implemented.** File info screen shows MIME type, file size, and
filesystem metadata for all file types via `--info` flag and Tab/i in the interactive
viewer. No format-specific deep metadata (archive listing, executable info) yet.


## Viewer Features

Cross-cutting features available in viewer mode regardless of file type.

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
It shows keyboard shortcuts and the currently active theme. Context-specific commands
(per file type) are not yet shown.


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

| Key       | Action                                     |
|-----------|--------------------------------------------|
| `Tab`     | Toggle content / file info               |
| `i`       | Jump to file info screen                   |
| `h` / `?` | Toggle help screen                         |
| `t`       | Cycle theme                                |

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

### Animated GIF Views *(context)*

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
- Default theme: `islands-dark`.
- Three custom embedded themes in `.tmTheme` format, compiled into the binary:
  - **islands-dark** — JetBrains Islands-inspired dark theme (default)
  - **dark-2026** — VS Code Dark 2026-inspired theme
  - **vivid-dark** — High-contrast dark theme with vivid colors
- Themes can be cycled live in the interactive viewer with `t`.

**Status: Implemented.** Theme selection works via CLI/env var. Three custom embedded
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

To support a range of terminal capabilities, the following rendering modes should be
available:

| Mode              | Colors             | Characters    |
|-------------------|--------------------|---------------|
| True color        | 24-bit RGB         | Full Unicode  |
| 256-color         | 256 ANSI colors    | Full Unicode  |
| Low color         | 16 ANSI colors     | Full Unicode  |
| No special chars  | 24-bit / 256 / 16  | ASCII only    |
| Black and white   | No color           | ASCII only    |

These modes affect all text rendering — syntax highlighting, image rendering, and UI
elements. The common colored text output layer (from the theme architecture above)
handles the downgrading: all output is authored in full 24-bit color via the theme
roles, and the active compatibility mode decides how to emit it to the terminal.

For library-produced output (syntect), the pipeline may need to intercept and remap
colors after syntect renders them, since syntect always outputs 24-bit ANSI codes.

**Status: Not implemented.** Currently assumes 24-bit truecolor throughout.


## CLI Options

Current and planned CLI options:

| Option           | Short | Description                                     | Status       |
|------------------|-------|-------------------------------------------------|--------------|
| `--help`         | `-h`  | Show help screen and exit                       | Implemented  |
| `--version`      |       | Show version info and exit                      | Implemented  |
| `--viewer`       | `-v`  | Force viewer mode                               | Planned      |
| `--print`        | `-p`  | Force print mode (direct stdout)                | Implemented  |
| `--plain`        | `-P`  | Disable syntax highlighting and pretty-printing | Implemented  |
| `--raw`          | `-r`  | Output verbatim source (no pretty-print)        | Implemented  |
| `--theme`        | `-t`  | Syntax highlighting theme                       | Implemented  |
| `--language`     | `-l`  | Force syntax language                           | Implemented  |
| `--width`        |       | Image rendering width in characters             | Implemented  |
| `--image-mode`   |       | Image rendering mode                            | Implemented  |
| `--info`         |       | Show file info instead of contents              | Implemented  |
| `--background`   |       | Image transparency background (auto/black/white/checkerboard) | Implemented |
| `--margin`       |       | Image margin in transparent pixels              | Implemented  |
| `--line-numbers` |       | Enable/disable line numbers                     | Planned      |
| `--sizing`       |       | Image sizing mode                               | Planned      |
| `--color-mode`   |       | Select compatibility/color mode                 | Planned      |

`--plain` and `--raw` are orthogonal: `--raw` preserves the original file structure
(no pretty-printing) but still applies colors and font styles. `--plain` disables all
console enhancements (colors, bold, italic) but doesn't change structure. They can be
combined: `--plain --raw` gives completely unmodified file content with no styling.

`--print` / `-p` forces print mode (direct stdout). `--plain` / `-P` disables syntax
highlighting and pretty-printing.

### `--help` Screen

The `--help` / `-h` output should be a custom-designed help screen (not the default
clap-generated output). It should include, in order:

1. **ASCII art "peek" logo** — a stylized text banner, colored using the active theme's
   palette (e.g. as a gradient across the letters). Serves double duty as a theme
   preview. Logo candidates:

   Option A — small slant (lightweight, clean):
   ```
                    __  
      ___  ___ ___ / /__
     / _ \/ -_) -_)  '_/
    / .__/\__/\__/_/\_\ 
   /_/                  
   ```

   Option B — chunky (compact, more weight):
   ```
                       __    
   .-----.-----.-----.|  |--.
   |  _  |  -__|  -__||    < 
   |   __|_____|_____||__|__|
   |__|                      
   ```
2. **Version** — read from `Cargo.toml` at build time.
3. **Brief description** — one or two sentences about what peek does.
4. **CLI option reference** — formatted list of all options with descriptions.

The entire help output should be styled using the active theme's colors — headings,
option names, descriptions, etc. This means `--help --theme <name>` can be used to
preview and compare different themes. The help screen itself becomes a theme showcase.

**Status: Implemented.** Custom themed help screen with gradient logo, version,
description, options reference, and theme listing with active marker.

### `--version` Screen

The `--version` output shows a subset of the `--help` screen:

1. **ASCII art "peek" logo** — same gradient-colored logo as `--help`.
2. **Version** — `peek v{VERSION}` in heading color.
3. **Brief description** — one-line description in foreground color.

No usage, options, or theme listing is shown. Like `--help`, it respects
`--theme <name>` for previewing different color schemes.

**Status: Implemented.**
