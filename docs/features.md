# peek — Feature Specification

Engineering-detail reference for what peek currently does (✅ implemented and ◐ partial). The
user-facing manual ([`manual/`](../manual/)) covers the same ground in less detail and without
status markers — update both when a feature changes. For planned and open ideas (☐ / ❓), see
[planned.md](planned.md).

Status legend: ✅ implemented · ◐ partial

## Contents

- [Operating Modes](#operating-modes)
- [Supported File Types](#supported-file-types)
    - [Source Code](#source-code-)
    - [Structured Data / Config Files](#structured-data--config-files)
    - [Image Files](#image-files-)
    - [Audio Files](#audio-files-)
    - [Animated Images (GIF, WebP)](#animated-images-gif-webp-)
    - [Comic Archives](#comic-archives-)
    - [Object Files](#object-files-)
    - [Java Classfiles](#java-classfiles-)
    - [SQLite Databases](#sqlite-databases-)
    - [Certificates and Keys](#certificates-and-keys-)
    - [Fonts](#fonts-)
    - [`.DS_Store`](#ds_store-)
    - [Binary and Archive Files](#binary-and-archive-files-)
- [Viewer Features](#viewer-features)
- [Keyboard Shortcuts](#keyboard-shortcuts)
- [Color and Rendering](#color-and-rendering)
- [CLI Options](#cli-options)
- [Distribution](#distribution-)

## Operating Modes

### Viewer Mode ✅

Full-screen interactive console view. User exits manually (`q` / `Esc`). Keyboard interaction for
toggling options, scrolling, searching, and switching between views.

Works for all file types via the mode-stack architecture: text/source/structured `ContentMode`,
`ImageRenderMode` for raster + rasterized SVG, `AnimationMode` for GIF/WebP, plus universal
`HexMode` / `InfoMode` / `HelpMode` / `AboutMode`. Scrolling; Tab cycles the file's view modes
(content / image / SVG-source / Info — Hex, About, and Help are kept on dedicated keys); `i` jumps
straight to Info; hex (`x`); help (`h`/`?`); about (`a`); live theme cycle (`t`); color-encoding
cycle (`c`); `r` toggles raw/pretty inside the structured-data viewer. Image-specific: `b` cycles
background, `m` cycles
render mode. Animation: `Space` play/pause, `n`/`p` and Left/Right step frames. `l` toggles the
line-number gutter and `w` toggles soft wrap in text views. Text search (`/` opens the prompt, `n`/
`p` cycle matches) works in the text / source / structured views.

### Print Mode ✅

Direct stdout, no interactivity (`cat`-like). Default output by file type:

- **Text / source code** — syntax-highlighted (unless `--plain`)
- **Structured data** — pretty-printed + highlighted; `--raw` emits verbatim source (still
  highlighted unless `--plain`)
- **Images** — ASCII art at contain ratio
- **SVG** — rendered preview (ASCII art)
- **Binary / unknown** — hex dump (streaming, `hexdump -C` layout, terminal-width aware)

Active when `--print` / `-p` is set or stdout isn't a TTY.

### Mode Selection ◐

- `--print` / `-p` forces print.
- **Default:** if stdout is a TTY, viewer; else print.
- **Binary / unknown** files open in the hex-dump viewer when interactive; piped binary streams
  a hex dump.
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

Config files highlight through the same path — no dedicated mode. `.ini` / `.cfg` / `.conf` /
`.properties` / `.env` / `.hcl` / `.tf`, plus by-name matches (`Makefile`, `Dockerfile`,
`.gitignore`, `.editorconfig`, …) resolve to their grammar via syntect's extension/name lookup.
Filename-keyed special cases fill the gaps where the extension misleads or is absent:
`.env.local` / `.env.production` / `.envrc` → DotENV, `justfile` → Makefile (closest grammar; no
Just definition exists), `.dockerignore` → Git Ignore. Names with no grammar (`.dhall`, `.cue`)
fall back to plain text.

Extensionless `#!` scripts (e.g. `postinst`, `configure`, git hooks) are detected by their
shebang line and routed to the matching grammar — the interpreter name (`sh`, `bash`, `python`,
`perl`, `ruby`, …) selects the syntax, including `env`-style shebangs (`#!/usr/bin/env python`).

Features: syntax-colored source with theme support; toggleable line numbers (✅, `--line-numbers` /
`-n` / `l`).

#### Markdown ✅

`.md` / `.markdown` / `.mdown` / `.mkd` / `.mkdn` / `.mdwn` files get a dual view:

- **Rendered** (default) — pulldown-cmark drives a CommonMark + GFM walker that emits
  width-wrapped, ANSI-styled text. Styled headings (H1 / H2 underlined with `═` / `─`), bullet
  and ordered lists with hanging indent, task lists (`☐` / `✓`), blockquote rail (`▍`),
  horizontal rules, GFM tables as box-drawing (per-column alignment from the header separator
  row, proportional shrink when the row exceeds available width), fenced code blocks
  syntect-highlighted by their declared language (falls back to full foreground-on-surface plain
  when language doesn't resolve), emphasis / strong / strikethrough as SGR attributes, inline code
  as full foreground-on-surface (boxed span), links
  (underlined + dim URL after), images (`[image: alt] (url)`), footnote references and
  definitions, and frontmatter (YAML `---` / TOML `+++`) stripped to a dim verbatim block at the
  top.
- **Source** — syntax-highlighted markdown source via `ContentMode`. Reachable with Tab. Becomes
  the entry view with `--raw`. `--plain` drops the rendered view.

The Info view adds a Markdown section: heading counts by level (H1..H6), fenced code-block count +
declared languages, inline-code / link / image / table / list-item counts, task-list progress
(`done / total + percent`), blockquote lines, footnote definitions, frontmatter detection (YAML /
TOML), prose word count (excludes fenced code), and reading-time estimate at 230 wpm.

#### Jupyter Notebook ✅

`.ipynb` files (JSON of cells) get three views:

- **Rendered** (default) — the notebook is translated to one Markdown document and rendered
  through the shared Markdown pipeline: markdown cells as prose, code cells as `In [n]:`-labelled
  fenced blocks syntect-highlighted in the kernel language, and outputs below each code cell —
  stream / `text/plain` results as fenced text, `error` outputs as a bold `ename: evalue` header
  plus the ANSI-stripped traceback, image outputs (`image/png` etc.) noted (inline ASCII rendering
  is a follow-up). nbformat 4 and the older nbformat-3 `worksheets` layout both parse.
- **Source** — the raw notebook JSON via the generic structured content mode (pretty-printed, `r`
  toggles raw). Reachable with Tab; becomes the entry view with `--raw`. `--plain` drops the
  rendered view.
- **Blocks** — a flat listing TOC of every code cell and image output, numbered in document order
  with readable synthetic names (`code-1.py`, `image-1.png`, …) instead of the notebook's opaque
  cell ids. Each row is extractable (`x`, or `--extract code-1.py`, writes the Python source or the
  decoded image bytes) and descendable (Enter recurses into peek over an in-memory copy — code
  opens syntax-highlighted, images render as ASCII). `--list` prints the block names + sizes. Image
  outputs are base64 in the file; peek decodes them (via the hand-rolled `crate::base64`) only at
  extract / descend time, so the render path never materialises image bytes.

The Info view adds a Notebook section: nbformat version, kernel display name, language + version,
cell count (code / markdown / raw split), output count (with image / error sub-counts), and the
highest execution count.

Detection is by `.ipynb` extension or by JSON carrying both `nbformat` and `cells` keys (so
notebooks piped via stdin route to the cell viewer rather than the generic JSON pretty-printer).

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

`html2text` holds the whole document in memory, so HTML over a 16 MB render cap
(`RENDER_MAX_BYTES`) skips the rendered view — it shows one warning line and the **Source** view
stands in. Keeps a pathological multi-hundred-MB page openable.

#### Email ◐

`.eml` (single RFC822 / MIME message) and `.mbox` (concatenated mailbox) parse via the pure-Rust
`mail-parser` crate. Detection works by extension and by content sniff — a leading `From ` line
(mbox) or an RFC822 header block carrying a recognised mail header (eml) — so extension-less
messages still route here.

**`.eml`** composes:

- **Message** (default) — a themed header block (From / To / Cc / Date / Subject, plus an
  attachment count when present) followed by the body. When the message carries an HTML part it
  renders through the same `html2text` driver as the HTML viewer; otherwise the plain-text part
  is word-wrapped to the viewport. `RenderedTextMode` caches the result per width / theme.
- **Source** — the raw RFC822 text via `ContentMode`. `--plain` drops the rendered view and
  opens straight on the source.
- **Attachments** (when present) — a listing over the message's MIME attachments, showing each
  part's **content type** alongside size and name; `e` extracts one to disk through the standard
  extract pipeline (`message/rfc822` → recursive peek on the saved part).

**`.mbox`** composes a **Messages** TOC (one row per message, prefixed with its index so
duplicate subjects stay distinct, with the message `Date` in the mtime column) over a hand-rolled
`From `-separator splitter. `Enter` descends
into the selected message — a zero-copy `InputSource::subrange` of just that message, viewed with
the same Message / Attachments / Info / hex / help stack as a standalone `.eml` (minus the
per-message raw Source view; the mailbox's own raw text is the secondary top-level view) — so even
a multi-GB mailbox lists instantly and only the opened message is parsed.

The Info view shows the header summary and attachment count + total size (`.eml`) or the message
count (`.mbox`).

#### Calendar and Contacts ✅

iCalendar (`.ics` / `.ical` / `.ifb`) and vCard (`.vcf` / `.vcard`) are the two IETF "vObject"
text formats. They share one content-line grammar (RFC 5545 §3.1 / RFC 6350 §3.3 — line folding,
`NAME;PARAM=VALUE:VALUE` properties, `BEGIN`/`END` component nesting), so both parse through a
single hand-rolled parser (`types/vobject/line.rs`) with no added dependency; only the renderer
diverges. Detection is by extension and by content sniff — a leading `BEGIN:VCALENDAR` /
`BEGIN:VCARD` marker — so extension-less files (and stdin) still route here.

Both flavours compose a **rendered** read view (default) followed by the raw **Source** via
`ContentMode`; `--plain` drops the rendered view and opens on the source. The rendered view goes
through `RenderedTextMode`, so it's cached per width / theme.

- **iCalendar** renders an agenda: an optional calendar name header, then one block per `VEVENT` /
  `VTODO`. Each event shows a summary heading, a human-formatted date-time span (same-day timed
  ranges collapse the redundant end date; `TZID` is shown verbatim — no zone conversion),
  location, a humanised `RRULE` (`Weekly on Mon, Tue, …, 40 times`), organizer + attendees,
  status, categories, link, and a wrapped description. Todos show due date and status +
  percent-complete. Info: calendar name, event / todo counts, event date range, version, product.
- **vCard** renders one grouped card per `VCARD`: display name (`FN`, or composed from the
  structured `N`), nickname, org (`Company — Department`), title, every email / phone / address
  with its `TYPE` annotation, web, birthday, categories, and a wrapped note. v3 and v4 cards are
  handled together (v4 `tel:` URIs and quoted multi-`TYPE` params included). Info: contact count
  + the leading card's version.

Date-times reformat to a readable `YYYY-MM-DD HH:MM` without a date crate — the values are already
calendar fields, so it's string reshaping, not time math. No inner items to extract.

#### EPUB ✅

`.epub` files (a ZIP container with HTML chapters + OPF metadata) get a three-mode view:

- **Read** (default) — one chapter at a time via the shared HTML rendering pipeline (same
  `html2text` driver as the standalone HTML viewer). `n` / `p` step forward / back through the
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

A `word/document.xml` over the 16 MB render cap (`RENDER_MAX_BYTES`, gated on the *uncompressed*
entry size so a zip-bombed body is caught before extraction) drops the **Read** view; the **TOC**
+ Info + hex views stand in, with the reason surfaced as a TOC warning. The gate lives in the
shared `read_zip_entry` helper (`types/archive/reader.rs`), so ODT (`content.xml`), EPUB chapter
/ image reads, CBZ page reads, and spreadsheet doc-props go through the same cap; RTF and HTML
apply it to the whole source.

#### ODT ✅

`.odt` files (OpenDocument Text — a ZIP container with `content.xml` body + `meta.xml` Dublin
Core metadata) get the same three-mode view as DOCX, backed by a shared AST + renderer in
`crates/peek-types/src/types/document/{ast,render,renderer}` plus the generic `RenderedTextMode`.
The per-format parser is the only piece that differs.

- **Read** (default) — styled body text. Headings (`<text:h text:outline-level="N">`) render
  bold + themed; bold / italic / underline / strikethrough / colored runs render via SGR. Span
  styling is resolved through `<office:automatic-styles>`: `<text:span text:style-name="T1">`
  picks up the run-style attrs that the automatic-styles block defines for `T1` (`fo:font-weight`,
  `fo:font-style`, `style:text-underline-style`, `style:text-line-through-style`, `fo:color`).
  Bulleted-list rendering uses `<text:list>` nesting depth for indent and a `•` marker on each
  `<text:list-item>`. `<draw:image>` references inside `<draw:frame>` surface as
  `[Image: <basename>]` placeholders. Hyperlinks (`<text:a>`) force-underline their inner runs.
  Tables flatten to ` | `-joined rows. Width-aware word wrap re-runs on resize.
- **TOC** — the raw ZIP file tree via the shared `ListingMode`, exactly as for DOCX. `--extract
  Pictures/foo.png` works as for any ZIP archive.
- **Info** — title, author, subject, keywords (multi-valued `<meta:keyword>` entries
  comma-joined), description, created / modified timestamps, plus paragraph / word / image
  counts.

`styles.xml` (the package's separate named-styles container) is intentionally not consulted in
v1 — real-world ODTs from LibreOffice / OpenOffice dump all directly-used styling into
content.xml's automatic-styles, and inheritance chains from styles.xml only matter for the
small fraction of files that rely on them.

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

RTF opens to a single Read view by default. When the file embeds images as `\pict` groups,
a synthetic TOC of those embeds is pushed alongside Read; `e` / `--extract` pulls one out
through the recursive-peek pipeline. Plain RTFs without embeds stay single-view.

#### PDF / Adobe Illustrator ✅

`.pdf` files (Portable Document Format — binary container with paged content, optional
attachments, and a metadata dict) get a multi-mode view powered by Pdfium. `.ai` files
(Adobe Illustrator, CS2/2005 onwards) are PDF 1.x internally — the default "Create PDF
Compatible File" save embeds a full PDF rendering — so they route through the same Pdfium
stack. The `.ai` extension selects an Illustrator flavour that labels the Info section
"Adobe Illustrator" and accepts `.ai` over `%PDF` magic without an extension-mismatch
warning; the render path is identical. (Legacy pre-CS2 `.ai` is pure PostScript and is not
yet supported — see [planned.md](planned.md).)

- **Read** (default) — paged image render. Each page is rasterized via Pdfium and ASCII-rendered
  through the shared image pipeline (same `prepare_decoded` / `render_prepared` path the
  comic-archive reader uses). `n` / `p` step pages, the status line shows `page X/Y`. Per-page
  cache keyed by `(cols, rows, style, image-mode, background, fit)`; resizing or cycling
  background / image mode / fit re-renders only the visible page.
- **Text** — width-wrapped text extraction across the whole document, separated by muted
  `--- Page N ---` markers. Same caching shape as DOCX / RTF (single `(width, style_mode)`
  cache rebuilt on resize). Reachable via Tab. Only present when the document actually carries
  a text layer — image-only scans and outlined-vector artwork (`.ai`) extract nothing, so the
  tab is skipped rather than shown empty (the first few pages are probed at compose time).
- **Embeds** — when the PDF carries `/EmbeddedFiles` attachments, a `ListingMode` of those
  attachments. `e` / Enter extracts the selected attachment as an `InputSource::Memory` that
  re-detects through the recursive-peek pipeline (an attached CSV opens in a CSV view, an
  attached image opens in the image viewer, and so on). Hidden when no attachments are present.
- **Info** — PDF version (`1.4`, `1.7`, …), title, author, subject, keywords, creation /
  modification dates (PDF `D:YYYYMMDDHHMMSSO…` strings reformatted to `YYYY-MM-DD HH:MM:SS UTC`
  / `±HH:MM`), page count, attachment count, inline-image count.

Print mode (`--print`) walks every page in order separated by blank lines. `cat file.pdf | peek`
detects the `%PDF-` magic and routes to the PDF mode stack (a piped `.ai` lands here too,
labelled as plain PDF — the Illustrator flavour is only recovered from the `.ai` extension).

Pdfium is loaded dynamically from `libpdfium.dylib` / `.so` / `.dll` shipped alongside the
peek binary in the release tarball — no system install required at runtime. Encrypted /
password-protected PDFs surface the open error in the Info section instead of crashing.

#### EPS / PostScript ✅

`.eps` / `.ps` files (Encapsulated PostScript and plain PostScript). PostScript is a *program*,
not a data file, so peek offers up to two image views plus source + metadata, composed from
whatever's available:

- **Preview** (default when present) — binary "DOS-EPS" containers (magic `C5 D0 D3 C6`) carry
  a 30-byte header pointing at an embedded TIFF/WMF preview the designer baked in. peek extracts
  the TIFF preview and renders it through the shared image pipeline — instant, no interpreter.
  (WMF previews are named in Info but not rendered; no pure-Rust WMF rasteriser.)
- **Render** — true Ghostscript rasterisation, present only when a `gs` interpreter is found on
  PATH. Renders page 1 (`-dEPSCrop` for EPS, full page for `.ps`) at 150 DPI through the image
  pipeline. Lazy: the subprocess only spawns when the tab is actually viewed — opening a file
  never blocks on Ghostscript. Ghostscript is **never bundled** (AGPL/GPL + large C dependency);
  it's an optional runtime enhancement.
- **Source** — the PostScript program text. For a binary DOS-EPS, the PostScript section is
  sliced out of the container so the source view shows the program, not raw binary.
- **Info** — DSC header fields (`%%Title`, `%%Creator`, `%%CreationDate`, `%%For`,
  `%%BoundingBox`, `%%LanguageLevel`, `%%Pages`), the embedded-preview descriptor (kind +
  decoded dimensions), and Ghostscript availability (with an install hint when absent).

A preview-less `.eps` / `.ps` with no `gs` on PATH degrades cleanly to Source + Info. Detection
covers the `.eps` / `.epsf` / `.epsi` / `.ps` extensions, the DOS-EPS magic, and a `%!PS…`
content sniff (`EPSF` in the version line picks EPS over plain PostScript).

The single-bitmap render path (decode → fit → window-crop → ASCII, with zoom/pan) is shared with
the PDF and CBZ page renderers via `viewer::paged::render_image_window`.

Not yet: legacy pre-CS2 Illustrator labelling, multi-page `.ps` paging, WMF / EPSI preview
rendering — see [planned.md](planned.md).

#### Spreadsheets ✅

`.xlsx` / `.xlsm` / `.ods` workbooks. A workbook is several named sheets, each a table, so the
viewer mirrors the SQLite shape (entities → listing → drill into one → streaming table):

- **Sheets** (default) — a listing of the workbook's sheets. Enter drills into a sheet's
  **aligned table view** (the shared `RowsTableMode`: sticky header, horizontal pan, whole-file
  cell `/` search); `e` extracts the sheet to a CSV file. Per-column alignment comes from
  calamine's native cell types (Int / Float → right; String / Bool / Date → left), and the
  header row is detected with the same all-text heuristic as the CSV viewer (`Shift+H`
  overrides).
- **Files** — the workbook's raw zip entries (it's an OOXML / ODF zip container), browsable and
  extractable through the standard archive path.
- **Info** — sheet count + names, plus core document properties (title / author / subject /
  keywords / created / modified) from `docProps/core.xml` (OOXML) or `meta.xml` (ODS).

Parsing is [`calamine`](https://docs.rs/calamine) — no spreadsheet engine, no formula
evaluation (cached cell values are shown). Detection is extension-routed (`.xlsx` / `.xlsm` /
`.ods`); like every OOXML / ODF container the magic bytes are `application/zip`, so an
extension-less workbook falls through to the archive viewer.

**Memory.** calamine has no streaming sheet API — a sheet is parsed whole into memory when you
drill in. Resident memory is therefore one sheet at a time (switching sheets drops the prior),
unlike the CSV viewer's sliding window. Sheets are bounded (Excel caps at ~1M rows) and the
container is read into memory for calamine's random access, so this is the accepted trade; a
truly streaming reader would need a custom parse of the sheet XML.

#### SQL ◐

`.sql` / `.ddl` / `.dml` / `.psql` / `.pgsql` files render as syntax-highlighted source. The Info
view adds an SQL section: heuristic dialect guess (PostgreSQL / MySQL / SQLite / T-SQL / generic),
statement count broken down by category (DDL / DML / DQL / TCL / Other), inventories of created
objects (tables, views, indexes, functions, triggers — with names), comment-line count, and a
flag when an inline `$$ … $$` PL/pgSQL block is present. The scanner tracks string / comment /
dollar-quoted state so semicolons inside strings or procedural bodies don't false-split. Real
formatter / outline mode still planned.

#### CSS ◐

`.css` files render as syntax-highlighted source. The Info view adds a CSS section: style-rule
count (CSS nesting included), total selector count with a per-kind occurrence histogram
(class / id / element / pseudo / attribute / universal), distinct custom-property count,
`@media` and `@keyframes` counts, and an `@import` list — absolute / protocol-relative URLs
flagged in the warning style. A Colors section renders the deduped colour palette as block-glyph
swatches, most-frequent first. Parsing is `cssparser` + `cssparser-color`: colours are scanned
only inside declaration values, so a colour word in a selector (`.gold`), a string
(`content: "red"`), or a comment never false-matches. Hex / `rgb()` / `hsl()` / `hwb()` / named
colours resolve to swatches; CIE / Oklab spaces are counted but not swatched. Per-rule
specificity annotation in the source view is still planned.

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
| CSV / TSV  | `.csv`, `.tsv`      | ✅      |

JSONC and JSON5 default to **raw** (the pretty path collapses comments / JSON5 syntax, so
defaulting to it would silently lose information); `r` toggles into the strict-JSON pretty form
when needed. JSON Lines defaults to pretty: each non-empty line round-trips through serde_json
and is separated by a blank line.

CSV / TSV open in an aligned table view: sticky header row + separator under it, body rows
served from a bounded sliding window over a seekable `csv` reader — a retained 1000-record
seed (top of file) plus a window that refills by seeking back to a sparse record-position
anchor, so resident memory stays flat regardless of file size or how far the user scrolls
(multi-GB files no longer materialise to the deepest row viewed). Column widths are seeded
from the first 1000 records, auto-widen monotonically as wider cells scroll into view (the
sticky header
repaints on every width change), and shrink only when the user presses `Shift+R` (reflow from
viewport). `Shift+H` toggles the header on/off, overriding the heuristic (row 0 all-text →
header on; row 0 has a typed cell → header off). `Left` / `Right` pan one column at a time
when the table is wider than the terminal. Per-column type inference (int / float / bool /
date / string / mixed) is sampled from the seed and rendered in the info section; numeric
columns (int / float only) are right-aligned in the body and header so digits line up. The
file's total record count, delimiter, encoding, and malformed-row counter sit alongside the
column stats. Encoding is UTF-8 native, with transparent UTF-16 LE/BE → UTF-8 transcode at
the byte-source boundary. Multi-line cells (embedded `\n` from a quoted record) collapse to
one visual row with a muted `↵` glyph marking the line break; `\t` becomes a space and
`\r` is dropped so nothing can break the terminal cursor. `/` opens a single-cell-scoped
search (substring, smart-case) that spans the whole file — it pages the window across every
record rather than holding them all, so it stays exhaustive at bounded memory; `n` / `p`
step matches, panning columns and scrolling rows to bring each match into view. The exact
total record count (and jump-to-end) is settled by a one-time streaming count pass that
discards cells; until then the info view shows `N (partial)`. Malformed records (over 4 MiB raw,
over 10 000 physical lines, or rejected by the csv crate) render as a single `<error>` row in
`theme.warning` and bump the status-bar counter. Print mode renders the seed widths only (no
auto-widen) and allows long cells to overflow rightward for that one row — alignment resumes on the
next row.

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

A corrupt or truncated image (bad CRC, partial payload) does not abort peek: the viewer degrades to
the Hex view and records the decode error as a warning (shown in Info and flagged with `!` on the
breadcrumb); the pipe path falls back to a hex dump with the cause on stderr. This degrade-to-Hex
fallback is generic to the render loop — any view mode that fails to render an input lands on Hex
plus a warning rather than crashing.

Decode allocation is capped so a decompression bomb — a tiny header declaring a gigapixel canvas, or
an animation declaring thousands of huge frames — errors and degrades to Hex instead of OOM-killing
the process. Static rasters inherit the `image` crate's 512 MiB default via `ImageReader`. The
GIF/WebP animation path (built directly, so uncapped by default) sets that per-frame limit *and*
caps the cumulative decoded-frame total at 1 GiB — the second cap covers WebP, whose decoder ignores
the per-frame limit, and the many-small-frames case the per-frame limit alone misses.

#### SVG ✅

SVG (`.svg`) is vector; the `image` crate doesn't handle it. Rasterized via `resvg`.

Two viewing modes (cycle with Tab):

- **Rendered preview** (default) — rasterize, render through the image pipeline
- **Source view** — syntax-highlighted XML (pretty or raw)

Re-renders on terminal resize.

##### SVG Animation ◐

CSS `@keyframes` animation is supported (`types/image/pipeline/svg_anim/`). The parser collects each
`@keyframes` rule plus inline-style `animation-*` references on elements, builds a merged frame
timeline (one frame per stop for `steps()` timing, ~30 fps interpolated for `linear`), and
`SvgAnimationMode` rasterizes each frame on demand from a per-frame patched SVG. A bounded LRU (64
entries, keyed by `(frame, grid_cols, grid_rows)`) makes a full second loop free.

Phase 1 covers what termsvg / asciinema-svg-style files use: `transform: translateX/Y/translate`
under `steps()` or `linear` timing. Targets resolve via inline `style="..."` *or* flat CSS
selectors (tag, `.class`, `#id`, `tag.class`) parsed by
`types/image/pipeline/svg_anim/selectors.rs`; combinators, pseudo-classes, attribute selectors,
and `*` are silently dropped. SMIL (`<animate>`, `<animateMotion>`) is still deferred.
`--no-svg-anim` forces the static render. The Info panel reports frame count, total duration,
and looping vs one-shot.

#### Transparency Handling ✅

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

#### Image Sizing Modes ✅

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

#### Zoom ✅

Every graphic-rendering view (raster images, animations, animated SVG,
PDF / CBZ pages, font specimen, static SVG) supports zoom:

- `+` / `-` — zoom in / out in 1.25× steps. Anchored on the viewport
  centre so the pixel under the centre stays put across the change.
- `0` — reset to 1× and pan to the origin.
- `1`..`9` — jump to a whole-number preset (1× .. 9×).
- Maximum 16×.

Every backend now ROI-renders: only the visible viewport's pixel ROI
is cropped from a native-resolution source and rescaled, so memory
stays proportional to the viewport, not to zoom². Each backend picks
its source strategy:

- **Raster / GIF / WebP** — source is the decoded native image.
- **CBZ** — decoded native bitmap per page; lazy-loaded, held for the
  renderer's lifetime.
- **PDF** — rasterised once at Pdfium's 4096-pixel render ceiling,
  single-slot cache (current page only) so a long document does not
  accumulate per-page rasterisations. Beyond the render ceiling the
  viewport upscales pixels rather than re-rasterising.
- **Font specimen** — re-rasterise the active face at higher resolution
  when zoom crosses into a new integer bucket (1×, 2×, 3×, …). A 1.25×
  step inside the same bucket reuses the existing source; only the
  bucket cross triggers a fontdue re-pass.
- **SVG (static + animated)** — same integer-bucket strategy: the
  rasterised source bitmap is rebuilt at `bucket × base` resolution
  when zoom crosses the next integer, so ROI crops read native resvg
  detail rather than upscaling pixels. The animated path caches per
  `(frame, grid, bucket)` in its bounded LRU so a frame revisited at
  the same zoom stays a cache hit.

Zoom is interactive only — pipe / `--print` output always renders at 1×.

### Audio Files ✅

Metadata-only Info view (no playback, no waveform). Container + codec params from a symphonia
probe — duration, channel count + layout, sample rate, bit depth, average bitrate — plus tag
fields from ID3v1/v2 (MP3, AIFF), Vorbis comments (Ogg, FLAC, Opus), MP4 atoms (m4a, m4b), and
APE: title, artist, album, album-artist, track / disc number, date, genre, composer, comment.

Embedded blobs (`APIC`/FLAC PICTURE/`covr`/`METADATA_BLOCK_PICTURE` album art, `USLT`/`SYLT`/
`LYRICS=` lyrics) get **dedicated Tab views**: Info → Cover (primary picture, ASCII-rendered
through the image pipeline) → Lyrics (plain text) → Embeds (TOC of every embedded blob).
Cover prefers the FrontCover-tagged picture, falls back to the first one. Embeds listing
shows `pictures/<usage>.<ext>` per visual (front / back / artist / leaflet / …) plus
`lyrics/lyrics.txt` when present, with the same `e` extract flow as PDF embeds;
`--extract pictures/front_cover.jpg` dumps the cover. Extracted picture bytes re-enter
the peek pipeline and render as ASCII art on recursive peek; lyrics re-enter as plain text.

| Format         | Extensions               | Status                                          |
|----------------|--------------------------|-------------------------------------------------|
| MP3            | `.mp3`                   | ✅                                               |
| FLAC           | `.flac`                  | ✅                                               |
| Ogg Vorbis     | `.ogg`, `.oga`           | ✅                                               |
| Opus           | `.opus`                  | ✅                                               |
| WAV            | `.wav`, `.wave`          | ✅                                               |
| MPEG-4 audio   | `.m4a`, `.m4b`, `.m4p`   | ✅                                               |
| AAC (ADTS)     | `.aac`                   | ✅                                               |
| AIFF           | `.aiff`, `.aif`, `.aifc` | ✅                                               |
| Apple CAF      | `.caf`                   | ✅                                               |
| Matroska audio | `.mka`                   | ✅                                               |
| WMA            | `.wma`                   | ◐ container-only — symphonia doesn't decode WMA |

### Animated Images (GIF, WebP) ✅

Auto-plays at native frame rate. `Space` toggles play/pause; `n`/`p` and Left/Right step frames; `b`
cycles background. Status line shows frame counter and play/pause. Print mode renders the first
frame. Frame count appears in the file info screen. Transparency handling applies.

### Comic Archives ✅

| Format | Extensions | Status |
|--------|------------|--------|
| CBZ    | `.cbz`     | ✅      |

`.cbz` files (Comic Book ZIP — a ZIP container holding page images in name order) get a
multi-mode view:

- **Read** (default) — paged image render through the shared image pipeline (same
  `PagedImageMode<R>` machinery as PDF). `n` / `p` step pages; the status line shows
  `page X/Y`. Pages are decoded lazily and held in a single-slot cache so resize / image-mode
  cycle / background cycle re-render only the current page.
- **TOC** — the raw ZIP file tree via the shared `ListingMode`. `e` / `--extract` pulls an
  individual page through recursive peek (an extracted PNG opens in the image viewer).
- **Info** — format label, page count, total uncompressed image bytes.

Page entries are filtered by image extension (`.png` / `.jpg` / `.jpeg` / `.webp` / `.gif` /
`.bmp` / `.tif` / `.tiff`) and sorted by name. `__MACOSX/` and other non-image entries are
skipped from the Read view but remain visible in the TOC. `.cbr` / `.cb7` / `.cbt` (other comic
archive containers) are not supported — only CBZ ships today.

### Object Files ✅

ELF, Mach-O, PE/COFF, and WebAssembly binaries — executables, shared libraries, relocatable
objects, `.wasm` modules — get a dedicated viewer instead of the binary hex fallback. Backed by the
`object` crate: one read-only API across every container format. Detection is magic-byte based
(`infer` MIME → `FileType::ObjectFile`, plus explicit `\0asm` for WASM), so an extensionless
`/bin/ls` routes correctly.

Three views, Tab-cycled:

- **Info** (landing) — format, architecture, file kind (executable / relocatable object / dynamic
  library / core dump), 32- vs 64-bit, endianness, entry point, section and symbol counts,
  debug-info presence, build identity (ELF build ID / Mach-O UUID / PE PDB GUID), and linked
  libraries (ELF `DT_NEEDED`, Mach-O dylibs, PE imports). Mirrors `file` + `readelf -d` +
  `otool -L`.
- **Sections** — `readelf -S`-style table: index, name, address, size, kind.
- **Symbols** — `nm`-style listing: address, size, type, bind, name. Prefers the full `.symtab`,
  falls back to the dynamic symbol table when the file is stripped. `/` searches symbol names;
  `Enter` **jumps the Hex view to the symbol's byte offset** (recovered from the containing
  section's file range). Undefined / `.bss` symbols list but can't jump — their address shows
  muted.

The Sections table uses the shared `TableMode` (the same one classfiles use): the column header
stays pinned through vertical scroll, each column is repainted live on a theme cycle,
`Left`/`Right` pan columns, and `/` searches names (`n`/`p` step matches, panning horizontally
only as far as needed to reveal an off-screen hit). Column widths fit their content.

Universal (fat) Mach-O containers are unwrapped transparently — the host architecture's slice is
parsed and the Info view lists every slice. No extract path: sections and symbols are not
standalone files.

| Format      | Coverage                                                        |
|-------------|-----------------------------------------------------------------|
| ELF         | executables, shared objects (`.so`), relocatable objects (`.o`) |
| Mach-O      | executables, `.dylib`, `.o`; universal (fat) binaries unwrapped |
| PE / COFF   | Windows executables and DLLs                                    |
| WebAssembly | `.wasm` modules (functions surface as symbols)                  |

`object` enum values (`BinaryFormat` / `Architecture` / `ObjectKind` / `Endianness`) are carried
through `ObjectMeta` and mapped to display labels only in `info_render`. Bare COFF `.obj` files have
no dedicated magic, so they're detected by validating the full COFF header (known machine, no
optional header, sane section count, executable-image flag clear) — strict enough that a Wavefront
`.obj` 3D model stays text. Remaining deeper inspection (compiler/toolchain notes, interactive
fat-slice switching) is tracked in [planned.md](planned.md).

### Java Classfiles ✅

`.class` files (JVM bytecode containers) get a dedicated viewer via the `cafebabe` crate.
Detection is magic-byte based — and the magic, `CA FE BA BE`, is shared byte-for-byte with the
Mach-O fat / universal-binary magic. `head_magic_mime` disambiguates on the field at offset 6:
a classfile's `major_version` is ≥ 45 (JDK 1.0); a fat Mach-O's `nfat_arch` slice count there is
small (< 45 in any real binary), so the field cleanly separates them.

Four views, Tab-cycled:

- **Info** (landing) — class name, superclass, interfaces, JDK version (classfile `major − 44`
  for major ≥ 49: 52 = Java 8, 61 = Java 17), kind (`public final class` / `interface` /
  `enum`), the `SourceFile` attribute, field and method counts.
- **Fields** — table: modifiers, type, name.
- **Methods** — table: modifiers, name, signature. Descriptors are decoded to source form —
  `(Ljava/lang/String;I)V` renders as `(String, int) -> void`.
- **Bytecode** — `javap -c`-style disassembly of every method: byte offset, mnemonic, and
  resolved operand (member references as `class.name:descriptor`, simple branch targets as
  absolute offsets, switches summarised by entry count). `n` / `p` jump between methods; `/`
  searches the listing. Parsed separately with
  bytecode enabled, so a decode failure here leaves the cheaper metadata views intact.

Field types and method signatures are syntax-coloured the way a Java / Rust highlighter would
show them — primitive types, class names, array brackets, and punctuation each in their own
theme colour, so a signature reads at a glance.

Both tables use the shared `TableMode` (sticky header, content-fitted columns, horizontal pan,
`/` search) — the same mode object files use.

Two deliberate departures from a naive `javap` port:

- **No constant-pool count.** `cafebabe`'s constant-pool iterator skips `Utf8` entries, so a
  `count()` reports only a fraction of the true pool size. A wrong number is worse than none, so
  the field is omitted rather than shown misleadingly.
- **`descriptor` is a formatter, not a parser.** `cafebabe` already parses descriptors into
  typed values, but its `Display` re-emits the raw JVM form (`(I)V`). The `descriptor` module
  turns those typed values into readable, colour-tagged spans; it never re-parses raw
  descriptor strings.

No extract path — fields and methods are not standalone files.

### SQLite Databases ✅

`.sqlite` / `.sqlite3` / `.db` / `.db3` files open as a read-only browse over the SQLite
schema. The bundled `rusqlite` (`bundled` feature compiles the upstream SQLite C
amalgamation in — no system libsqlite at runtime) drives detection, schema scrape, and row
reads.

Two views, Tab-cycled:

- **Listing** (landing) — `tables/` / `views/` / `indexes/` / `triggers/` groups, one leaf
  per entity. Tables and views get two leaves: `<name>.sql` for the `CREATE …` DDL and
  `<name>.csv` for the contents. Indexes and triggers only get `.sql`. Empty kind groups
  are omitted. The listing's size column shows DDL byte length for schema rows and the
  row count for contents rows so users can compare table populations at a glance.
- **Info** — page size, page count, encoding, journal mode, schema version, user version,
  application ID (when non-zero), `PRAGMA integrity_check(1)` result, entity counts, total
  rows, and the five biggest tables.

Drill-down is split by leaf suffix:

- `<name>.sql` → Enter dumps `sqlite_master.sql` for the entity into an in-memory
  `.sql` source with a `-- <name> from <db>` header comment; the outer re-detect routes
  it through the existing SQL syntax view. Schema rows can also be extracted (`e`) and
  saved like any other archive entry.
- `<name>.csv` → Enter pushes a streaming rows view that mirrors the CSV table viewer
  (sticky header, horizontal pan, cell-scoped `/` search). A sliding-window cursor
  buffers 1000 rows at a time; scrolling outside the window triggers a refill via
  `SELECT * FROM "<entity>" LIMIT 1000 OFFSET k`. `COUNT(*)` runs once at construction
  so the scrollbar / `Bottom` math is exact without driving a full scan. NULL renders
  distinct from the empty string (cells are `Option<String>` across the shared
  `RowSource` trait); BLOBs render as `<blob: N bytes>` (inline hex preview deferred).
  Per-column alignment is inferred from declared type affinity: `INT` / `REAL` /
  `NUMERIC` / `DECIMAL` right-align, everything text-shaped (`CHAR` / `CLOB` / `TEXT` /
  `DATE` / `TIME` / `BOOL`) stays left. `e` extracts the rows to a CSV file on disk
  by streaming `SELECT *` through a `csv::Writer` into a tempfile — NULL → empty,
  numbers / text → display form, BLOB → SQL hex literal `X'…'` (lossless,
  round-trippable into an `INSERT`).

Sources without an on-disk path (stdin, in-memory, extracted from another container)
spool to a `NamedTempFile` that lives for the connection's lifetime, so piped databases
work too. peek never writes to the database — connections open with
`SQLITE_OPEN_READ_ONLY`.

Cell-scoped search spans the whole table — it pages the sliding window across every row
(repeated `ensure_row`), so it's exhaustive without materialising the table. Predicate-
pushdown LIKE / GLOB queries (running the match in SQL instead of row-by-row) are deferred.
WAL / `-journal` sidecar inspection, SQLCipher-encrypted DBs, and a custom-query prompt are
also deferred.

### Certificates and Keys ◐

Cryptographic material gets a per-entry Info section. The source view depends on the container:
PEM shows its text, JWK shows the pretty-printed JSON, raw DER is binary so it gets Info + hex
only. Detection runs both ways: extension routing covers `.pem` / `.csr` / `.crl` / `.key` /
`.p7b` / `.p7c` / `.pub` (PEM), `.der` (DER), and `.jwk` / `.jwks` (JWK); content sniff catches
`-----BEGIN ` headers, OpenSSH algorithm prefixes (`ssh-rsa`, `ssh-ed25519`, `ecdsa-sha2-*`, incl.
the FIDO/U2F `sk-*` variants), a JSON object whose `kty` is a known key type, and raw DER. `.crt`
/ `.cer` deliberately route by content because they carry *either* PEM or DER — a `-----BEGIN `
lead picks PEM, a leading `0x30 0x82` SEQUENCE that fully decodes as an X.509 cert picks DER. The
DER and JWK sniffs both parse (not just pattern-match) so unrelated ASN.1 / JSON isn't mislabelled.
A `.json`-named JWK keeps the generic JSON view — the JWK sniff only fires for `.jwk` / `.jwks`,
stdin, and extension-less files, where the extension isn't already authoritative.

Decoded entries — a single PEM file may carry many (fullchain bundles, multi-block exports), a JWK
Set holds one per `keys` member, a DER file is one entry. Each renders as its own block under the
info section (headed **PEM** / **DER** / **JWK** by source). DER carries no label, so its kind is
recovered by structure: X.509 cert, then CRL, then CSR, then a PKCS#8 / SPKI key — first that
decodes wins:

| Entry             | Surface fields                                                                                                                                                                                                                                                                                                                           |
|-------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| X.509 certificate | label, version, subject, issuer, serial (hex), NotBefore / NotAfter (UTC ISO 8601), days remaining (warning-coloured ≤ 30 days; negative for expired), public key algorithm + bits, signature algorithm, SANs (DNS / IP / email / URI), CA flag, self-signed flag, key usage, extended key usage, SHA-1 fingerprint, SHA-256 fingerprint |
| CSR (PKCS#10)     | label, subject, requested SANs (DNS / IP / email / URI), public key algorithm + bits, signature algorithm                                                                                                                                                                                                                                |
| CRL               | label, issuer, This Update / Next Update, revoked entry count, signature algorithm                                                                                                                                                                                                                                                       |
| Private key       | label, key type (RSA / EC + curve / Ed25519 / DSA / opaque), bit size (best-effort from PKCS#1 / SEC1 / PKCS#8). Encrypted / opaque keys (`ENCRYPTED PRIVATE KEY`, `OPENSSH PRIVATE KEY`) show structural info only — no password prompt                                                                                                 |
| Public key        | label, key type, bit size (parsed from SPKI envelope)                                                                                                                                                                                                                                                                                    |
| SSH public key    | algorithm, bits, comment, SHA-256 fingerprint (matches `ssh-keygen -l -E sha256` output)                                                                                                                                                                                                                                                 |
| JSON Web Key      | type (RSA / EC / oct / OKP) + curve, bit size (RSA modulus / curve / `oct` secret), algorithm, use, key ops, key ID, RFC 7638 thumbprint (`SHA-256:` base64url)                                                                                                                                                                          |

Decode failures don't suppress the rest of the section — a malformed block lands in a per-entry
**Parse error** row so a single bad PEM in a chain doesn't hide the others. Unrecognised PEM
labels surface as an `Unknown` entry that still records the label + DER body size.

Crates: `pem` (block parsing), `x509-parser` (cert / CSR / CRL), `ssh-key` (OpenSSH public-key
text), `sha1` + `sha2` (fingerprints), `serde_json` (JWK). Private-key bit-size inference uses a
hand-rolled ASN.1 TLV walker over PKCS#1 / SEC1 / PKCS#8 / SPKI envelopes — small enough to inline
without pulling in `pkcs8` / `sec1` / `pkcs1` separately; the JWK thumbprint reuses the shared
base64url codec in `crate::base64`.

Not yet wired: PKCS#12 / PFX, encrypted PKCS#8 password prompt. Tracked in
[planned.md](planned.md).

### Fonts ◐

TrueType and OpenType wrappers (`.ttf` / `.otf`), font collections (`.ttc` / `.otc`), and the
WOFF / WOFF2 web wrappers (`.woff` / `.woff2`) decode into a themed **Font** info section.
Detection runs both ways: extension routing covers the six extensions; magic-byte sniff catches
`00 01 00 00` / `OTTO` / `ttcf` / Apple's `true` variant / `wOFF` / `wOF2`, so unnamed sources
(stdin, archive entries) still classify. Source view is omitted — fonts are binary containers,
so the universal hex aux mode handles raw byte inspection.

Both web wrappers are unwrapped to their inner sfnt before the metadata / specimen pipeline
runs, so every downstream consumer sees a plain font:

- **WOFF 1.0** — zlib-per-table compression around an ordinary sfnt. Unwrapped in-tree (offset
  table + directory rebuilt, each table inflated) on the `flate2` zlib decoder already present —
  no new dependency.
- **WOFF 2.0** — a single brotli stream plus a glyf/loca table transform, so bare decompression
  isn't enough: the glyf table is re-encoded and loca must be rebuilt from it. Delegated to
  `wuff` (pure Rust, `brotli-decompressor` — no C++ FFI), which reconstructs the sfnt.

Detection is content-true for both — `wOFF` / `wOF2` carry magic signatures, unlike a bare
`.br` stream.

Per face:

- Family, subfamily, full name, postscript name, version
- OS/2 weight (`100`..`900` with the canonical name in parens), width class, italic flag,
  monospaced flag
- Glyph count, units per em, codepoint count (sum across all Unicode cmap subtables)
- Script coverage — a list bucketed from cmap code-point ranges (`Latin`, `Latin Extended`,
  `Greek`, `Cyrillic`, `Hebrew`, `Arabic`, `Devanagari`, `Thai`, `Hangul`, `CJK`, `Emoji`,
  `Symbols`)
- Hinting present flag (head.flags bit 0)
- Designer, vendor URL, copyright, license URL

Apple system fonts still ship their canonical name records on the Macintosh platform (Mac
Roman), so the `name` decoder handles both UTF-16BE (Windows / Unicode platforms) and Mac
Roman with the full upper-half mapping — `©` / `™` / accented Latin round-trip cleanly.

**Specimen view.** The default open lands on a rasterised sample sentence — a hard-coded
pangram + digits + ASCII alphabet, run through `fontdue` at a fixed pixel size and routed
through the existing ASCII image pipeline. Every image-mode key works on the specimen
(`m` cycles full-color / block / geo / ascii / contour, `b` cycles backgrounds, `f` cycles
fit modes).

Collections (`.ttc` / `.otc`) expose every face: `n` / `p` step the active face through the
specimen in place, the status line shows `Face N/M`, and the Info screen lists every face's
metadata block (family / subfamily / weight / glyphs / scripts / …). A face fontdue can't
parse leaves the previous specimen in place rather than going blank.

Crates: `ttf-parser` for the `name` / `head` / `maxp` / `cmap` / `OS/2` / `post` table walks
(pure Rust, no_std, zero-alloc). `fontdue` for the specimen rasteriser. `wuff` for the WOFF2
unwrap.

Multi-script sample sentences keyed on cmap coverage are [planned](planned.md#font-files-).

### `.DS_Store` ✅

Apple Finder's per-folder Desktop Services Store — the "Bud1" Buddy-allocator container. peek
parses the block store and its single `DSDB` B-tree read-only, collecting every
`(filename, structure-id, typed value)` record.

- **Records** (landing view) — a `viewer::table::TableMode` with one row per stored property:
  `File │ Property │ Code │ Value`. Rows arrive in B-tree key order (by filename). Friendly
  `Property` labels cover the common Finder codes; the long tail shows `—` and leaves the raw
  `Code` to speak for itself.
- **Info** — record count, distinct tracked filenames, and the folder's own view style /
  background when present.

Value decoding: `Iloc` / `dilc` → `(x, y)` icon coordinates (or `auto`); `fwi0` → window-frame
bounds + view style; `vstl` → the view-style menu name; `BKGD` → `default` / `color #RRGGBB` /
`picture`; `modD` / `moDD` → a UTC date (an 8-byte little-endian `CFAbsoluteTime` double, seconds
since 2001, reusing the info layer's date formatter). Embedded binary property lists (`bwsp`,
`icvp`, `lsvp`) are reported as `binary plist, N bytes` rather than expanded.

Detection is by the `\0\0\0\1Bud1` magic (so renamed / stdin-piped stores route correctly) as
well as the canonical `.DS_Store` filename. The walk is defensive: a malformed node or an unknown
record encoding stops it and flags the Records table / Info `Note` row as truncated, keeping the
records gathered so far. No source view (opaque binary — `x` drops to hex) and no extract path
(records aren't files). Parser: hand-rolled, no added dependency.

### Binary and Archive Files ◐

For files peek doesn't have a specialized viewer for — firmware images, opaque container
formats — the baseline shows the **file info screen**:

- File type / MIME (detected via magic bytes through the `infer` crate)
- Size (exact + human-readable)
- Filesystem metadata (permissions, timestamps)

`infer` provides MIME only — no deeper metadata. Format-specific details could be added later
with dedicated parsers.

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

`/` opens a **leaf-name search**. The query matches against the last path segment of each
row only — `sub/` finds nothing because no leaf carries a slash. Directory leaves
participate, so a search for an ancestor name brings that subtree into view; the file
selection only moves when the active match lands on a file row, so Extract / Descend still target a
descendable entry. `n` / `p` step matches with wrap. Same `/` search is wired into every
ListingMode consumer — archives, ISO 9660, PDF `/EmbeddedFiles`, audio embed bundles,
directories, comic archives, and the EPUB / DOCX / ODT ZIP TOC.

| Format       | Extensions                     | Status    |
|--------------|--------------------------------|-----------|
| ZIP          | `.zip`, `.jar`, `.war`, `.apk` | ✅         |
| Tar          | `.tar`                         | ✅         |
| Tar + gzip   | `.tar.gz`, `.tgz`              | ✅         |
| Tar + bzip2  | `.tar.bz2`, `.tbz2`            | ✅         |
| Tar + xz     | `.tar.xz`, `.txz`              | ✅         |
| Tar + zstd   | `.tar.zst`, `.tzst`            | ✅         |
| Tar + lz4    | `.tar.lz4`, `.tlz4`            | ✅         |
| Tar + brotli | `.tar.br`, `.tbr`              | ✅         |
| 7-Zip        | `.7z`                          | ✅         |
| cpio         | `.cpio`                        | ✅         |
| cpio + gzip  | `.cpio.gz`                     | ✅         |
| ar / Debian  | `.ar`, `.deb`, `.a`            | ✅         |
| RAR          | `.rar`                         | ☐ planned |

Info view shows entry / file / directory counts and total uncompressed size. Listing failures
(corrupt archive, unsupported variant) surface as a warning row and the TOC view is empty. When an
`ar` archive's members are object files — i.e. a static library (`.a` / `.lib`) — the Info view adds
a **Static library** section: object-member count and the target architecture (read from the first
object member). A non-object `ar` archive such as a `.deb` doesn't get this section.

A **sticky parent breadcrumb** pins the current top row's ancestor chain to the upper rows of the
viewport when scrolled — so even mid-tree the path back to root stays visible. Same TOC code path
serves disk-image listings, so the behavior matches there too. Capped to one third of the viewport
height, suppressed when scroll is at the top or the top row is a top-level entry. Toggle with `s`;
when off the status bar shows `sticky off`.

#### Single-stream Compression ✅

Bare single-stream codec wrappers decompress transparently — peek opens straight to the inner
content (rendered as whatever it actually is: source, JSON, image, etc.), and the info view
adds a Compression row showing the codec and the size before / after decompression. No TOC
detour. Decompression failures fall back to a Hex view of the raw compressed bytes plus a
warning row in info.

| Format | Extensions | Status |
|--------|------------|--------|
| gzip   | `.gz`      | ✅      |
| bzip2  | `.bz2`     | ✅      |
| xz     | `.xz`      | ✅      |
| zstd   | `.zst`     | ✅      |
| lz4    | `.lz4`     | ✅      |
| brotli | `.br`      | ✅      |

Decompressed output is capped at 256 MiB. Anything larger surfaces a warning and the viewer
shows the raw compressed bytes — the same shape as a corrupt-stream fallback.

Every codec except brotli is detected by magic bytes *and* extension; a raw brotli stream has
no signature, so `.br` / `.tar.br` are extension-only — an extensionless or piped brotli stream
won't auto-classify.

#### Disk Images ✅

| Format | Extensions            | Status                                                  |
|--------|-----------------------|---------------------------------------------------------|
| ISO    | `.iso`                | ✅ PVD metadata + recursive directory listing (Joliet)   |
| DMG    | `.dmg`                | ✅ UDIF trailer + plist partition map (no inner-FS walk) |
| Raw    | `.img`, `.bin`, `.dd` | ✅ MBR partition table walk in info (no listing)         |

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

**DMG** opens straight to the file info screen — there's no TOC listing because the inner
filesystem (HFS+ / APFS / FAT) would need its own walker.

**Apple Disk Image (UDIF)** metadata comes from the 512-byte "koly" trailer at the end of the
file: UDIF version, image variant (device / partition / mounted system), total uncompressed size,
data-fork length, embedded XML partition-map size, segment number / count, data + master checksum
algorithms, and the documented trailer flag bits (flattened, internet-enabled).

The **partition map** is decoded from the embedded XML plist the trailer points at — one read of
the plist region (a few KB), no payload bytes. Each `blkx` table is read for its Apple type token
(parsed from the entry name), its logical size (sector span × 512), its start sector, and a
compression summary from the entry's "mish" block table.

The rows split two ways. **Filesystem** partitions (`Apple_HFS`, `Apple_APFS`, … — and any
unrecognised type, which errs toward this side) each get a detail block: full name, friendly type
(`HFS+` / `APFS` / …), logical size, stored size + percent, codec (zlib / bzip2 / lzfse / lzma /
ADC) + ratio, a run-type histogram (`408 (1 raw, 4 ignore, 403 zlib)`), and the image offset in
bytes + sectors. The format **scaffolding** — protective MBR, primary/backup GPT header + table,
and free-space gaps — collapses into one compact `Partition scheme` block, one line each (size,
codec/`sparse`, offset). Nothing is hidden; the split just puts the substance up front. The image
offset (`mount -o offset=`, `dd skip=`, `mmls`) is shown for every partition, scaffolding included.

The mish runs are read for their structure only; reconstructing a partition's payload
(decompressing the runs) and walking its inner filesystem is deferred — see
[planned.md](planned.md#disk-images-).

The parsers are hand-rolled — no extra crate. The plist is walked with `quick-xml` (an existing
dep, same as DOCX / ODT) in `dmg_plist.rs`; the block tables parse in `mish.rs`. Hex view (`x`)
still works on the raw image bytes.

#### Filesystem Directories ✅

`peek <dir>` opens a one-level listing instead of erroring on "is a directory". Entries sort
dirs-first, then by case-insensitive name; perms / size / mtime / name columns mirror the archive
TOC view. A synthetic `..` row leads the list (suppressed at filesystem root) so the user can
walk back up — selecting it canonicalizes the current path and re-targets to its parent.
**Enter** descends: file → push (Esc returns to the listing); directory → re-target the current
frame (no stack of dirs to back out of). **Esc** at any directory listing exits peek. Hidden
entries are included; symlinks are followed for kind classification, with broken links shown as
`?`. `--print` and `--list` both render the listing. `/` searches entry names (same leaf-name
search as the archive TOC); `n` / `p` step matches with wrap, moving the selection onto each hit.

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
- **CSS** — rule count, selector count + per-kind histogram, custom-property count, `@media` /
  `@keyframes` counts, `@import` list (external URLs flagged), colour-palette swatch grid
- **Structured data** — top-level kind, key/element count, max nesting depth, total node count, XML
  root + namespaces
- **SVG** — viewBox, declared dimensions, element counts (paths, groups, rects, circles, text),
  script / external-href flags, plus source text stats
- **Binary** — detected format from magic (Mach-O, ELF, PE, ZIP, SQLite, …)

**JSON output (`--info --json`)** ✅ — emits the info screen as a single JSON object for shell
pipelines (`peek x --info --json | jq .size_bytes`). Core metadata is fully typed — `size_bytes`
stays a number, timestamps are ISO-8601 UTC strings (independent of `--utc`), MIME entries carry a
machine `category` (`registered` / `vendor` / `convention` / …). Absent optionals (created,
compression, warnings) are omitted rather than null. Each file type contributes a typed object
nested under its own key (`pdf`, `archive`, `image`, `sqlite`, …) with raw typed values and
lowercase machine tokens for enum fields (`line_endings: "crlf"`, `top_level_kind: "object"`). A
type with no typed encoder falls back to a `details` text array, but every shipping type provides
one. `--json` requires `--info` and is rejected otherwise.

Both outputs derive from **one view model per type**: a struct that derives `serde::Serialize`
(JSON) and `#[derive(InfoView)]` (the themed terminal render), so labels, skip rules, and
per-field formatting are declared once and can't drift between print and JSON. A displayed section
block corresponds to a nested JSON object; per-field human formatting (sizes, gradients, muted
secondary text) lives in `InfoValue` impls while the same field serializes its machine value.

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

### Text Search ◐

`/` opens a search prompt over the status line; type a query and Enter runs it. Matching is
**exact substring** with **smart-case** — an all-lowercase query matches case-insensitively, any
uppercase character makes the whole query case-sensitive. Available in every text-rendering
view: source / plain text / structured raw-pretty / SVG XML (`ContentMode`), the rendered HTML
view, the EPUB **Read** view, the DOCX / ODT / RTF **Read** views, the PDF **Text** view, the
CSV / TSV **Table** view and the SQLite contents view that shares it (single-cell scope —
a query can't span a delimiter; over a SQLite table the scan currently covers only the
buffered window), and every listing TOC (leaf-name scope — matches the last path segment only, so
`sub/` finds nothing).
The shared search primitives in `viewer/search.rs` back all of them — each view scans its
own content domain into one.

On confirm the viewer jumps to the first match. `n` / `p` cycle forward / backward through every
match (wrapping at the ends), scrolling each match's line into view. (In the EPUB Read view
`n`/`p` normally step chapters; while a search is active they navigate matches instead — `Esc`
clears the search to get chapter stepping back.) A match gets an explicit background **and**
foreground pair — the syntax colour underneath is dropped so matched text looks uniform
regardless of what it was (and resumes after the span). Both states' colours derive from the
theme's `accent` hue: the current `n`/`p` match is vivid (`search_current_style`), the rest are
muted/dark (`search_match_style`), each paired with a neutral contrasting foreground. The status
line shows `cur/total` while a search is active, or `no match` when the query isn't found.

The scan is a single pass over the active view's lines, capped at 100,000 matches. An
empty-query Enter clears the search; so does `Esc` while a search is active (it clears matches
first, then falls through to the normal back / quit behaviour on a second press). Search clears
when the scanned line set changes underneath it — the `ContentMode` raw/pretty toggle, an EPUB
chapter step, or a terminal resize (the read-mode views key match indices to wrapped lines).

Regex matching and incremental (search-as-you-type) are still planned — see
[planned.md](planned.md#viewer-features-).

### Help Screen ✅

`h` / `?` opens the help screen. Shows keyboard shortcuts and the active theme. The shortcut list
is sectioned: a **Global** block, then one block per loaded mode (its label as the heading) for
that mode's extras — so an EPUB file's help shows a **Read** section (chapter nav) and a **TOC**
section (pin parent path) under separate headings, instead of one flat list that mixes them. A
mode's entry is dropped from its section when it duplicates a global key. The screen still lists
every mode the file has at once — it doesn't filter down to just the active mode.

### About Screen ✅

`a` shows the gradient peek logo, version, tagline, the active theme's full palette as colored
swatches, and a short list of pointers (homepage, license, common keys). Doubles as a theme
showcase — cycling themes with `t` while on About previews how each theme paints the full palette.

The logo is animated while About is open (`viewer/logo_anim.rs`, driven by the standard
`Mode::next_tick`/`tick` contract): the gradient slides across the wordmark in a seamless
ping-pong, and every few seconds two short bright runners trace the wordmark's outline in
opposite directions from the left corner, meeting at the far edge (white on dark themes, black
on light). Both painters share one glyph-walking loop (`output::paint_logo_with`); only the
per-glyph color closure differs. In plain mode (`--color plain` or cycling with `c`) the
animation is off — About paints the static logo and stops ticking until color comes back.
`Space` (`Action::PlayPause`, same key the image animation modes use) pauses / resumes the
animation, freezing it on the current frame; while paused the tick scheduling stops entirely.

### Extraction ✅

Pull an inner item out of a container as a standalone file. Sources currently:

- **Archive entries** (`.zip`, `.tar[.gz|.bz2|.xz|.zst|.lz4|.br]`, `.7z`, `.cpio[.gz]`, `.ar`):
  extract a single file by its inner path. Stored zip / uncompressed tar members are a verbatim
  slice of the backing source, so they extract as a zero-copy `FileRange` view — no spool, no
  copy. Other entries ≥ 16 MiB spool to a `NamedTempFile` in `$TMPDIR/peek-*` (random-access
  reads without holding the whole payload in RAM); smaller entries stay in `Bytes`. The temp
  file unlinks automatically when the last reference to the extracted `InputSource` drops (RAII
  via `Arc<NamedTempFile>`) — including a `FileRange` carved from a spooled entry, which carries
  the guard so a recursive view keeps its backing tempfile alive. The 256 MiB cap survives only
  on the in-memory fallback path. Pass `--no-tempfile` to force RAM-only behaviour (cap
  dropped — the user has opted in to OOM risk). tar/cpio/ar/7z extract streams the walk over a
  seekable reader (a windowed range adapter backs `FileRange` sources), so locating one member
  never reads the whole archive into RAM and a compressed tar inflates only up to the matched
  entry — every codec (gz/bz2/xz/zst/lz4/br) streams. 7z streams per-entry too (solid blocks still
  decode up to the match — inherent to the format — but the member never buffers).
- **ISO entries** (`.iso`): extract a single file via a zero-copy `FileRange` view over the
  backing image — no decompression, no buffering, multi-GB ISOs unaffected. A recursive ISO
  inside a spooled archive entry now also yields a guarded `FileRange` rather than buffering the
  range into RAM.
- **Animation frames** (`.gif`, `.webp`, animated SVG): extract a single composited frame as a
  PNG at the source's native pixel size (SVG sub-512px scales up to 512 on the longest axis;
  override with `--extract-size`).
- **PDF embeds** (`/EmbeddedFiles` attachments, memory source) and **inline images**
  (`pages/page{N}/image{M}.{ext}` pseudo-paths for image XObjects).
- **Audio embeds**: `pictures/<usage>.<ext>` per visual, plus `lyrics/lyrics.txt`.
- **SQLite entities**: `<kind>/<name>.sql` (DDL) and `<kind>/<name>.csv` (table / view contents).
- **Spreadsheet sheets**: `<sheet>.csv` streams one worksheet to CSV; raw ZIP paths extract the
  underlying workbook part.
- **Document embeds** (DOCX / ODT / RTF): extract an embedded image by its inner path.

CLI: `peek <file> --extract <KEY> [-o PATH]`. `<KEY>` is an entry path for containers or a
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

| Key                   | Action                                                              |
|-----------------------|---------------------------------------------------------------------|
| `q`                   | Quit                                                                |
| `Esc`                 | Pop the session stack (exit at depth 1, return to parent otherwise) |
| `Up` / `k`            | Scroll up                                                           |
| `Down` / `j`          | Scroll down                                                         |
| `Page Up` / `u` / `Ctrl+B` / `Ctrl+U` | Page up                                             |
| `Page Down` / `d` / `Ctrl+F` / `Ctrl+D` | Page down                                         |
| `Home` / `g`          | Go to top                                                           |
| `End` / `G`           | Go to bottom                                                        |
| `Enter`               | Descend into selection (recursive peek)                             |
| `e`                   | Extract selected entry / current frame                              |
| `s`                   | Toggle sticky parent-directory breadcrumb in listing TOCs           |

### Views and Modes

| Key         | Action                                      |
|-------------|---------------------------------------------|
| `Tab`       | Cycle the file's view modes forward         |
| `Shift+Tab` | Cycle the file's view modes backward        |
| `i`         | Jump to file info screen                    |
| `h` / `?`   | Toggle help screen                          |
| `t` / `T`   | Cycle theme forward / backward              |
| `c` / `C`   | Cycle output color mode forward / backward  |
| `x`         | Toggle hex dump (no-op when hex is default) |
| `a`         | Toggle about / status screen                |

### Search *(context: text / source / structured views)*

| Key | Action                |
|-----|-----------------------|
| `/` | Open search prompt    |
| `n` | Next search match     |
| `p` / `N` | Previous search match |

### Text Views *(context)*

| Key | Action                                            |
|-----|---------------------------------------------------|
| `l` | Toggle line numbers                               |
| `w` | Toggle line wrapping                              |
| `r` | Toggle pretty-print vs raw (structured data only) |

### Image Views *(context)*

| Key              | Action                                                                 |
|------------------|------------------------------------------------------------------------|
| `m` / `M`        | Cycle rendering mode forward / backward (full/block/geo/ascii/contour) |
| `b` / `B`        | Cycle background forward / backward (auto/black/white/checkerboard)    |
| `f`              | Cycle fit mode (Contain / FitWidth / FitHeight)                        |
| `Left` / `Right` | Pan horizontally (FitHeight)                                           |
| `+` / `-`        | Zoom in / out in 1.25× steps (viewport-centre anchored)                |
| `0`              | Reset zoom to 1× and pan to origin                                     |
| `1`..`9`         | Jump to whole-number preset zoom (1×..9×)                              |

### Animated Image Views *(context: GIF, WebP)*

| Key              | Action                                          |
|------------------|-------------------------------------------------|
| `Space`          | Play / pause animation                          |
| `n` / `p`        | Next / previous frame                           |
| `f`              | Cycle fit mode (Contain / FitWidth / FitHeight) |
| `Left` / `Right` | Pan horizontally under `FitHeight`              |
| `b`              | Cycle background                                |
| `m`              | Cycle render mode                               |

`Left` / `Right` are pan keys in both static and animated image views — frame stepping uses
`n` / `p` exclusively (the previous Left/Right frame-step bindings are gone).

### Streaming Table — CSV / TSV / SQLite contents *(context)*

| Key       | Action                                                   |
|-----------|----------------------------------------------------------|
| `Shift+H` | Toggle header row on / off (CSV: override the heuristic) |
| `Shift+R` | Reflow column widths from the currently-visible viewport |

### Font Specimen *(context)*

| Key       | Action                                       |
|-----------|----------------------------------------------|
| `n` / `p` | Step to the next / previous face in a `.ttc` |

The help screen (`h`) is the authoritative in-app reference — all bindings derive from a single
source (`crates/peek-foundation/src/viewer/ui/keys.rs::Action::bindings`).

## Color and Rendering

### Theme Selection ✅

`--theme` / `PEEK_THEME`. Ten custom embedded `.tmTheme` themes:

- **idea-dark** — JetBrains IDEA default Dark
- **idea-light** — JetBrains IntelliJ Light
- **solarized-light** — Solarized Light
- **github-light** — GitHub Light
- **vscode-dark-modern** — VS Code Dark Modern
- **vscode-dark-2026** — VS Code Dark 2026
- **vscode-monokai** — VS Code Monokai
- **graveyard** — gothic moonlit night
- **candy-floss** — pastel candy on dark plum
- **victorian** — parlour parchment with oxblood

Default adapts to the terminal background (OSC 11 probe): `idea-light` on a light terminal,
`idea-dark` on a dark one (or when output is piped). Explicit `--theme` / `PEEK_THEME` wins.

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
`include_str!`. The gutter role drives the line-number column in ContentMode; the `search_match`
role paints search-result backgrounds in the text views.

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

For library-produced output (syntect), `viewer::ranges_to_escaped_trim_newline` replaces syntect's
hardcoded 24-bit `as_24_bit_terminal_escaped` with one routed through `StyleMode::fg_seq`, so
syntax-highlighted code is downgraded along with everything else.

## CLI Options

| Option           | Short | Description                                                                                                       | Status |
|------------------|-------|-------------------------------------------------------------------------------------------------------------------|--------|
| `--help`         | `-h`  | Show help screen and exit (short / long forms)                                                                    | ✅      |
| `--version`      | `-V`  | Show version info and exit                                                                                        | ✅      |
| `--print`        | `-p`  | Force print mode (direct stdout)                                                                                  | ✅      |
| `--plain`        | `-P`  | Sterile output: no highlighting, pretty-printing, or colors                                                       | ✅      |
| `--raw`          | `-r`  | Output verbatim source (no pretty-print)                                                                          | ✅      |
| `--theme`        | `-t`  | Syntax highlighting theme                                                                                         | ✅      |
| `--color`        | `-C`  | Output color encoding (truecolor/256/16/grayscale/plain)                                                          | ✅      |
| `--language`     | `-L`  | Force syntax language                                                                                             | ✅      |
| `--width`        | `-w`  | Image rendering width in characters                                                                               | ✅      |
| `--image-mode`   | `-m`  | Image rendering mode                                                                                              | ✅      |
| `--edge-density` |       | Edge density target for `--image-mode contour`                                                                    | ✅      |
| `--info`         | `-i`  | Show file info instead of contents                                                                                | ✅      |
| `--json`         |       | Emit `--info` as machine-readable JSON (requires `--info`)                                                        | ✅      |
| `--list`         | `-l`  | Print container TOC to stdout (archives, ISOs, directories, PDF / EPUB / DOCX / ODT / RTF / audio / comic embeds) | ✅      |
| `--utc`          |       | Show timestamps in UTC (default: local + offset)                                                                  | ✅      |
| `--background`   |       | Image transparency background (auto/black/white/checkerboard)                                                     | ✅      |
| `--margin`       |       | Image margin in transparent pixels                                                                                | ✅      |
| `--cell-aspect`  |       | Override terminal cell aspect ratio (height ÷ width)                                                              | ✅      |
| `--no-svg-anim`  |       | Force static render for animated SVG                                                                              | ✅      |
| `--line-numbers` | `-n`  | Enable line numbers (toggle with `l` in the viewer)                                                               | ✅      |
| `--extract`      | `-x`  | Extract a single inner item from a container by key                                                               | ✅      |
| `--output`       | `-o`  | Output path for `--extract` (or `-` for stdout)                                                                   | ✅      |
| `--extract-size` |       | Output pixel size for animation / SVG frame extract                                                               | ✅      |
| `--no-tempfile`  |       | Keep archive extracts in RAM (skip the `$TMPDIR` spool path)                                                      | ✅      |
| `--update`       |       | Check for newer release and re-run `install.sh`                                                                   | ✅      |

`--plain` is the single "sterile output" knob: it implies `--color plain` and additionally
disables syntax highlighting and structured pretty-printing. HTML and SVG drop their rendered
/ rasterized view and fall back to raw source; other rich views (image, PDF, DOCX, EPUB) still
compose but render without color. `--raw` is narrower: it skips pretty-printing of structured
/ SVG sources but keeps colors, font styles, and rich renders. Use `--raw --color plain` for
raw structure without colors while still letting HTML / SVG render.

`--print` / `-p` forces print mode regardless of TTY.

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

