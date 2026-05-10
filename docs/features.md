# peek — Feature Specification

Covers what peek currently does (✅ implemented and ◐ partial). For planned and open ideas
(☐ / ❓), see [planned.md](planned.md).

Status legend: ✅ implemented · ◐ partial

## Operating Modes

### Viewer Mode ◐

Full-screen interactive console view. User exits manually (`q` / `Esc`). Keyboard interaction for
toggling options, scrolling, searching, and switching between views.

Works for all file types via the mode-stack architecture: text/source/structured `ContentMode`,
`ImageRenderMode` for raster + rasterized SVG, `AnimationMode` for GIF/WebP, plus universal
`HexMode` / `InfoMode` / `HelpMode` / `AboutMode`. Scrolling; Tab cycles the file's view modes
(content / image / SVG-source / Info — Hex, About, and Help are kept on dedicated keys); `i` jumps
straight to Info; hex (`x`); help (`h`/`?`); about (`a`); live theme cycle (`t`); color-encoding
cycle (`c`); `r` toggles raw/pretty inside the structured-data viewer. Image-specific: `b` cycles
background, `m` cycles
render mode. Animation: `p` play/pause, `n`/`N` and Left/Right step frames. `l` toggles the
line-number gutter in text views. Search not yet.

### Print Mode ◐

Direct stdout, no interactivity (`cat`-like). Default output by file type:

- **Text / source code** — syntax-highlighted (unless `--plain`)
- **Structured data** — pretty-printed + highlighted; `--raw` emits verbatim source (still
  highlighted unless `--plain`)
- **Images** — ASCII art at contain ratio
- **SVG** — rendered preview (ASCII art)
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

#### Markdown ◐

`.md` / `.markdown` / `.mdown` / `.mkd` files render as syntax-highlighted source today. The Info
view adds a Markdown section: heading counts by level (H1..H6), fenced code-block count + declared
languages, inline-code / link / image / table / list-item counts, task-list progress (`done /
total + percent`), blockquote lines, footnote definitions, frontmatter detection (YAML / TOML),
prose word count (excludes fenced code), and reading-time estimate at 230 wpm. Rendered "read mode"
(styled headings, bold, lists, per-language dispatch inside fenced code) is still planned — see
[planned.md](planned.md#markup--documentation-).

#### HTML ✅

`.html` / `.htm` / `.xhtml` files (and stdin streams that start with `<!DOCTYPE html>` or
`<html`) get a dual view:

- **Rendered** (default) — lynx-style flow rendered via the `html2text` crate: paragraph wrap to
  the terminal width, list bullets, table grid (with column sizing), numbered link references,
  and ANSI styling for `<strong>` / `<em>` / `<code>` / `<s>` / `<a>` plus author colors from
  inline `style="..."` and `<style>` rules (CSS pulled in via `use_doc_css`). Near-grayscale
  colors are filtered so author body / heading defaults don't fight the terminal's foreground.
  Tab cycles to the source view.
- **Source** — raw HTML with XML syntax highlighting via `ContentMode`.

The Info view shows the structured XML stats (root element, element counts).

#### EPUB ✅

`.epub` files (a ZIP container with HTML chapters + OPF metadata) get a three-mode view:

- **Read** (default) — one chapter at a time via the shared HTML rendering pipeline (same
  `html2text` driver as the standalone HTML viewer). `n` / `N` step forward / back through the
  spine; the status line shows `ch X/Y`. Each rendered chapter is cached at the current width so
  stepping back is instant; a terminal resize re-renders only the visible chapter. `<img>` tags
  with empty / missing `alt` get a fallback `image: <basename>` label so chapter image
  references stay visible. Cover-style chapters (almost no text + at least one image) render
  the first image as ASCII art inline so e.g. `peek book.epub` opens on the cover. The TOC view
  still exposes every container entry for general image inspection via recursive peek.
- **TOC** — the raw ZIP file tree via the existing `ListingMode`. Useful for inspecting cover
  images, stylesheets, or the OPF / NCX metadata files inside the container. Recursive peek
  (`Enter`) descends into selected entries.
- **Info** — DC metadata extracted from the OPF: title, author (`dc:creator`), language,
  publisher, date, identifier, description, plus the spine length.

Print mode (`--print` or non-TTY stdout) walks every chapter in spine order separated by blank
lines, so `peek book.epub | less` renders the whole book.

#### DOCX ✅

`.docx` files (Office Open XML — a ZIP container with `word/document.xml` body + `docProps`
metadata) get a three-mode view:

- **Read** (default) — styled body text. Headings (`Heading1..6` paragraph styles) render bold +
  themed; bold / italic / underline / strikethrough runs render via SGR; explicit run colors apply;
  bullet-list paragraphs (those carrying `numPr`) render with a `•` marker indented per `ilvl`.
  Embedded images surface inline as `[Image: <basename>]` placeholders resolved from the
  document's relationships; tables flatten to ` | `-joined rows. Width-aware word wrap re-runs on
  resize. Parsed by a hand-rolled `quick-xml` walk over `word/document.xml` (full WordprocessingML
  deserializers reject real-world Word files because numeric attributes routinely carry
  `"auto"` / `"none"` strings their strict integer types can't decode).
- **TOC** — the raw ZIP file tree via the shared `ListingMode`. Inspects the inner XML parts and
  embedded media; recursive peek (`Enter`) descends into selected entries. `--extract
  word/media/imageN.png` works as for any ZIP archive.
- **Info** — core document properties from `docProps/core.xml`: title, author, subject,
  keywords, created / modified timestamps, plus paragraph / word / image counts.

Lists currently render as flat bullets — numbering cascade resolution from `numbering.xml`
(numbered lists, nested numbering schemes) isn't done yet; everything that has a `numPr` shows
as `•`.

#### RTF ✅

`.rtf` files (Rich Text Format — control-word markup, single file, not a container) get a
single styled-text view:

- **Read** (default) — body text rendered with bold / italic / underline / strikethrough runs and
  per-run color from the RTF color table. Powered by `rtf-parser`. The metadata `\info` group is
  stripped from the body so its title / author strings don't leak into the rendered output, and
  `\par` paragraph terminators are pre-processed into explicit CRLFs (rtf-parser's lexer doesn't
  emit a token for them by default).
- **Info** — title, author, subject, keywords, plus created / revised dates pulled from the
  `\info` group, and paragraph / word counts.

There is no TOC view or per-entry extract — RTF isn't a container.

#### SQL ◐

`.sql` / `.ddl` / `.dml` / `.psql` / `.pgsql` files render as syntax-highlighted source. The Info
view adds an SQL section: heuristic dialect guess (PostgreSQL / MySQL / SQLite / T-SQL / generic),
statement count broken down by category (DDL / DML / DQL / TCL / Other), inventories of created
objects (tables, views, indexes, functions, triggers — with names), comment-line count, and a
flag when an inline `$$ … $$` PL/pgSQL block is present. The scanner tracks string / comment /
dollar-quoted state so semicolons inside strings or procedural bodies don't false-split. Real
formatter / outline mode still planned.

### Structured Data / Config Files

| Format     | Extensions          | Status |
|------------|---------------------|--------|
| JSON       | `.json`, `.geojson` | ✅      |
| JSONC      | `.jsonc`            | ✅      |
| JSON5      | `.json5`            | ✅      |
| JSON Lines | `.jsonl`, `.ndjson` | ✅      |
| YAML       | `.yaml`, `.yml`     | ✅      |
| TOML       | `.toml`             | ✅      |
| XML        | `.xml`              | ✅      |
| CSV        | `.csv`, `.tsv`      | ☐      |

JSONC and JSON5 default to **raw** (the pretty path collapses comments / JSON5 syntax, so
defaulting to it would silently lose information); `r` toggles into the strict-JSON pretty form
when needed. JSON Lines defaults to pretty: each non-empty line round-trips through serde_json
and is separated by a blank line. Pending entries (CSV/TSV) live in
[planned.md](planned.md#structured-data-additions-).

Two viewing sub-modes (toggle with `r`; CLI `--raw`):

- **Pretty** (default) — reformatted with syntax highlighting
- **Raw** — verbatim source with syntax highlighting only

`--plain` / `-P` disables all styling.

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

| Mode      | Description                                                                   |
|-----------|-------------------------------------------------------------------------------|
| `full`    | All glyphs (block, quadrant, extended)                                        |
| `block`   | Block / quadrant elements + ASCII subset                                      |
| `geo`     | Block / quadrant elements + line segments only                                |
| `ascii`   | Legacy luminance-based density ramp                                           |
| `contour` | Sobel edge detection rendered as line-art (`--edge-density` tunes line count) |

In viewer mode, Tab cycles the file's view modes (image → file info for raster; image → SVG source
→ file info for SVG). 24-bit truecolor; status line shows the active mode.

#### SVG ✅

SVG (`.svg`) is vector; the `image` crate doesn't handle it. Rasterized via `resvg`.

Two viewing modes (cycle with Tab):

- **Rendered preview** (default) — rasterize, render through the image pipeline
- **Source view** — syntax-highlighted XML (pretty or raw)

Re-renders on terminal resize.

##### SVG Animation ◐

CSS `@keyframes` animation is supported (`viewer/image/svg_anim.rs`). The parser collects each
`@keyframes` rule plus inline-style `animation-*` references on elements, builds a merged frame
timeline (one frame per stop for `steps()` timing, ~30 fps interpolated for `linear`), and
`SvgAnimationMode` rasterizes each frame on demand from a per-frame patched SVG. A bounded LRU (64
entries, keyed by `(frame, grid_cols, grid_rows)`) makes a full second loop free.

Phase 1 covers what termsvg / asciinema-svg-style files use: `transform: translateX/Y/translate`
under `steps()` or `linear` timing, inline-style targets only. SMIL (`<animate>`,
`<animateMotion>`) and class/id-selector targets are deferred. `--no-svg-anim` forces the static
render. The Info panel reports frame count, total duration, and looping vs one-shot.

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

| Mode        | Behavior                                                              |
|-------------|-----------------------------------------------------------------------|
| `Contain`   | Fit within both width and height — whole image always shown (default) |
| `FitWidth`  | Width fills the terminal; height grows freely → vertical scroll       |
| `FitHeight` | Height fills the terminal; width grows freely → horizontal scroll     |

Cycle interactively with `f` (image / SVG render views). Pipe / `--print`
output always uses `Contain` (rows are unbounded, so the other modes are
either nonsensical or reduce to `Contain`). The image is never rotated;
only the constraining axis changes.

Scroll keys in image views:

- `Up` / `Down` / `PgUp` / `PgDn` — vertical scroll under `FitWidth`
- `Left` / `Right` — horizontal scroll under `FitHeight`
- `Home` — return to top-left; `End` — jump to bottom

Toggling fit mode resets the scroll offset (the old position has no
meaning in the new grid). No `--sizing` CLI flag yet.

### Animated Images (GIF, WebP) ✅

Auto-plays at native frame rate. `p` toggles play/pause; `n`/`N` and Left/Right step frames; `b`
cycles background. Status line shows frame counter and play/pause. Print mode renders the first
frame. Frame count appears in the file info screen. Transparency handling applies.

### Binary and Archive Files ◐

For files peek doesn't have a specialized viewer for — executables, fonts — the baseline shows
the **file info screen**:

- File type / MIME (detected via magic bytes through the `infer` crate)
- Size (exact + human-readable)
- Filesystem metadata (permissions, timestamps)

`infer` provides MIME only — no deeper metadata. Format-specific details (executable
architecture, font tables) could be added later with dedicated parsers.

Binary files open in the hex-dump viewer by default (`hexdump -C`-style, terminal-width aware,
streaming via `ByteSource`). File info reachable via Tab / `i` from within hex, and via `--info`.
`--plain` / `-P` still uses hex for binary. No format-specific deep metadata yet.

#### Archive Listing ◐

Container archives open in a **TOC view** — one row per entry with permissions, uncompressed
size, mtime, and path. Listing reads only the per-entry headers, so multi-GB archives open
instantly. Up/Down move a file-selection cursor (skipping directories), Top/End jump to the
first / last file, PgUp/Dn page-scroll then snap selection to the first visible file. The
selected leaf gets a highlighted background + arrow marker. `e` extracts the selected entry —
see [Extraction](#extraction-) below. Tab cycles TOC ↔ Info; `x` still drops into the raw hex
dump of the archive bytes.

| Format      | Extensions                     | Status |
|-------------|--------------------------------|--------|
| ZIP         | `.zip`, `.jar`, `.war`, `.apk` | ✅      |
| Tar         | `.tar`                         | ✅      |
| Tar + gzip  | `.tar.gz`, `.tgz`              | ✅      |
| Tar + bzip2 | `.tar.bz2`, `.tbz2`            | ✅      |
| Tar + xz    | `.tar.xz`, `.txz`              | ✅      |
| Tar + zstd  | `.tar.zst`, `.tzst`            | ✅      |
| 7-Zip       | `.7z`                          | ✅      |
| RAR         | `.rar`                         | ☐ planned |

Info view shows entry / file / directory counts and total uncompressed size. Listing failures
(corrupt archive, unsupported variant) surface as a warning row and the TOC view is empty.

A **sticky parent breadcrumb** pins the current top row's ancestor chain to the upper rows of the
viewport when scrolled — so even mid-tree the path back to root stays visible. Same TOC code path
serves disk-image listings, so the behavior matches there too. Capped to one third of the viewport
height, suppressed when scroll is at the top or the top row is a top-level entry. Toggle with `s`;
when off the status bar shows `sticky off`.

#### Disk Images ✅

| Format | Extensions | Status                                                  |
|--------|------------|---------------------------------------------------------|
| ISO    | `.iso`     | ✅ PVD metadata + recursive directory listing (Joliet)   |
| DMG    | `.dmg`     | ✅ UDIF trailer-only (no partition map walk yet)        |

**ISO 9660** opens to a **TOC view** (the same tree-style listing archive containers use): one row
per file/directory with size, mtime, and 8.3 / Joliet name; depth tracked by indented tree glyphs.
The walker reads the root directory extent from the PVD (or SVD, if Joliet is present — preferred
for longer Unicode names) and recurses through child extents. Per-entry permissions are not
surfaced because Rock Ridge SUSP fields aren't parsed; the renderer falls back to typical defaults
(`rwxr-xr-x` for dirs, `rw-r--r--` for files). Bounded depth + entry caps defend against malformed
images.

ISO **metadata** also remains on the info screen (`i`): volume label, volume set, system ID,
publisher, data preparer, application, volume size in blocks, and the four PVD timestamps
(creation / modification / expiration / effective). Joliet extension and El Torito boot record
presence are surfaced from the descriptor walk.

**DMG** opens straight to the file info screen — there's no listing path because the inner
filesystem (HFS+ / APFS / FAT) would need its own walker.

**Apple Disk Image (UDIF)** metadata comes from the 512-byte "koly" trailer at the end of the
file: UDIF version, image variant (device / partition / mounted system), total uncompressed size,
data-fork length, embedded XML partition-map size, segment number / count, data + master checksum
algorithms, and the documented trailer flag bits (flattened, internet-enabled). The XML partition
map itself isn't parsed yet; it shows up as a presence + size row.

Both parsers are hand-rolled — no extra crate. Hex view (`x`) still works on the raw image bytes.

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

All callers paint truecolor RGB; the `StyleMode` enum on `PeekTheme` owns the conversion and is the
single point where the encoding is decided. Image rendering routes the same way via
`StyleMode::write_fg` / `write_fg_bg`. Plain mode emits text content with zero ANSI escapes (no SGR
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
- **Markdown** — heading counts per level, fenced code-block count + languages, inline code, links,
  images, tables, list items, task progress, blockquote lines, footnotes, frontmatter kind, prose
  word count, reading-time estimate
- **SQL** — dialect guess, statement count by category (DDL/DML/DQL/TCL), created-object inventory
  (tables, views, indexes, functions, triggers), comment-line count, PL/pgSQL block flag
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

### Line Numbers ✅

Toggleable line numbers for text-based views (ContentMode: source, structured raw/pretty, plain
text, SVG XML). Off by default; `--line-numbers` / `-n` enables at startup, `l` toggles in the
viewer. Gutter is right-aligned with a minimum width of 2 digits and painted in the theme's gutter
color. In pretty mode the numbers count visible pretty-printed lines (the lines actually shown), not
source byte lines.

### Line Wrapping ✅

Soft wrap on by default for ContentMode (text, source, structured pretty/raw, SVG XML). Each
visible logical line is sliced into visual rows of width `term_cols - gutter_width` via
`wrap_styled`, so the row budget accounts for wrapped continuations and the status line never
scrolls out of view.

Toggle with `w`. Vertical scroll (`j`/`k`, PgUp/PgDn, Home/End) moves one **visual row** at a time
when wrap is on — long lines no longer make a single keypress jump over all their wrapped rows.
The line-number gutter shows the real (logical) line number on the first segment; continuation
rows have a blank gutter of the same width so wrapped content aligns under its first row.

Status bar shows `Wrap` only when wrap is on (default-on convention; absence means "off").

### Horizontal Scrolling ✅

Companion to wrap-off mode: `Left` / `Right` pan the viewport horizontally by 8 columns per
press (`less -S` feel). Active only when wrap is off — wrap-on makes Left/Right inert because
content is already fully visible. The gutter does not pan; it stays anchored to the left edge.

### Help Screen ◐

`h` / `?` opens the help screen. Shows keyboard shortcuts and the active theme. Shortcut list is
composed per file type from the union of global actions and each loaded mode's extras — so an SVG
file's help shows the background-cycle shortcut, while a JSON file's doesn't. Per-active-mode
filtering (showing only the active mode's extras) not yet done.

### About Screen ✅

`a` shows the gradient peek logo, version, tagline, the active theme's full palette as colored
swatches, and a short list of pointers (homepage, license, common keys). Doubles as a theme
showcase — cycling themes with `t` while on About previews how each theme paints the full palette.

### Extraction ✅

Pull an inner item out of a container as a standalone file. Three sources currently:

- **Archive entries** (`.zip`, `.tar[.gz|.bz2|.xz|.zst]`, `.7z`): extract a single file by its
  inner path. Phase 1 always decompresses into memory with a 256 MB cap.
- **ISO entries** (`.iso`): extract a single file via a zero-copy `FileRange` view over the
  backing image — no decompression, no buffering, multi-GB ISOs unaffected.
- **Animation frames** (`.gif`, `.webp`, animated SVG): extract a single composited frame as a
  PNG at the source's native pixel size (SVG sub-512px scales up to 512 on the longest axis;
  override with `--extract-size`).

CLI: `peek <file> --extract <KEY> [-o PATH]`. `<KEY>` is an entry path for archives/ISOs or a
1-based frame index for animations. `-o PATH` overrides the suggested filename; `-o -` or piping
stdout streams raw bytes. Adding `--print` or `--info` instead replaces the active source with
the extracted item and runs the rest of the pipeline against it — that's recursive peek
(`peek archive.zip --extract foo.py --print` syntax-highlights the inner file).

Viewer: in a listing TOC, `e` extracts the selected file; in an animation, `e` extracts the
current frame. Either way a status-line prompt opens prefilled with the suggested filename —
Esc cancels, Enter writes. Path safety rejects traversal (`..`) before any TOC lookup.

DMG extract is intentionally unsupported — UDIF block decompression is a separate effort.

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
| `e`                   | Extract selected entry / current frame |

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

| Key              | Action                                              |
|------------------|-----------------------------------------------------|
| `m`              | Cycle rendering mode (full/block/geo/ascii/contour) |
| `b`              | Cycle background (auto/black/white/checkerboard)    |
| `f`              | Cycle fit mode (Contain / FitWidth / FitHeight)     |
| `Left` / `Right` | Pan horizontally (FitHeight)                        |
| `+` / `=`        | Zoom in (planned)                                   |
| `-`              | Zoom out (planned)                                  |

### Animated Image Views *(context: GIF, WebP)*

| Key              | Action                                          |
|------------------|-------------------------------------------------|
| `p`              | Play / pause animation                          |
| `n` / `N`        | Next / previous frame                           |
| `f`              | Cycle fit mode (Contain / FitWidth / FitHeight) |
| `Left` / `Right` | Pan horizontally under `FitHeight`              |
| `b`              | Cycle background                                |
| `m`              | Cycle render mode                               |

`Left` / `Right` are pan keys in both static and animated image views — frame stepping uses
`n` / `N` exclusively (the previous Left/Right frame-step bindings are gone).

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

Color is handled by `StyleMode` — all callers paint truecolor RGB and the active mode decides the
wire form. Image rendering routes through the same point via `StyleMode::write_fg` / `write_fg_bg`.
Character compatibility is partial: `--image-mode ascii` falls back to a luminance density ramp for
terminals without block/quadrant glyphs, but the rest of the UI (status line, info screen) still
uses Unicode box-drawing and dashes.

For library-produced output (syntect), `viewer::ranges_to_escaped` replaces syntect's hardcoded
24-bit `as_24_bit_terminal_escaped` with one routed through `StyleMode::fg_seq`, so
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

