# peek feature specification

This document is the engineering-detail reference for what peek does today. It covers implemented
(✅) and partial (◐) features. The user manual ([`manual/`](../manual/)) covers the same ground in
less detail and without status markers. When a feature changes, update both. For planned features
and open ideas (☐ and ❓), see [planned.md](planned.md).

Status legend: ✅ implemented · ◐ partial

## Contents

- [Operating modes](#operating-modes)
- [Supported file types](#supported-file-types)
  - [Source code](#source-code-)
  - [Structured data / config files](#structured-data-and-config-files)
  - [Image files](#image-files-)
  - [Audio files](#audio-files-)
  - [Animated images (GIF, WebP)](#animated-images-gif-webp-)
  - [Comic archives](#comic-archives-)
  - [Object files](#object-files-)
  - [Java classfiles](#java-classfiles-)
  - [SQLite databases](#sqlite-databases-)
  - [LMDB databases](#lmdb-databases-)
  - [Certificates and keys](#certificates-and-keys-)
  - [Fonts](#fonts-)
  - [`.DS_Store`](#ds_store-)
  - [Binary and archive files](#binary-and-archive-files-)
- [Viewer features](#viewer-features)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Color and rendering](#color-and-rendering)
- [Command-line options](#command-line-options)
- [Distribution](#distribution-)

## Operating modes

### Viewer mode ✅

The viewer mode is a full-screen interactive console view. It stays open until you exit with `q` or
`Esc`. You use the keyboard to toggle options, scroll, search, and switch between views.

The viewer works for all file types through the mode-stack architecture:

- `ContentMode` shows text, source code, and structured data.
- `ImageRenderMode` shows raster images and rasterized SVG.
- `AnimationMode` shows GIF and WebP animations.
- `HexMode`, `InfoMode`, `HelpMode`, and `AboutMode` are available for every file.

The viewer has these controls:

- `Tab` cycles through the file's view modes, such as content, image, SVG source, and Info. Hex,
  About, and Help have their own keys and aren't in the cycle.
- `i` jumps straight to Info, `x` opens the hex view, `h` or `?` opens help, and `a` opens About.
- `t` cycles the theme live, and `c` cycles the color encoding.
- `r` toggles between raw and pretty output in the structured-data viewer.
- In image views, `b` cycles the background and `m` cycles the render mode.
- In animations, `Space` plays or pauses, and `n`, `p`, `Left`, and `Right` step through frames.
- In text views, `l` toggles the line-number gutter and `w` toggles soft wrap.

Text search works in the text, source, and structured views. To search, press `/` to open the
prompt. `Ctrl-R` toggles regular-expression matching, and `n` and `p` cycle through the matches.
Each query scans at most 256 MB (`SEARCH_SCAN_MAX_BYTES`). The budget applies to the raw text scan
and to the CSV and SQLite table-cell scans. When a query reaches the budget, the counts show as
partial (`12/3400+`) and a warning appears.

### Print mode ✅

Print mode writes straight to stdout with no interaction, like `cat`. The default output depends on
the file type:

- **Text and source code**: syntax-highlighted, unless you pass `--plain`.
- **Structured data**: pretty-printed and highlighted. With `--raw`, peek prints the source
  verbatim. The output is still highlighted unless you pass `--plain`.
- **Images**: rendered as ASCII art, scaled to fit the terminal at the image's aspect ratio.
- **SVG**: a rendered preview as ASCII art.
- **Binary and unknown files**: a hex dump. The dump streams, uses the `hexdump -C` layout, and
  adapts to the terminal width.

Print mode is active when you set `--print` or `-p`, or when stdout isn't a terminal (TTY).

### Mode selection ◐

- `--print` or `-p` forces print mode.
- **Default:** if stdout is a TTY, peek opens the viewer. Otherwise it prints.
- **Binary and unknown files** open in the hex-dump viewer when peek runs interactively. When you
  pipe a binary file, peek streams a hex dump.
- All data types should support both modes where it makes sense.

TTY detection and `--print` or `-p` work. Binary files default to the hex-dump viewer, which is
interactive in a TTY and streamed for pipes. With `--plain` or `-P`, peek still uses hex for binary
files, because plain text would corrupt bytes that aren't valid UTF-8. peek doesn't yet pick the
mode based on content length. Today a TTY gets the viewer and a non-TTY gets print mode.

### Input ✅

peek is a single-file viewer, so it takes at most one positional argument. To read from stdin, pass
`-` explicitly or pipe into peek with no file argument. peek detects the stdin type by magic bytes
(images, binary) and by content sniffing (JSON, YAML, XML, and SVG). For plain text, peek uses
`--language` to choose the syntax highlighting.

To view several files, run peek once for each file. peek has no `cat`-style batch mode, because
concatenating images, structured data, and binary into one stream rarely produces useful output.

| Scenario              | Stdin is TTY                     | Stdin is piped            |
| --------------------- | -------------------------------- | ------------------------- |
| `peek` (no arguments) | Show short help                  | Read stdin, render        |
| `peek -`              | Read stdin (blocks until Ctrl-D) | Read stdin, render        |
| `peek file.rs`        | View file normally               | View file (stdin ignored) |

After peek consumes piped stdin, it reopens file descriptor 0 from the controlling terminal, so the
interactive viewer still gets keyboard input. peek resolves the terminal with `ttyname()` to the
real device path instead of using `/dev/tty`, because macOS kqueue can't register `/dev/tty`.

Stdin input works in all viewers: text, source code, structured data, raster images (PNG, JPEG,
WebP, and others), animated images (GIF and WebP), and SVG.

## Supported file types

This list isn't exhaustive. More types get added over time.

### Source code ✅

peek supports all standard languages that syntect supports, together with the extended definitions
from `two-face` (taken from bat). That covers more than 100 languages, including Rust, Python,
JavaScript, TypeScript, C, C++, Java, Go, Ruby, Shell, TOML, and Dockerfile.

Config files use the same highlighting path. They have no dedicated mode. Files such as `.ini`,
`.cfg`, `.conf`, `.properties`, `.env`, `.hcl`, and `.tf` resolve to their grammar through syntect's
extension lookup. Files such as `Makefile`, `Dockerfile`, `.gitignore`, and `.editorconfig` resolve
through syntect's name lookup. Special cases keyed on the filename fill the gaps where the extension
is misleading or missing:

- `.env.local`, `.env.production`, and `.envrc` use DotENV.
- `justfile` uses Makefile, the closest grammar, because no Just definition exists.
- `.dockerignore` uses Git Ignore.

Names with no grammar, such as `.dhall` and `.cue`, fall back to plain text.

peek detects extensionless `#!` scripts, such as `postinst`, `configure`, and git hooks, by their
shebang line and routes them to the matching grammar. The interpreter name (`sh`, `bash`, `python`,
`perl`, `ruby`, and so on) selects the syntax. This includes `env`-style shebangs such as
`#!/usr/bin/env python`.

Source files have syntax coloring with theme support. You can toggle line numbers (✅) with
`--line-numbers`, `-n`, or `l`.

#### Markdown ✅

Files with the extensions `.md`, `.markdown`, `.mdown`, `.mkd`, `.mkdn`, and `.mdwn` get two views:

- **Rendered** (default): pulldown-cmark drives a walker for CommonMark and GitHub Flavored Markdown
  (GFM). The walker emits text that wraps to the terminal width and uses ANSI styling. It renders
  these elements:
  - Styled headings. H1 gets a `═` underline and H2 gets a `─` underline.
  - Bullet and ordered lists with a hanging indent.
  - Task lists with `☐` and `✓`.
  - Blockquotes with a rail (`▍`).
  - Horizontal rules.
  - GFM tables drawn with box-drawing characters. Each column takes its alignment from the header
    separator row. When a row is wider than the available width, the columns shrink in proportion.
  - Fenced code blocks, highlighted by syntect in their declared language. If the language doesn't
    resolve, the block shows as plain full foreground on the surface color.
  - Emphasis, strong, and strikethrough as Select Graphic Rendition (SGR) attributes.
  - Inline code as full foreground on the surface color, as a boxed span.
  - Links, underlined and followed by the URL in dim text.
  - Images as `[image: alt] (url)`.
  - Footnote references and definitions.
  - Front matter (YAML `---` or TOML `+++`), shown as a dim verbatim block at the top.
- **Source**: the Markdown source with syntax highlighting, through `ContentMode`. You reach it with
  `Tab`. With `--raw`, it becomes the view that peek opens first. With `--plain`, peek drops the
  rendered view.

The Info view adds a Markdown section with these details:

- Heading counts by level, from H1 to H6.
- The number of fenced code blocks and their declared languages.
- Counts of inline code spans, links, images, tables, and list items.
- Task-list progress, shown as the number done, the total, and a percentage.
- The number of blockquote lines.
- The number of footnote definitions.
- Whether the file has front matter, and whether it's YAML or TOML.
- The prose word count, which excludes fenced code.
- A reading-time estimate at 230 words per minute.

#### Jupyter Notebook ✅

`.ipynb` files are JSON documents made of cells. They get three views:

- **Rendered** (default): peek translates the notebook into one Markdown document and renders it
  through the shared Markdown pipeline.
  - Markdown cells show as prose.
  - Code cells show as fenced blocks labeled `In [n]:`, highlighted by syntect in the kernel
    language.
  - Outputs show below each code cell. Stream and `text/plain` results show as fenced text. `error`
    outputs show as a bold `ename: evalue` header followed by the traceback with ANSI codes
    stripped. Image outputs, such as `image/png`, are noted but not drawn. Inline ASCII rendering of
    them is a follow-up.

  peek parses both nbformat 4 and the older nbformat 3 `worksheets` layout.

- **Source**: the raw notebook JSON through the generic structured content mode. It's
  pretty-printed, and `r` toggles raw output. You reach it with `Tab`. With `--raw`, it becomes the
  view that peek opens first. With `--plain`, peek drops the rendered view.
- **Blocks**: a flat table of contents (TOC) listing every code cell and image output. The entries
  are numbered in document order and get readable synthetic names such as `code-1.py` and
  `image-1.png`, instead of the notebook's opaque cell IDs.
  - To extract an entry, press `x` or pass `--extract code-1.py`. peek writes the Python source or
    the decoded image bytes.
  - To open an entry, press `Enter`. peek opens a nested peek view over an in-memory copy. Code
    opens with syntax highlighting, and images render as ASCII art.
  - `--list` prints the block names and sizes.

  The file stores image outputs as base64. peek decodes them with the hand-rolled `crate::base64`
  only when you extract or open them, so the render path never holds image bytes in memory.

The Info view adds a Notebook section. It shows the nbformat version, the kernel display name, the
language and its version, and the cell count split into code, Markdown, and raw cells. It also shows
the output count with image and error sub-counts, and the highest execution count.

peek detects a notebook by its `.ipynb` extension or by JSON that has both the `nbformat` and
`cells` keys. As a result, a notebook piped through stdin opens in the cell viewer instead of the
generic JSON pretty-printer.

#### HTML ✅

`.html`, `.htm`, and `.xhtml` files get two views. So do stdin streams that start with
`<!DOCTYPE html>` or `<html`.

- **Rendered** (default): a lynx-style flow rendered by the `html2text` crate. Paragraphs wrap to
  the terminal width. The view shows list bullets, a table grid with column sizing, and numbered
  link references. It applies ANSI styling for `<strong>`, `<em>`, `<code>`, `<s>`, and `<a>`. It
  also applies author colors from inline `style="..."` attributes and `<style>` rules, using the CSS
  that `use_doc_css` pulls in. peek filters out near-grayscale colors, so the author's default body
  and heading colors don't fight the terminal's foreground. `Tab` cycles to the source view.
- **Source**: the raw HTML with XML syntax highlighting, through `ContentMode`.

The Info view shows the structured XML stats, such as the root element and the element counts.

`html2text` holds the whole document in memory. For HTML larger than the render cap
(`RENDER_MAX_BYTES`), peek skips the rendered view. It shows one warning line, and the **Source**
view takes its place. As a result, you can still open a pathological page of several hundred MB.

#### Email ◐

peek parses `.eml` files (a single RFC 822 or MIME message) and `.mbox` files (a concatenated
mailbox) with the pure-Rust `mail-parser` crate. Detection works by extension and by content sniff,
so messages without an extension still open here. The sniff looks for a leading `From ` line for
mbox, or an RFC 822 header block with a recognized mail header for `.eml`.

A **`.eml`** file gets these views:

- **Message** (default): a themed header block followed by the body. The header block shows From,
  To, Cc, Date, and Subject, plus an attachment count when the message has attachments. If the
  message has an HTML part, peek renders it through the same `html2text` driver as the HTML viewer.
  Otherwise, peek word-wraps the plain-text part to the viewport. `RenderedTextMode` caches the
  result for each width and theme.
- **Source**: the raw RFC 822 text through `ContentMode`. With `--plain`, peek drops the rendered
  view and opens straight on the source.
- **Attachments** (when present): a listing of the message's MIME attachments and inline media. It
  shows each part's MIME type (the `Content-Type` header) together with its size and name. To
  extract a part to disk through the standard extract pipeline, press `e`. A `message/rfc822` part
  opens in a nested peek view on the saved part.

An **`.mbox`** file gets a **Messages** TOC with one row for each message. A hand-rolled splitter
finds the messages by their `From ` separator lines. Each row starts with the message's index, so
messages with the same subject stay distinct. The modification-time column shows the message's
`Date`.

To open the selected message, press `Enter`. peek opens a zero-copy `InputSource::subrange` that
covers only that message. The message gets the same Message, Attachments, Info, hex, and help views
as a standalone `.eml`. The only difference is that it has no raw Source view of its own, because
the mailbox's raw text is the secondary top-level view. Because of this design, even a mailbox of
several GB lists instantly, and peek parses only the message you open.

For `.eml`, the Info view shows the header summary, the attachment count, and the total attachment
size. For `.mbox`, it shows the message count.

#### Calendar and contacts ✅

iCalendar (`.ics`, `.ical`, and `.ifb`) and vCard (`.vcf` and `.vcard`) are the two IETF "vObject"
text formats. They share one content-line grammar, defined in RFC 5545 §3.1 and RFC 6350 §3.3. The
grammar covers line folding, `NAME;PARAM=VALUE:VALUE` properties, and `BEGIN` and `END` component
nesting. Both formats parse through a single hand-rolled parser (`types/vobject/line.rs`) that adds
no dependency. Only the renderer differs. Detection works by extension and by content sniff of a
leading `BEGIN:VCALENDAR` or `BEGIN:VCARD` marker, so files without an extension and stdin still
open here.

Both formats get a **rendered** read view (default), followed by the raw **Source** view through
`ContentMode`. With `--plain`, peek drops the rendered view and opens on the source. The rendered
view goes through `RenderedTextMode`, so peek caches it for each width and theme.

- **iCalendar** renders an agenda. It starts with an optional calendar name header, followed by one
  block for each `VEVENT` and `VTODO`.
  - Each event shows a summary heading and a readable date and time span. For timed ranges within
    one day, peek leaves out the redundant end date. peek shows `TZID` verbatim and doesn't convert
    time zones.
  - Events also show the location, the `RRULE` in readable form (`Weekly on Mon, Tue, …, 40 times`),
    the organizer and attendees, the status, the categories, the link, and a wrapped description.
  - To-dos show the due date, the status, and the percent complete.
  - The Info view shows the calendar name, the event and to-do counts, the event date range, the
    version, and the product.
- **vCard** renders one grouped card for each `VCARD`.
  - Each card shows the display name. peek takes it from `FN`, or composes it from the structured
    `N` property.
  - The card also shows the nickname, the organization (`Company — Department`), the title, every
    email, phone, and address with its `TYPE` annotation, the web address, the birthday, the
    categories, and a wrapped note.
  - peek handles v3 and v4 cards together, including v4 `tel:` URIs and quoted parameters with
    several `TYPE` values.
  - The Info view shows the contact count and the version of the first card.

peek reformats dates and times to a readable `YYYY-MM-DD HH:MM` without a date crate. The values are
already calendar fields, so this is string reshaping, not time math. These files have no inner items
to extract.

#### EPUB ✅

An `.epub` file is a ZIP container with HTML chapters and Open Packaging Format (OPF) metadata. It
gets three views:

- **Read** (default): one chapter at a time, through the shared HTML rendering pipeline. This is the
  same `html2text` driver as the standalone HTML viewer.
  - `n` and `p` step forward and back through the spine. The status line shows `ch X/Y`.
  - peek caches each rendered chapter at the current width, so stepping back is instant. A terminal
    resize re-renders only the visible chapter.
  - If an `<img>` tag has an empty or missing `alt`, peek shows a fallback `image: <basename>`
    label, so image references in chapters stay visible.
  - Cover-style chapters, which have almost no text and at least one image, render the first image
    as ASCII art inline. For example, `peek book.epub` opens on the cover.
  - The TOC view still lists every container entry, so you can inspect any image through a nested
    peek view.
- **TOC**: the raw ZIP file tree through the existing `ListingMode`. Use it to inspect cover images,
  stylesheets, or the OPF and Navigation Control file for XML (NCX) metadata files inside the
  container. To open the selected entry in a nested peek view, press `Enter`.
- **Info**: the Dublin Core (DC) metadata from the OPF. It shows the title, the author
  (`dc:creator`), the language, the publisher, the date, the identifier, the description, and the
  spine length.

Print mode (`--print`, or stdout that isn't a TTY) walks every chapter in spine order, with blank
lines between them. As a result, `peek book.epub | less` renders the whole book.

#### DOCX ✅

A `.docx` file uses Office Open XML (OOXML). It's a ZIP container with a `word/document.xml` body
and `docProps` metadata. It gets three views:

- **Read** (default): styled body text.
  - Headings (the `Heading1..6` paragraph styles) render bold and themed.
  - Bold, italic, underline, and strikethrough runs render with SGR. Explicit run colors apply.
  - Bullet-list paragraphs, which are those that have `numPr`, render with a `•` marker, indented by
    `ilvl`.
  - Embedded images show inline as `[Image: <basename>]` placeholders, resolved from the document's
    relationships.
  - Tables flatten to rows joined with `|`.
  - Word wrap follows the width and re-runs when you resize the terminal.

  A hand-rolled `quick-xml` walk over `word/document.xml` does the parsing. Full WordprocessingML
  parsers that map the XML to typed structs reject real-world Word files. Numeric attributes in
  those files often hold `"auto"` or `"none"`, which the parsers' strict integer types can't decode.

- **TOC**: the raw ZIP file tree through the shared `ListingMode`. Use it to inspect the inner XML
  parts and embedded media. To open the selected entry in a nested peek view, press `Enter`.
  `--extract word/media/imageN.png` works as it does for any ZIP archive.
- **Info**: core document properties from `docProps/core.xml`. It shows the title, author, subject,
  keywords, and the created and modified timestamps. It also shows paragraph, word, and image
  counts.

Lists render as flat bullets for now. peek doesn't yet resolve the numbering cascade from
`numbering.xml`, which covers numbered lists and nested numbering schemes. Every paragraph that has
a `numPr` shows as `•`.

If `word/document.xml` is larger than the render cap (`RENDER_MAX_BYTES`), peek drops the **Read**
view. The check uses the _uncompressed_ entry size, so peek catches a zip-bombed body before it
extracts the entry. The **TOC**, Info, and hex views take its place, and the TOC shows a warning
with the reason. The check lives in the shared `read_zip_entry` helper (`types/archive/reader.rs`).
As a result, the same cap applies to ODT (`content.xml`), EPUB chapter and image reads, CBZ page
reads, and spreadsheet document properties. RTF and HTML apply the cap to the whole source.

#### ODT ✅

An `.odt` file uses OpenDocument Text. It's a ZIP container with a `content.xml` body and `meta.xml`
Dublin Core metadata. It gets the same three views as DOCX. Both formats share an abstract syntax
tree (AST) and renderer in `crates/peek-types/src/types/document/{ast,render,renderer}`, together
with the generic `RenderedTextMode`. Only the parser differs between the formats.

- **Read** (default): styled body text.
  - Headings (`<text:h text:outline-level="N">`) render bold and themed.
  - Bold, italic, underline, strikethrough, and colored runs render with SGR.
  - peek resolves span styling through `<office:automatic-styles>`. A
    `<text:span text:style-name="T1">` element picks up the run-style attributes that the
    automatic-styles block defines for `T1`: `fo:font-weight`, `fo:font-style`,
    `style:text-underline-style`, `style:text-line-through-style`, and `fo:color`.
  - Bulleted lists use the `<text:list>` nesting depth for the indent and put a `•` marker on each
    `<text:list-item>`.
  - `<draw:image>` references inside `<draw:frame>` show as `[Image: <basename>]` placeholders.
  - Hyperlinks (`<text:a>`) underline all their inner runs.
  - Tables flatten to rows joined with `|`.
  - Word wrap follows the width and re-runs when you resize the terminal.
- **TOC**: the raw ZIP file tree through the shared `ListingMode`, exactly as for DOCX.
  `--extract Pictures/foo.png` works as it does for any ZIP archive.
- **Info**: the title, author, subject, keywords, description, and the created and modified
  timestamps. If the file has several `<meta:keyword>` entries, peek joins them with commas. The
  view also shows paragraph, word, and image counts.

In v1, peek intentionally doesn't read `styles.xml`, the package's separate container for named
styles. Real-world ODT files from LibreOffice and OpenOffice put all directly used styling into the
automatic styles in `content.xml`. Inheritance chains from `styles.xml` only matter for the small
fraction of files that rely on them.

#### RTF ✅

An `.rtf` file uses Rich Text Format, which is control-word markup in a single file, not a
container. It gets a single styled-text view:

- **Read** (default): the body text, with bold, italic, underline, and strikethrough runs and
  per-run color from the RTF color table. `rtf-parser` does the parsing. peek strips the `\info`
  metadata group from the body, so its title and author strings don't leak into the rendered output.
  peek also turns `\par` paragraph terminators into explicit CRLF line breaks before parsing,
  because the `rtf-parser` lexer doesn't emit a token for them by default.
- **Info**: the title, author, subject, keywords, and the created and revised dates from the `\info`
  group. It also shows paragraph and word counts.

By default, an RTF file opens to a single Read view. If the file embeds images as `\pict` groups,
peek adds a synthetic TOC of those images next to Read. To pull one out through the nested-peek
pipeline, press `e` or pass `--extract`. Plain RTF files without embedded images keep the single
view.

#### PDF / Adobe Illustrator ✅

A `.pdf` file uses the Portable Document Format. It's a binary container with paged content,
optional attachments, and a metadata dictionary. peek shows it in several views powered by Pdfium.

`.ai` files from Adobe Illustrator CS2 (2005) and later are PDF 1.x internally. The default "Create
PDF Compatible File" save option embeds a full PDF rendering, so these files go through the same
Pdfium stack. The `.ai` extension selects an Illustrator flavor that differs in two ways. The Info
section gets the label Adobe Illustrator, and peek accepts `.ai` with `%PDF` magic without an
extension-mismatch warning. The render path is identical. Legacy `.ai` files from before CS2 are
pure PostScript, and peek doesn't support them yet. For details, see [planned.md](planned.md).

- **Read** (default): a paged image render. peek rasterizes each page with Pdfium and renders it as
  ASCII art through the shared image pipeline. This is the same `prepare_decoded` and
  `render_prepared` path that the comic-archive reader uses. `n` and `p` step through pages, and the
  status line shows `page X/Y`. Each page has a cache entry keyed on
  `(cols, rows, style, image-mode, background, fit)`. When you resize the terminal or cycle the
  background, image mode, or fit, peek re-renders only the visible page.

  `o` toggles the **reconstructed-text overlay** (`types/pdf/text_overlay.rs`):
  1. peek extracts the page's text layer as positioned words. Pdfium gives loose bounds for each
     character. peek groups the characters into words and snaps each line to its baseline.
  2. peek projects the words through the same mapping from page points to source pixels to effective
     cells that the rasterizer uses.
  3. Each word's characters overwrite the rendered glyph cells in its box, and the cells keep their
     SGR colors.

  peek orients the color pair for each cell. The half-block renderer assigns a cell's two pixel
  colors to foreground and background by position. The letter paints in whichever color is closer to
  the word's font color. The font color is the per-character fill color from Pdfium, the stroke
  color for outlined text, or black when neither is declared. If the two colors are too close to
  read, as on blank paper, peek nudges the contrast toward black or white. Padding spaces get the
  same orientation, with their background on the paper side. Orientation works the same in
  truecolor, 256-color palette, and 16-color output. peek compares escapes by their nominal RGB and
  swaps them by re-planning, so the stream stays in its palette.

  The overlay has these limits:
  - peek skips words shorter than about 0.55 cell rows, because their text lines would collide.
  - If a word's box is more than 3 times its character count, the rendered glyphs are large enough
    to read on their own. In that case peek keeps the surrounding glyphs and blanks only one
    delimiter cell on each side.

  A single-slot word cache for each page sits beside the bitmap cache. The toggle and the `text`
  status segment live in `PagedImageMode`. They're gated by `PageRenderer::supports_text_overlay`,
  so CBZ files and image-only PDFs don't advertise a key that does nothing.

- **Text**: text extracted from the whole document and wrapped to the width, with muted
  `--- Page N ---` markers between pages. It uses the same caching as DOCX and RTF: a single
  `(width, style_mode)` cache that peek rebuilds on resize. You reach it with `Tab`. The view exists
  only when the document has a text layer. Image-only scans and outlined vector artwork (`.ai`)
  yield no text, so peek skips the tab instead of showing it empty. peek probes the first few pages
  when it composes the views.
- **Embeds**: if the PDF has `/EmbeddedFiles` attachments, a `ListingMode` of those attachments. To
  extract the selected attachment, press `e` or `Enter`. peek extracts it as an
  `InputSource::Memory` and detects its type again through the nested-peek pipeline. For example, an
  attached CSV opens in a CSV view, and an attached image opens in the image viewer. The view is
  hidden when the PDF has no attachments.
- **Info**: the PDF version (`1.4`, `1.7`, and so on), title, author, subject, keywords, and the
  creation and modification dates. peek reformats the PDF `D:YYYYMMDDHHMMSSO…` date strings to
  `YYYY-MM-DD HH:MM:SS UTC` or `±HH:MM`. The view also shows the page count, the attachment count,
  and the inline-image count.

Print mode (`--print`) walks every page in order, with blank lines between them.
`cat file.pdf | peek` detects the `%PDF-` magic and opens the PDF views. A piped `.ai` file also
lands here and is labeled as plain PDF, because peek can only recognize the Illustrator flavor from
the `.ai` extension.

peek loads Pdfium dynamically from `libpdfium.dylib`, `.so`, or `.dll`, which ships next to the peek
binary in the release tarball. You don't need to install anything on your system to run it. For
encrypted or password-protected PDFs, peek shows the open error in the Info section instead of
crashing.

#### EPS / PostScript ✅

peek opens `.eps` and `.ps` files, which are Encapsulated PostScript (EPS) and plain PostScript.
PostScript is a _program_, not a data file. So peek offers up to two image views, plus source and
metadata views, depending on what's available:

- **Preview** (default when present): binary "DOS-EPS" containers (magic `C5 D0 D3 C6`) have a
  30-byte header that points at an embedded TIFF or Windows Metafile (WMF) preview that the designer
  baked in. peek extracts the TIFF preview and renders it through the shared image pipeline. This is
  instant and needs no interpreter. The Info view names WMF previews, but peek doesn't render them,
  because no pure-Rust WMF rasterizer exists.
- **Render**: a true Ghostscript rasterization. This view exists only when peek finds a `gs`
  interpreter on `PATH`. It renders page 1 at 150 DPI through the image pipeline. EPS files render
  with `-dEPSCrop`, and `.ps` files render the full page. The render is lazy. The subprocess starts
  only when you view the tab, so opening a file never waits for Ghostscript. peek **never bundles**
  Ghostscript, because of its AGPL and GPL licenses and its large C dependency. It's an optional
  runtime enhancement.
- **Source**: the PostScript program text. For a binary DOS-EPS file, peek slices the PostScript
  section out of the container, so the source view shows the program and not raw binary.
- **Info**: the Document Structuring Conventions (DSC) header fields (`%%Title`, `%%Creator`,
  `%%CreationDate`, `%%For`, `%%BoundingBox`, `%%LanguageLevel`, and `%%Pages`). It also shows the
  embedded-preview descriptor, with the kind and decoded dimensions, and whether Ghostscript is
  available. When Ghostscript is missing, the view shows an install hint.

An `.eps` or `.ps` file with no preview and no `gs` on `PATH` falls back cleanly to Source and Info.
Detection covers the `.eps`, `.epsf`, `.epsi`, and `.ps` extensions, the DOS-EPS magic, and a
`%!PS…` content sniff. If the version line contains `EPSF`, peek treats the file as EPS instead of
plain PostScript.

The single-bitmap render path decodes, fits, crops to the window, and renders ASCII art. It supports
zoom and pan. The PDF and CBZ page renderers share this path through
`viewer::paged::render_image_window`.

These features aren't done yet: legacy pre-CS2 Illustrator labeling, paging through multi-page `.ps`
files, and WMF and EPSI preview rendering. For details, see [planned.md](planned.md).

#### Spreadsheets ✅

peek opens `.xlsx`, `.xlsm`, and `.ods` workbooks. A workbook holds several named sheets, and each
sheet is a table. So the viewer mirrors the SQLite layout: a listing of entities, from which you
open one entity in a streaming table.

- **Sheets** (default): a listing of the workbook's sheets.
  - To open a sheet in an **aligned table view**, press `Enter`. The view is the shared
    `RowsTableMode`, with a sticky header, horizontal pan, and `/` search across all cells in the
    file.
  - To extract the sheet to a CSV file, press `e`.
  - Each column's alignment comes from calamine's native cell types. Int and Float align right.
    String, Boolean, and Date align left.
  - peek detects the header row with the same all-text heuristic as the CSV viewer. `Shift+H`
    overrides it.
- **Files**: the workbook's raw ZIP entries. The workbook is an OOXML or ODF ZIP container, so you
  can browse and extract its entries through the standard archive path.
- **Info**: the sheet count and names, plus core document properties: title, author, subject,
  keywords, created, and modified. peek reads them from `docProps/core.xml` for OOXML or `meta.xml`
  for ODS.

[`calamine`](https://docs.rs/calamine) does the parsing. It's not a spreadsheet engine and doesn't
evaluate formulas, so peek shows the cached cell values. peek picks the type by extension (`.xlsx`,
`.xlsm`, and `.ods`). Like every OOXML and ODF container, a workbook has `application/zip` magic
bytes, so a workbook without an extension opens in the archive viewer.

**Memory.** calamine has no streaming API for sheets. When you open a sheet, calamine parses the
whole sheet into memory. peek therefore holds one sheet at a time in memory, and switching sheets
drops the previous one. The CSV viewer, by contrast, uses a sliding window. Sheet size has an upper
bound, because Excel caps sheets at about 1M rows. peek also reads the container into memory,
because calamine needs random access. This is an accepted trade-off. A truly streaming reader would
need a custom parser for the sheet XML.

#### Presentations ◐

Presentations are the third format of the office trio. `.pptx`, `.pptm`, and `.ppsx` (Office Open
XML) and `.odp` (OpenDocument Presentation) files get **slide-by-slide text**. Apple Keynote
(`.key`) files get a **preview**.

- **Read** (default for PPTX and ODP): one slide at a time through the shared `document` prose
  renderer. The title placeholder becomes a heading and the bullets become the body. The flow
  mirrors EPUB chapters:
  - `n` and `p` step through slides.
  - `/` searches the current slide.
  - The status line shows `slide X/Y`.
  - `--print` walks every slide.
  - Embedded images show as `[Image: name]` references in the prose. These are references only. You
    can still reach the bitmaps through the Files TOC or `--extract`.
- **Preview** (default for Keynote): the embedded `preview.jpg` deck thumbnail, rendered through the
  image pipeline with zoom, pan, and fit. Modern `.key` files store slide text as undocumented
  snappy-compressed protobuf, so peek doesn't extract the text. The thumbnail and the Files TOC take
  its place.
- **Files**: the raw ZIP entries. Every presentation format is an OOXML, ODF, or iWork ZIP
  container, so you can browse and extract its entries through the standard archive path.
- **Info**: slide, word, and image counts, plus document properties: title, author, created, and
  application. peek reads them from `docProps/core.xml` and `app.xml` for OOXML, or from `meta.xml`
  for ODP. For Keynote, the view shows the build that created the file, from
  `BuildVersionHistory.plist`.

peek parses the slide XML with a hand-written `quick-xml` walk, the same approach as the DOCX
reader, and reuses the `document` AST and renderer. peek picks the type by extension. Like every
OOXML and ODF container, a deck has `application/zip` magic bytes, so a deck without an extension
opens in the archive viewer. `.key` is the exception. It shares its extension with PEM private keys,
so peek checks the start of the file for ZIP magic to tell a Keynote package from a text key.

#### SQL ◐

`.sql`, `.ddl`, `.dml`, `.psql`, and `.pgsql` files render as syntax-highlighted source. The Info
view adds an SQL section with these details:

- A heuristic guess at the dialect, which is one of PostgreSQL, MySQL, SQLite, T-SQL, or generic.
- The statement count, broken down by category. The categories are Data Definition Language (DDL),
  Data Manipulation Language (DML), Data Query Language (DQL), Transaction Control Language (TCL),
  and Other.
- Lists of the objects that the file creates, with their names: tables, views, indexes, functions,
  and triggers.
- The number of comment lines.
- A flag when the file has an inline `$$ … $$` PL/pgSQL block.

The scanner tracks whether it's inside a string, a comment, or a dollar-quoted body. As a result,
semicolons inside strings or procedural bodies don't split statements by mistake. A real formatter
and an outline mode are still planned.

#### CSS ◐

`.css` files render as syntax-highlighted source. The Info view adds a CSS section with these
details:

- The style-rule count, including nested CSS rules.
- The total selector count, with a histogram of occurrences by kind: class, ID, element, pseudo,
  attribute, and universal.
- The number of distinct custom properties.
- The `@media` and `@keyframes` counts.
- A list of `@import` rules. Absolute and protocol-relative URLs show in the warning style.

A Colors section shows the deduplicated color palette as block-glyph swatches, with the most
frequent colors first. `cssparser` and `cssparser-color` do the parsing. peek scans for colors only
inside declaration values. So a color word in a selector (`.gold`), in a string (`content: "red"`),
or in a comment never matches by mistake. Hex, `rgb()`, `hsl()`, `hwb()`, and named colors resolve
to swatches. peek counts colors in CIE and Oklab spaces but doesn't draw swatches for them.
Specificity annotations for each rule in the source view are still planned.

### Structured data and config files

| Format     | Extensions          | Status |
| ---------- | ------------------- | ------ |
| JSON       | `.json`, `.geojson` | ✅     |
| JSONC      | `.jsonc`            | ✅     |
| JSON5      | `.json5`            | ✅     |
| JSON Lines | `.jsonl`, `.ndjson` | ✅     |
| YAML       | `.yaml`, `.yml`     | ✅     |
| TOML       | `.toml`             | ✅     |
| XML        | `.xml`              | ✅     |
| CSV, TSV   | `.csv`, `.tsv`      | ✅     |

JSONC and JSON5 files open in the **raw** view by default. The pretty view collapses comments and
JSON5 syntax, so a pretty default would silently lose information. To see the strict-JSON pretty
form, press `r`.

JSON Lines files open in the pretty view by default. Each non-empty line round-trips through
`serde_json`, and a blank line separates the records.

#### CSV and TSV tables

CSV and TSV files open in an aligned table view. A sticky header row with a separator line under it
stays at the top.

**Memory.** A bounded sliding window over a seekable `csv` reader serves the body rows. The window
has two parts:

- A retained seed of the first 1000 records.
- A window that refills by seeking back to a sparse record-position anchor.

Resident memory stays flat whatever the file size and however far you scroll. A multi-GB file no
longer holds every row up to the deepest one you viewed.

**Column widths.** peek seeds column widths from the first 1000 records. Columns widen
automatically, and never narrow, as wider cells scroll into view. The sticky header repaints on
every width change. Columns shrink only when you press `Shift+R`, which reflows the widths from the
viewport.

**Header and panning.** A heuristic decides whether row 0 is a header. If every cell in row 0 is
text, the header is on. If row 0 has a typed cell, the header is off. To override the heuristic,
press `Shift+H`. When the table is wider than the terminal, `Left` and `Right` pan one column at a
time.

**Column types.** peek samples each column's type from the seed: `int`, `float`, `bool`, `date`,
`string`, or mixed. The info section shows the inferred types. Only `int` and `float` columns count
as numeric. They're right-aligned in the body and the header so that the digits line up. The info
section also shows the file's total record count, the delimiter, the encoding, and a counter of
malformed rows next to the column stats.

**Record count.** A one-time streaming pass that discards the cells settles the exact total record
count. The jump to the end of the file also waits for this pass. Until the pass finishes, the info
view shows `N (partial)`.

**Encoding.** UTF-8 is native. peek transcodes UTF-16 LE and UTF-16 BE to UTF-8 at the byte-source
boundary. The transcode reads the whole file into memory, so the 32 MB whole-document budget limits
it. If a UTF-16 file is over the limit, peek shows only the Source view with a warning instead of
failing to open the file.

**Cell contents.** A multi-line cell, which holds an embedded `\n` from a quoted record, collapses
to one visual row. A muted `↵` glyph marks each line break. A `\t` becomes a space and peek drops
`\r`, so no cell content can break the terminal cursor.

**Search.** To search, press `/`. The search looks inside single cells, matches substrings, and uses
smart case. It pages the window across the records instead of holding them all in memory. The walk
shares the 256 MB `SEARCH_SCAN_MAX_BYTES` budget. If a scan hits the budget, peek marks its counts
as partial (`12/3400+`, `no match (partial scan)`) and raises a warning. `n` and `p` step through
the matches. They pan columns and scroll rows to bring each match into view.

**Malformed records.** peek treats a record as malformed if it's over 4 MiB raw, spans over 10 000
physical lines, or the `csv` crate rejects it. It renders a malformed record as a single `<error>`
row in `theme.warning` and increments the status-bar counter.

**Print mode.** Print mode uses the seed widths only and doesn't widen columns. A long cell may
overflow to the right on its own row, and alignment resumes on the next row.

#### Viewing sub-modes

Structured data has two viewing sub-modes. To toggle between them, press `r` or use the `--raw`
option:

- **Pretty** (default): peek reformats the source and adds syntax highlighting.
- **Raw**: peek shows the source verbatim with syntax highlighting only.

`--plain` or `-P` turns off all styling.

### Image files ✅

peek renders raster formats as ASCII art. The `image` crate supports most formats. AVIF support
comes from `avif-decode` (rav1d), which peek registers as an `image` decoding hook in
`types/image/avif.rs`.

| Format  | Extensions             |
| ------- | ---------------------- |
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

peek has five ASCII-art rendering modes. To cycle through them, press `m`. To pick one from the
command line, use `--image-mode`.

| Mode      | Description                                                                   |
| --------- | ----------------------------------------------------------------------------- |
| `full`    | All glyphs: block, quadrant, and extended                                     |
| `block`   | Block and quadrant elements, and an ASCII subset                              |
| `geo`     | Block and quadrant elements, and line segments only                           |
| `ascii`   | Legacy density ramp based on luminance                                        |
| `contour` | Sobel edge detection rendered as line art (`--edge-density` tunes line count) |

In the viewer, Tab cycles through the file's view modes. For a raster image, the order is image and
then file info. For SVG, the order is image, SVG source, and then file info. Output uses 24-bit true
color. The status line shows the active mode.

#### Corrupt images

A corrupt or truncated image, such as one with a bad CRC or a partial payload, doesn't stop peek. In
the viewer, peek falls back to the Hex view and records the decode error as a warning. The warning
shows in Info, and a `!` on the breadcrumb flags it. In a pipe, peek falls back to a hex dump and
writes the cause to stderr.

This fallback to Hex is generic to the render loop. If any view mode fails to render an input, peek
shows the Hex view with a warning instead of crashing.

#### Decompression bombs

peek caps decode allocation. A decompression bomb is a tiny header that declares a gigapixel canvas,
or an animation that declares thousands of huge frames. peek reports an error and falls back to Hex
instead of running out of memory and getting killed.

- Static rasters inherit the 512 MiB default of the `image` crate through `ImageReader`.
- The GIF and WebP animation path builds its decoders directly, so it has no cap by default. peek
  sets the same per-frame limit on it _and_ caps the cumulative decoded-frame total at 1 GiB. The
  second cap covers WebP, whose decoder ignores the per-frame limit. It also covers many small
  frames, which the per-frame limit alone misses.

#### SVG ✅

SVG (`.svg`) is a vector format, and the `image` crate doesn't handle it. peek rasterizes SVG with
`resvg`.

SVG has two viewing modes. To cycle between them, press Tab:

- **Rendered preview** (default): peek rasterizes the image and renders it through the image
  pipeline.
- **Source view**: syntax-highlighted XML in the pretty or raw form.

The preview re-renders when you resize the terminal.

##### SVG animation ◐

peek supports CSS `@keyframes` animation in `types/image/pipeline/svg_anim/`. It works in three
steps:

1. The parser collects each `@keyframes` rule and the inline-style `animation-*` references on
   elements.
2. The parser builds a merged frame timeline. `steps()` timing gets one frame per stop, and `linear`
   timing gets about 30 interpolated frames per second.
3. `SvgAnimationMode` rasterizes each frame on demand from an SVG patched for that frame.

A bounded LRU (least recently used) cache holds 64 entries, keyed by
`(frame, grid_cols, grid_rows)`. After the first loop, a full second loop costs nothing.

Phase 1 covers what files in the style of termsvg and asciinema-svg use: `transform` with
`translateX`, `translateY`, or `translate` under `steps()` or `linear` timing.

- Targets resolve through an inline `style="..."` _or_ flat CSS selectors: tag, `.class`, `#id`, and
  `tag.class`. `types/image/pipeline/svg_anim/selectors.rs` parses the selectors.
- peek silently drops combinators, pseudo-classes, attribute selectors, and `*`.
- SMIL (`<animate>`, `<animateMotion>`) is still deferred.
- `--no-svg-anim` forces the static render.
- The Info panel reports the frame count, the total duration, and whether the animation loops or
  plays once.

#### Transparency handling ✅

Images with transparency (PNG, SVG, WebP, and GIF) need a background to composite on before ASCII
rendering. Without one, transparent regions default to black, which makes dark content invisible on
a dark terminal background.

| Background     | Description                                          |
| -------------- | ---------------------------------------------------- |
| `none`         | No compositing. Transparent regions render unchanged |
| `black`        | Solid black                                          |
| `white`        | Solid white                                          |
| `checkerboard` | Classic Photoshop-style pattern                      |

peek picks the background automatically. Dark content gets a white background, and light content
gets a black one. You can also set the background with `--background` or cycle it with `b`. The
checkerboard uses 8×8 pixel gray squares. peek always composites when an image has an alpha channel.
You can't turn compositing off for a single image.

#### Image sizing modes ✅

| Mode        | Behavior                                                                  |
| ----------- | ------------------------------------------------------------------------- |
| `Contain`   | Fits within both width and height, so the whole image shows (default)     |
| `FitWidth`  | Width fills the terminal. Height grows freely, so you scroll vertically   |
| `FitHeight` | Height fills the terminal. Width grows freely, so you scroll horizontally |

To cycle the sizing mode, press `f` in an image or SVG render view. Pipe and `--print` output always
uses `Contain`. Output rows have no bound there, so the other modes either make no sense or reduce
to `Contain`. peek never rotates the image. Only the constraining axis changes.

Scroll keys in image views:

- `Up`, `Down`, `PgUp`, and `PgDn` scroll vertically under `FitWidth`.
- `Left` and `Right` scroll horizontally under `FitHeight`.
- `Home` returns to the top-left corner, and `End` jumps to the bottom.

Changing the sizing mode resets the scroll offset, because the old position has no meaning in the
new grid. There's no `--sizing` option yet.

#### Zoom ✅

Every view that renders graphics supports zoom. That covers raster images, animations, animated SVG,
PDF and CBZ pages, the font specimen, and static SVG.

- `+` and `-` zoom in and out in steps of 1.25×. The zoom anchors on the center of the viewport, so
  the pixel under the center stays in place.
- `0` resets the zoom to 1× and pans to the origin.
- `1` to `9` jump to a whole-number preset, from 1× to 9×.
- The maximum zoom is 16×.

Every backend now renders only a region of interest (ROI). peek crops the pixel ROI of the visible
viewport from a source at native resolution and rescales it. Memory stays proportional to the
viewport, not to the square of the zoom. Each backend picks its own source strategy:

- **Raster, GIF, and WebP**: the source is the decoded native image.
- **CBZ**: a decoded native bitmap for each page. peek loads pages lazily and keeps them for as long
  as the renderer lives.
- **PDF**: peek rasterizes the page once at the 4096-pixel render ceiling of Pdfium. A single-slot
  cache holds only the current page, so a long document doesn't pile up rasterized pages. Past the
  render ceiling, the viewport upscales pixels instead of rasterizing again.
- **Font specimen**: peek rasterizes the active face again at a higher resolution when the zoom
  crosses into a new integer bucket (1×, 2×, 3×, and so on). A 1.25× step inside the same bucket
  reuses the existing source. Only crossing a bucket triggers a new `fontdue` pass.
- **SVG, static and animated**: the same integer-bucket strategy. When the zoom crosses the next
  integer, peek rebuilds the rasterized source bitmap at `bucket × base` resolution. ROI crops then
  read native `resvg` detail instead of upscaled pixels. The animated path caches each
  `(frame, grid, bucket)` in its bounded LRU cache, so a frame you revisit at the same zoom stays a
  cache hit.

Zoom works only in the interactive viewer. Pipe and `--print` output always renders at 1×.

### Audio files ✅

Audio files get an Info view with metadata only. peek doesn't play audio or draw a waveform.

A `symphonia` probe supplies the container and codec parameters: duration, channel count and layout,
sample rate, bit depth, and average bitrate. Tag fields come from these sources:

- ID3v1 and ID3v2 (MP3, AIFF)
- Vorbis comments (Ogg, FLAC, Opus)
- MP4 atoms (m4a, m4b)
- APE

The tag fields are title, artist, album, album artist, track and disc number, date, genre, composer,
and comment.

#### Embedded pictures and lyrics

Embedded pictures and lyrics get **dedicated Tab views**. Album art comes from `APIC`, FLAC PICTURE,
`covr`, and `METADATA_BLOCK_PICTURE`. Lyrics come from `USLT`, `SYLT`, and `LYRICS=`. Tab cycles
through the views in this order:

1. **Info**
2. **Cover**: the primary picture, rendered as ASCII art through the image pipeline. Cover prefers
   the picture tagged FrontCover and falls back to the first picture.
3. **Lyrics**: plain text.
4. **Embeds**: a table of contents (TOC) of every embedded item.

The Embeds listing shows `pictures/<usage>.<ext>` for each picture, such as front, back, artist, or
leaflet. It also shows `lyrics/lyrics.txt` when the file has lyrics. The `e` extract flow is the
same as for PDF embeds. For example, `--extract pictures/front_cover.jpg` writes out the cover. On a
recursive peek, extracted picture bytes re-enter the peek pipeline and render as ASCII art. Lyrics
re-enter as plain text.

| Format         | Extensions               | Status                                           |
| -------------- | ------------------------ | ------------------------------------------------ |
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
| WMA            | `.wma`                   | ◐ Container only. `symphonia` doesn't decode WMA |

### Animated images (GIF, WebP) ✅

Animations play automatically at their native frame rate.

- `Space` toggles between play and pause.
- `n`, `p`, `Left`, and `Right` step through frames.
- `b` cycles the background.

The status line shows the frame counter and whether the animation is playing or paused. Print mode
renders the first frame. The file info screen shows the frame count. Transparency handling applies
to animations too.

### Comic archives ✅

| Format | Extensions | Status |
| ------ | ---------- | ------ |
| CBZ    | `.cbz`     | ✅     |

A `.cbz` file (Comic Book ZIP) is a ZIP container that holds page images in name order. CBZ files
get a view with several modes:

- **Read** (default): a paged image render through the shared image pipeline. It uses the same
  `PagedImageMode<R>` machinery as PDF. `n` and `p` step through pages, and the status line shows
  `page X/Y`. peek decodes pages lazily and holds them in a single-slot cache. A resize, an image
  mode change, or a background change re-renders only the current page.
- **TOC**: the raw ZIP file tree, shown through the shared `ListingMode`. `e` or `--extract` pulls
  out a single page through recursive peek. For example, an extracted PNG opens in the image viewer.
- **Info**: the format label, the page count, and the total uncompressed size of the images.

peek keeps entries with an image extension (`.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`, `.bmp`,
`.tif`, or `.tiff`) as pages and sorts them by name. The Read view skips `__MACOSX/` and other
entries that aren't images, but they stay visible in the TOC. peek doesn't support the other comic
archive containers `.cbr`, `.cb7`, and `.cbt`. Only CBZ ships today.

### Object files ✅

ELF, Mach-O, PE/COFF, and WebAssembly binaries get a dedicated viewer instead of the binary hex
fallback. That covers executables, shared libraries, relocatable objects, and `.wasm` modules. The
`object` crate backs the viewer with one read-only API across every container format.

Detection works on magic bytes. The `infer` MIME type maps to `FileType::ObjectFile`, and an
explicit `\0asm` check covers WASM. A file without an extension, such as `/bin/ls`, still routes
correctly.

Object files have three views. To cycle through them, press Tab:

- **Info** (landing view): this view covers what `file`, `readelf -d`, and `otool -L` show together:
  - Format and architecture.
  - File kind: executable, relocatable object, dynamic library, or core dump.
  - 32-bit or 64-bit, and endianness.
  - Entry point, and section and symbol counts.
  - Whether debug info is present.
  - Build identity, which is the ELF build ID, Mach-O UUID, or PE PDB GUID.
  - Linked libraries, which are the ELF `DT_NEEDED` entries, Mach-O dylibs, or PE imports.
- **Sections**: a table in the style of `readelf -S` with index, name, address, size, and kind.
- **Symbols**: a listing in the style of `nm` with address, size, type, bind, and name. It prefers
  the full `.symtab` and falls back to the dynamic symbol table for a stripped file. `/` searches
  symbol names. `Enter` **jumps the Hex view to the symbol's byte offset**, which peek recovers from
  the file range of the section that holds the symbol. Undefined and `.bss` symbols show in the list
  but can't jump, and their address shows muted.

The Sections table uses the shared `TableMode`, the same one that classfiles use:

- The column header stays pinned when you scroll vertically.
- Each column repaints live when you cycle the theme.
- `Left` and `Right` pan columns.
- `/` searches names. `n` and `p` step through matches and pan horizontally only as far as needed to
  reveal a match that's off screen.
- Column widths fit their content.

peek unwraps universal (fat) Mach-O containers transparently. It parses the slice for the host
architecture, and the Info view lists every slice. There's no extract path, because sections and
symbols aren't standalone files.

| Format      | Coverage                                                            |
| ----------- | ------------------------------------------------------------------- |
| ELF         | executables, shared objects (`.so`), relocatable objects (`.o`)     |
| Mach-O      | executables, `.dylib`, `.o`, and unwrapped universal (fat) binaries |
| PE/COFF     | Windows executables and DLLs                                        |
| WebAssembly | `.wasm` modules (functions surface as symbols)                      |

`ObjectMeta` carries the `object` enum values (`BinaryFormat`, `Architecture`, `ObjectKind`, and
`Endianness`). Only `info_render` maps them to display labels.

Bare COFF `.obj` files have no dedicated magic. peek detects them by validating the full COFF
header: a known machine, no optional header, a sane section count, and a clear executable-image
flag. The check is strict enough that a Wavefront `.obj` 3D model stays text.

[planned.md](planned.md) tracks the remaining deeper inspection, such as compiler and toolchain
notes and interactive switching between fat slices.

### Java classfiles ✅

`.class` files hold JVM bytecode. They get a dedicated viewer through the `cafebabe` crate.

Detection works on magic bytes. The classfile magic, `CA FE BA BE`, is the same byte for byte as the
magic of a Mach-O fat (universal) binary. `head_magic_mime` distinguishes them by the field at
offset 6. In a classfile, that field is `major_version`, which is at least 45 (JDK 1.0). In a fat
Mach-O, that field is the `nfat_arch` slice count, which is small (under 45 in any real binary). The
field separates the two cleanly.

Classfiles have four views. To cycle through them, press Tab:

- **Info** (landing view): the class name, superclass, interfaces, and JDK version. For major
  version 49 and later, the JDK version is the classfile `major − 44`, so 52 is Java 8 and 61 is
  Java 17. Info also shows the kind (`public final class`, `interface`, or `enum`), the `SourceFile`
  attribute, and the field and method counts.
- **Fields**: a table with modifiers, type, and name.
- **Methods**: a table with modifiers, name, and signature. peek decodes descriptors to source form,
  so `(Ljava/lang/String;I)V` renders as `(String, int) -> void`.
- **Bytecode**: a disassembly of every method in the style of `javap -c`. Each line shows the byte
  offset, the mnemonic, and the resolved operand. Member references show as `class.name:descriptor`,
  simple branch targets as absolute offsets, and switches as an entry count. `n` and `p` jump
  between methods, and `/` searches the listing. peek parses this view separately with bytecode
  turned on, so a decode failure here leaves the cheaper metadata views intact.

peek colors field types and method signatures the way a Java or Rust highlighter would. Primitive
types, class names, array brackets, and punctuation each get their own theme color, so you can read
a signature at a glance.

Both tables use the shared `TableMode`, the same mode that object files use. It has a sticky header,
columns fitted to content, horizontal panning, and `/` search.

The viewer departs from a naive `javap` port in two deliberate ways:

- **No constant-pool count.** The constant-pool iterator in `cafebabe` skips `Utf8` entries, so a
  `count()` reports only a fraction of the true pool size. A wrong number is worse than none, so
  peek leaves the field out instead of showing a misleading value.
- **`descriptor` is a formatter, not a parser.** `cafebabe` already parses descriptors into typed
  values, but its `Display` emits the raw JVM form again (`(I)V`). The `descriptor` module turns
  those typed values into readable spans tagged with colors. It never parses raw descriptor strings
  again.

There's no extract path, because fields and methods aren't standalone files.

### SQLite databases ✅

`.sqlite`, `.sqlite3`, `.db`, and `.db3` files open as a read-only browser over the SQLite schema.
The bundled `rusqlite` handles detection, reading the schema, and reading rows. Its `bundled`
feature compiles in the upstream SQLite C amalgamation, so peek needs no system `libsqlite` at run
time.

SQLite databases have two views. To cycle between them, press Tab:

- **Listing** (landing view): the groups `tables/`, `views/`, `indexes/`, and `triggers/`, with one
  leaf per entity. Tables and views get two leaves: `<name>.sql` for the `CREATE …` DDL (data
  definition language) and `<name>.csv` for the contents. Indexes and triggers get only `.sql`. peek
  leaves out empty groups. The size column shows the DDL byte length for schema rows and the row
  count for contents rows, so you can compare table sizes at a glance.
- **Info**: page size, page count, encoding, journal mode, schema version, user version, application
  ID (when it isn't zero), the result of `PRAGMA integrity_check(1)`, entity counts, total rows, and
  the five biggest tables.

#### Drill-down

What Enter does depends on the leaf suffix.

On a `<name>.sql` leaf, Enter writes the entity's `sqlite_master.sql` into an in-memory `.sql`
source with a `-- <name> from <db>` header comment. The outer re-detect routes it through the
existing SQL syntax view. You can also extract schema rows with `e` and save them like any other
archive entry.

On a `<name>.csv` leaf, Enter opens a streaming rows view that mirrors the CSV table viewer. It has
a sticky header, horizontal panning, and `/` search inside cells.

- A sliding-window cursor buffers 1000 rows at a time. When you scroll outside the window, peek
  refills it with `SELECT * FROM "<entity>" LIMIT 1000 OFFSET k`.
- `COUNT(*)` runs once when the view opens, so the scrollbar and `Bottom` math are exact without a
  full scan.
- NULL renders differently from the empty string, because cells are `Option<String>` across the
  shared `RowSource` trait.
- BLOBs render as `<blob: N bytes>`. An inline hex preview is still to come.
- The declared type affinity sets each column's alignment. `INT`, `REAL`, `NUMERIC`, and `DECIMAL`
  columns align right. Columns with text-like types (`CHAR`, `CLOB`, `TEXT`, `DATE`, `TIME`, and
  `BOOL`) stay left.
- `e` extracts the rows to a CSV file on disk. peek streams `SELECT *` through a `csv::Writer` into
  a temporary file. NULL becomes an empty cell, and numbers and text use their display form. A BLOB
  becomes a SQL hex literal `X'…'`, which is lossless and works in an `INSERT` statement.

#### Sources without a path

A source without a path on disk, such as stdin, an in-memory source, or an entry extracted from
another container, spools to a `NamedTempFile`. The file lives as long as the connection, so piped
databases work too. peek never writes to the database. Connections open with
`SQLITE_OPEN_READ_ONLY`.

#### Search and deferred work

Search inside cells spans the whole table. It pages the sliding window across every row with
repeated `ensure_row` calls, so it's exhaustive without holding the whole table in memory.

These features are still to come:

- Pushing the predicate down into `LIKE` or `GLOB` queries, which would run the match in SQL instead
  of row by row.
- Inspection of WAL (write-ahead log) and `-journal` sidecar files.
- Databases encrypted with SQLCipher.
- A prompt for custom queries.

### LMDB databases ✅

peek opens LMDB environments read-only. An environment is a `data.mdb` file, or a single-file
(`MDB_NOSUBDIR`) environment under any name. peek reads the pages itself through a `ByteSource`. It
doesn't link liblmdb, because liblmdb maps the whole file into memory with `mmap` and writes the
lock file even on a read-only open.

peek supports both 64-bit little-endian formats: 0.9 (data version 1) and 1.0 (data version 3, with
a 24-byte page header). Detection uses only the meta-page magic, because MS Access also uses `.mdb`.

LMDB environments have these views:

- **Databases** (landing view when named sub-databases exist): a `ListingMode` with one row per
  database and its size on disk (pages × page size). The list includes `@main` when the main
  database also holds plain keys. Enter opens a database's rows.
- **Rows** (landing view when there are no named sub-databases): the shared `RowsTableMode` with the
  row cursor on, showing `Key │ Value │ Size`.
  - Each `(key, value)` pair gets one row, so a `dupsort` key spans one row per duplicate.
  - Text shows as text, and other bytes as hex. `integerkey` and `integerdup` values show as
    numbers.
  - The row total comes from the database record. A leaf index that peek builds on demand gives
    random access.
  - Value cells preview 256 bytes, so an overflow value costs one small read.
- **Info**: format, page size, map size, pages used, last transaction ID (`txnid`), active meta
  page, free-list pages, and one block per database. Each block shows entries, depth, branch, leaf,
  and overflow pages, and flags.

Enter on a row opens the value as its own source, and peek detects its type again (JSON, image, and
so on). `e` saves the value. An overflow value comes back as a `FileRange` over the database file,
so a value of any size opens without a copy.

The extract key is `<db>/<key>[/<dup>]`. It uses `@main` for the main database and `x:<hex>` for
keys that aren't plain text. Extract looks the key up by descending the B+ tree. If a comparator
that the app defines defeats the descent, extract falls back to a full scan.

The walk is defensive. peek caps the depth at 32, and a walk may visit at most as many pages as the
file holds. A crafted cycle or shared subtree ends in an error instead of a hang. peek detects
32-bit, big-endian, and encrypted environments and reports them in Info, without a table.

### Certificates and keys ◐

Cryptographic material gets an Info section for each entry. The source view depends on the
container:

- PEM shows its text.
- JWK (JSON Web Key) shows the pretty-printed JSON.
- Raw DER is binary, so it gets only Info and hex.

#### Detection

Detection works both by extension and by content.

Extension routing covers these files:

- PEM: `.pem`, `.csr`, `.crl`, `.key`, `.p7b`, `.p7c`, and `.pub`
- DER: `.der`
- JWK: `.jwk` and `.jwks`

The content sniff catches these:

- `-----BEGIN ` headers.
- OpenSSH algorithm prefixes (`ssh-rsa`, `ssh-ed25519`, `ecdsa-sha2-*`), including the FIDO/U2F
  `sk-*` variants.
- A JSON object whose `kty` is a known key type.
- Raw DER.

`.crt` and `.cer` files deliberately route by content, because they carry _either_ PEM or DER. A
leading `-----BEGIN ` picks PEM. A leading `0x30 0x82` SEQUENCE that fully decodes as an X.509
certificate picks DER. The DER and JWK sniffs both parse the content instead of only matching a
pattern, so unrelated ASN.1 or JSON doesn't get the wrong label.

A JWK in a file named `.json` keeps the generic JSON view. The JWK sniff fires only for `.jwk` and
`.jwks` files, stdin, and files without an extension, where the extension isn't already
authoritative.

#### Decoded entries

A single PEM file may carry many entries, such as a full-chain bundle or an export with several
blocks. A JWK Set holds one entry for each member of `keys`. A DER file is one entry. Each entry
renders as its own block under the info section, headed **PEM**, **DER**, or **JWK** by source.

DER carries no label, so peek recovers its kind from its structure. It tries an X.509 certificate,
then a CRL, then a CSR, and then a PKCS#8 or SPKI key. The first one that decodes wins.

| Entry             | Surface fields                                                                                                                                                                                                                                                                                                                                       |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| X.509 certificate | label, version, subject, issuer, serial (hex), NotBefore and NotAfter (UTC ISO 8601), days remaining (warning color at 30 days or less, negative when expired), public key algorithm and bits, signature algorithm, SANs (DNS, IP, email, and URI), CA flag, self-signed flag, key usage, extended key usage, SHA-1 fingerprint, SHA-256 fingerprint |
| CSR (PKCS#10)     | label, subject, requested SANs (DNS, IP, email, and URI), public key algorithm and bits, signature algorithm                                                                                                                                                                                                                                         |
| CRL               | label, issuer, This Update and Next Update, revoked entry count, signature algorithm                                                                                                                                                                                                                                                                 |
| Private key       | label, key type (RSA, EC with curve, Ed25519, DSA, or opaque), bit size (best effort from PKCS#1, SEC1, or PKCS#8). Encrypted and opaque keys (`ENCRYPTED PRIVATE KEY`, `OPENSSH PRIVATE KEY`) show structural info only, with no password prompt                                                                                                    |
| Public key        | label, key type, bit size (parsed from the SPKI envelope)                                                                                                                                                                                                                                                                                            |
| SSH public key    | algorithm, bits, comment, SHA-256 fingerprint (matches `ssh-keygen -l -E sha256` output)                                                                                                                                                                                                                                                             |
| JSON Web Key      | type (RSA, EC, oct, or OKP) and curve, bit size (RSA modulus, curve, or `oct` secret), algorithm, use, key ops, key ID, RFC 7638 thumbprint (`SHA-256:` base64url)                                                                                                                                                                                   |

A decode failure doesn't hide the rest of the section. A malformed block gets its own **Parse
error** row, so a single bad PEM in a chain doesn't hide the others. A PEM label that peek doesn't
recognize shows as an `Unknown` entry that still records the label and the size of the DER body.

#### Crates

- `pem` parses PEM blocks.
- `x509-parser` parses certificates, CSRs, and CRLs.
- `ssh-key` parses OpenSSH public-key text.
- `sha1` and `sha2` compute fingerprints.
- `serde_json` parses JWK.

Bit-size inference for private keys uses a hand-written ASN.1 TLV walker over PKCS#1, SEC1, PKCS#8,
and SPKI envelopes. The walker is small enough to inline instead of pulling in `pkcs8`, `sec1`, and
`pkcs1` separately. The JWK thumbprint reuses the shared base64url codec in `crate::base64`.

peek doesn't yet support PKCS#12 and PFX, or prompt for the password of an encrypted PKCS#8 key.
[planned.md](planned.md) tracks both.

### Fonts ◐

peek decodes these font files into a themed **Font** info section:

- TrueType and OpenType files (`.ttf` and `.otf`).
- Font collections (`.ttc` and `.otc`).
- The WOFF and WOFF2 web wrappers (`.woff` and `.woff2`).

Detection works in two ways. Extension routing covers the six extensions. A magic-byte sniff catches
`00 01 00 00`, `OTTO`, `ttcf`, Apple's `true` variant, `wOFF`, and `wOF2`, so sources without a
name, such as stdin and archive entries, still classify. Fonts have no source view. They are binary
containers, so you inspect their raw bytes in the universal hex auxiliary mode.

peek unwraps both web wrappers to their inner sfnt before the metadata and specimen pipeline runs.
This way, every downstream consumer sees a plain font.

- **WOFF 1.0** wraps an ordinary sfnt and compresses each table with zlib. peek unwraps it in-tree:
  it rebuilds the offset table and the directory and inflates each table. It uses the `flate2` zlib
  decoder that the project already depends on, so it adds no new dependency.
- **WOFF 2.0** uses a single brotli stream plus a transform of the `glyf` and `loca` tables, so
  decompression alone isn't enough. The `glyf` table must be re-encoded, and `loca` must be rebuilt
  from it. peek delegates this to `wuff`, which reconstructs the sfnt. `wuff` is pure Rust and uses
  `brotli-decompressor`, so it needs no C++ foreign function interface (FFI).

Detection of both formats relies on content. Unlike a bare `.br` stream, `wOFF` and `wOF2` carry
magic signatures.

For each face, the info section shows:

- The family, subfamily, full name, PostScript name, and version.
- The OS/2 weight from `100` to `900`, with the canonical name in parentheses. Also the width class,
  the italic flag, and the monospaced flag.
- The glyph count, units per em, and codepoint count. The codepoint count is the sum across all
  Unicode cmap subtables.
- Script coverage: a list of scripts bucketed from the cmap code-point ranges. The buckets are
  `Latin`, `Latin Extended`, `Greek`, `Cyrillic`, `Hebrew`, `Arabic`, `Devanagari`, `Thai`,
  `Hangul`, `CJK`, `Emoji`, and `Symbols`.
- A flag that shows whether the font has hinting, from bit 0 of `head.flags`.
- The designer, vendor URL, copyright, and license URL.

Apple system fonts still ship their canonical name records on the Macintosh platform, encoded in Mac
Roman. For this reason, the `name` decoder handles both Mac Roman and UTF-16BE, which the Windows
and Unicode platforms use. It decodes Mac Roman with the full upper-half mapping. Characters such as
`©`, `™`, and accented Latin letters round-trip cleanly.

**Specimen view.** When you open a font, peek shows a rasterized sample sentence by default. The
sample is a hard-coded pangram, the digits, and the ASCII alphabet. peek rasterizes it with
`fontdue` at a fixed pixel size and sends the result through the existing ASCII image pipeline.
Every image-mode key works on the specimen:

- `m` cycles through the image modes `full` (full color), `block`, `geo`, `ascii`, and `contour`.
- `b` cycles backgrounds.
- `f` cycles fit modes.

Collections (`.ttc` and `.otc`) expose every face. To step the active face through the specimen in
place, press `n` or `p`. The status line shows `Face N/M`. The Info screen lists a metadata block
for every face, with the family, subfamily, weight, glyphs, scripts, and more. If `fontdue` can't
parse a face, the previous specimen stays in place and the view doesn't go blank.

peek uses these crates:

- `ttf-parser` walks the `name`, `head`, `maxp`, `cmap`, `OS/2`, and `post` tables. It is pure Rust,
  `no_std`, and doesn't allocate.
- `fontdue` rasterizes the specimen.
- `wuff` unwraps WOFF2.

Multi-script sample sentences keyed on cmap coverage are [planned](planned.md#font-files-).

### `.DS_Store` ✅

A `.DS_Store` file is the Desktop Services Store that Apple Finder keeps in each folder. It is a
"Bud1" container that uses a buddy allocator. peek parses the block store and its single `DSDB`
B-tree read-only and collects every record. Each record holds a filename, a structure ID, and a
typed value.

peek offers two views:

- **Records** is the landing view. It is a `viewer::table::TableMode` with one row per stored
  property and the columns `File │ Property │ Code │ Value`. Rows arrive in B-tree key order, which
  sorts them by filename. Friendly `Property` labels cover the common Finder codes. For the long
  tail, the label shows `—` and the raw `Code` speaks for itself.
- **Info** shows the record count and the number of distinct tracked filenames. If the folder stores
  its own view style or background, Info shows those too.

peek decodes these values:

- `Iloc` and `dilc` become `(x, y)` icon coordinates, or `auto`.
- `fwi0` becomes the window-frame bounds and the view style.
- `vstl` becomes the name of the view style in the menu.
- `BKGD` becomes `default`, `color #RRGGBB`, or `picture`.
- `modD` and `moDD` become a UTC date. The stored value is an 8-byte little-endian `CFAbsoluteTime`
  double that counts seconds since 2001. peek formats it with the date formatter of the info layer.
- Embedded binary property lists (`bwsp`, `icvp`, and `lsvp`) show as `binary plist, N bytes`. peek
  doesn't expand them.

Detection uses the `\0\0\0\1Bud1` magic, so renamed stores and stores piped through stdin route
correctly. Detection also uses the canonical `.DS_Store` filename.

The walk is defensive. If it meets a malformed node or an unknown record encoding, it stops. It then
flags the Records table and the `Note` row in Info as truncated, and it keeps the records gathered
so far.

The format has no source view, because it is opaque binary. To see the raw bytes, press `x` to open
the hex view. It also has no extract path, because records aren't files. The parser is hand-rolled
and adds no dependency.

### Binary and archive files ◐

Some files have no specialized viewer in peek, such as firmware images and opaque container formats.
For these files, the baseline is the **file info screen**, which shows:

- The file type and MIME type. The `infer` crate detects them from magic bytes.
- The size, both exact and human-readable.
- Filesystem metadata, such as permissions and timestamps.

`infer` provides only the MIME type and no deeper metadata. Dedicated parsers could add
format-specific details later.

Binary files open in the hex-dump viewer by default. The viewer uses a layout like `hexdump -C`,
adapts to the terminal width, and streams through `ByteSource`. From the hex view, press Tab or `i`
to reach the file info. You can also get it with `--info`. `--plain` (`-P`) still uses hex for
binary files. There is no format-specific deep metadata yet.

#### Archive listing ◐

Container archives open in a **TOC view**, with one row per entry. Each row shows the permissions,
the uncompressed size, the modification time, and the path. The listing reads only the header of
each entry, so multi-GB archives open instantly.

Use these keys in the TOC view:

- Up and Down move a file-selection cursor and skip directories.
- Top and End jump to the first and the last file.
- PgUp and PgDn scroll a page and then snap the selection to the first visible file.
- `e` extracts the selected entry. See [Extraction](#extraction-) below.
- Tab cycles between the TOC and Info.
- `x` still opens the raw hex dump of the archive bytes.

The selected leaf gets a highlighted background and an arrow marker.

To search leaf names, press `/`. The query matches only the last path segment of each row. For
example, `sub/` finds nothing, because no leaf contains a slash. Directory leaves take part in the
search, so a search for an ancestor's name brings that subtree into view. The file selection moves
only when the active match lands on a file row. As a result, Extract and Descend still target an
entry that you can descend into. `n` and `p` step through the matches and wrap around at the ends.

The same `/` search works in every `ListingMode` consumer:

- Archives.
- ISO 9660 images.
- PDF `/EmbeddedFiles`.
- Bundles of embedded files in audio files.
- Directories.
- Comic archives.
- The ZIP TOC of EPUB, DOCX, and ODT files.

| Format       | Extensions                     | Status |
| ------------ | ------------------------------ | ------ |
| ZIP          | `.zip`, `.jar`, `.war`, `.apk` | ✅     |
| Tar          | `.tar`                         | ✅     |
| Tar + gzip   | `.tar.gz`, `.tgz`              | ✅     |
| Tar + bzip2  | `.tar.bz2`, `.tbz2`            | ✅     |
| Tar + xz     | `.tar.xz`, `.txz`              | ✅     |
| Tar + zstd   | `.tar.zst`, `.tzst`            | ✅     |
| Tar + lz4    | `.tar.lz4`, `.tlz4`            | ✅     |
| Tar + brotli | `.tar.br`, `.tbr`              | ✅     |
| 7-Zip        | `.7z`                          | ✅     |
| cpio         | `.cpio`                        | ✅     |
| cpio + gzip  | `.cpio.gz`                     | ✅     |
| ar / Debian  | `.ar`, `.deb`, `.a`            | ✅     |

peek doesn't support RAR (`.rar`) yet. See [planned.md](planned.md).

The Info view shows the counts of entries, files, and directories, and the total uncompressed size.
If the listing fails, for example because the archive is corrupt or uses an unsupported variant,
Info shows a warning row and the TOC view is empty.

If the members of an `ar` archive are object files, the archive is a static library (`.a` or
`.lib`). In that case, the Info view adds a **Static library** section with the count of object
members and the target architecture. peek reads the architecture from the first object member. An
`ar` archive without object members, such as a `.deb`, doesn't get this section.

When you scroll, a **sticky parent breadcrumb** pins the ancestor chain of the current top row to
the upper rows of the viewport. Even in the middle of the tree, the path back to the root stays
visible. Disk-image listings use the same TOC code, so the breadcrumb behaves the same there. The
breadcrumb takes at most one third of the viewport height. It is hidden when the scroll is at the
top or when the top row is a top-level entry. To toggle it, press `s`. When it is off, the status
bar shows `sticky off`.

#### Single-stream compression ✅

peek decompresses bare single-stream codec wrappers transparently. It opens straight to the inner
content and renders it as whatever it is, such as source, JSON, or an image. There is no detour
through a TOC. The info view adds a Compression row that shows the codec and the size before and
after decompression. If decompression fails, peek falls back to a Hex view of the raw compressed
bytes and adds a warning row to the info view.

| Format | Extensions | Status |
| ------ | ---------- | ------ |
| gzip   | `.gz`      | ✅     |
| bzip2  | `.bz2`     | ✅     |
| xz     | `.xz`      | ✅     |
| zstd   | `.zst`     | ✅     |
| lz4    | `.lz4`     | ✅     |
| brotli | `.br`      | ✅     |

The decompressed output is capped at 256 MiB. If the output is larger, peek shows a warning and the
raw compressed bytes. This is the same result as the fallback for a corrupt stream.

peek detects every codec except brotli by both magic bytes _and_ extension. A raw brotli stream has
no signature, so peek detects `.br` and `.tar.br` by extension only. A brotli stream without an
extension, or one piped through stdin, doesn't classify automatically.

#### Disk images ✅

| Format | Extensions            | Status                                                   |
| ------ | --------------------- | -------------------------------------------------------- |
| ISO    | `.iso`                | ✅ PVD metadata + recursive directory listing (Joliet)   |
| DMG    | `.dmg`                | ✅ UDIF trailer + plist partition map (no inner-FS walk) |
| Raw    | `.img`, `.bin`, `.dd` | ✅ MBR partition table walk in info (no listing)         |

##### ISO 9660

An **ISO 9660** image opens to a **TOC view**. This is the same tree-style listing that archive
containers use. Each file or directory gets one row with its size, modification time, and 8.3 or
Joliet name. Indented tree glyphs show the depth.

The walker reads the extent of the root directory from the Primary Volume Descriptor (PVD) and
recurses through the child extents. If the image has Joliet, the walker reads the Supplementary
Volume Descriptor (SVD) instead, because Joliet allows longer Unicode names. peek doesn't parse the
Rock Ridge System Use Sharing Protocol (SUSP) fields, so it can't show the permissions of each
entry. Instead, the renderer falls back to typical defaults: `rwxr-xr-x` for directories and
`rw-r--r--` for files. Limits on depth and on entry count defend against malformed images.

The info screen (`i`) still shows the ISO **metadata**:

- The volume label, volume set, system ID, publisher, data preparer, and application.
- The volume size in blocks.
- The four PVD timestamps: creation, modification, expiration, and effective.

The descriptor walk also reports whether the image has the Joliet extension and an El Torito boot
record.

##### DMG

A **DMG** file opens straight to the file info screen. It has no TOC listing, because the inner
filesystem, such as HFS+, APFS, or FAT, would need its own walker.

The metadata of an **Apple Disk Image**, which uses the Universal Disk Image Format (UDIF), comes
from the 512-byte "koly" trailer at the end of the file. The info screen shows:

- The UDIF version.
- The image variant: device, partition, or mounted system.
- The total uncompressed size and the length of the data fork.
- The size of the embedded XML partition map.
- The segment number and the segment count.
- The checksum algorithms for the data and the master checksum.
- The documented trailer flag bits: flattened and internet-enabled.

peek decodes the **partition map** from the embedded XML plist that the trailer points at. It reads
the plist region once. That region is a few KB, and peek reads no payload bytes. For each `blkx`
table, peek reads:

- Its Apple type token, parsed from the entry name.
- Its logical size, which is the sector span times 512.
- Its start sector.
- A compression summary from the `mish` block table of the entry.

The rows split into two groups:

- Each **filesystem** partition, such as `Apple_HFS` or `Apple_APFS`, gets a detail block. Any type
  that peek doesn't recognize also lands in this group. The block shows:
  - The full name and a friendly type, such as `HFS+` or `APFS`.
  - The logical size, and the stored size with its percentage.
  - The codec and the compression ratio. The codec is zlib, bzip2, lzfse, lzma, or Apple Data
    Compression (ADC).
  - A histogram of run types, such as `408 (1 raw, 4 ignore, 403 zlib)`.
  - The image offset in bytes and in sectors.
- The **scaffolding** of the format collapses into one compact `Partition scheme` block. The
  scaffolding is the protective MBR, the primary and backup GPT header and table, and the gaps of
  free space. Each gets one line with its size, its codec or `sparse`, and its offset.

Nothing is hidden. The split puts the substance up front. peek shows the image offset for every
partition, including the scaffolding. You can pass it to tools such as `mount -o offset=`,
`dd skip=`, and `mmls`.

peek reads the `mish` runs only for their structure. Rebuilding the payload of a partition by
decompressing the runs, and walking its inner filesystem, are deferred. See
[planned.md](planned.md#disk-images-).

The parsers are hand-rolled and need no extra crate. `dmg_plist.rs` walks the plist with
`quick-xml`. The DOCX and ODT readers already depend on `quick-xml`. `mish.rs` parses the block
tables. The hex view (`x`) still works on the raw image bytes.

#### Filesystem directories ✅

`peek <dir>` opens a listing of one directory level instead of failing with an "is a directory"
error. Entries sort with directories first, then by name without regard to case. The columns for
permissions, size, modification time, and name match the archive TOC view. A synthetic `..` row
leads the list, so you can walk back up. The row is hidden at the filesystem root. When you select
it, peek resolves the current path to its canonical form and moves to its parent.

Use these keys in a directory listing:

- **Enter** descends into the selected entry. On a file, peek pushes a new view, and Esc returns you
  to the listing. On a directory, peek moves the current frame to that directory. This way, there is
  no stack of directories to back out of.
- **Backspace** walks up to the parent directory directly. It has the same target as the `..` row.
  When you walk up, the cursor lands on the subdirectory you came from instead of the top of the
  list. This way, your place stays put as you step in and out of sibling directories. In an archive
  or container TOC, directory rows are selectable, so Backspace instead moves the selection to the
  parent directory row, one level per press.
- **z** toggles the size column between exact byte counts and human-readable units, such as KiB,
  MiB, and GiB. It works in every listing view: directories, archive and container TOCs, and email
  attachments. Exact bytes are the default, and `--list` and `--print` always show exact bytes.
- **Esc** exits peek from any directory listing.
- `/` searches entry names. It is the same leaf-name search as in the archive TOC. `n` and `p` step
  through the matches, wrap around at the ends, and move the selection to each match.

The listing includes hidden entries. peek follows symlinks to classify their kind and shows broken
links as `?`. Both `--print` and `--list` render the listing.

#### Hex dump mode ✅

The hex view reads bytes from disk on demand and never reads the whole file. The layout is
compatible with `hexdump -C`:

1. An 8-digit offset.
2. Two hex columns of N/2 bytes each, separated by an extra space.
3. A column of printable ASCII between `|` characters.

The number of bytes per row (`bpr`) scales with the terminal width. A row takes `14 + 4*bpr`
columns. peek rounds `bpr` down to a multiple of 8, with a minimum of 8. In pipe mode, peek uses
`$COLUMNS` if it is at least 24 and falls back to 16 otherwise.

To open the hex view from any view, press `x`. The viewer keeps a logical `Position`, which is a
byte offset or a line index. It captures the position when you leave any mode that tracks position,
and it restores the position when you return. When you enter hex from a text view, the top row is
the byte offset of the current line. peek finds the offset with `InputSource::line_to_byte`, which
is approximate for pretty-printed content. When you return to text, the line scroll lines up again.
Modes that don't track position, such as Info, Help, Image preview, and Animation, leave the saved
position untouched. This way, detours keep your place.

If you press `x` again, peek returns to your last primary mode, which is the most recent mode that
isn't auxiliary. This works no matter how many detours you took in between. When hex is the default
view for a binary file, there is no primary mode, so `x` does nothing there.

## Viewer features

### Color modes ✅

To pick a color mode, use `--color` (`-C`) or the `PEEK_COLOR` environment variable. The default is
`auto`. The other five values are encodings:

| Mode        | Encoding                                               |
| ----------- | ------------------------------------------------------ |
| `auto`      | `truecolor` on a terminal, `plain` otherwise (default) |
| `truecolor` | 24-bit RGB (`\x1b[38;2;r;g;bm`)                        |
| `256`       | xterm 256-color palette (`\x1b[38;5;Nm`)               |
| `16`        | 16 base ANSI colors (`\x1b[3Nm` / `\x1b[9Nm`)          |
| `grayscale` | 24-bit luminance only, preserves shading               |
| `plain`     | no escapes, strips all color from the output           |

To cycle modes in the viewer, press `c`. Each cycle invalidates the cache of rendered lines, so the
whole UI repaints in the new encoding.

All callers paint truecolor RGB. The `StyleMode` enum on `PeekTheme` owns the conversion and is the
single place that decides the encoding. Image rendering uses the same route through
`StyleMode::write_fg` and `write_fg_bg`. Plain mode emits the text content with no ANSI escapes at
all, not even Select Graphic Rendition (SGR) resets. This makes piped output safe to combine with
other tools.

`auto` is a command-line value (`cli::ColorArg`), not a `StyleMode`. `cli::parse` resolves it once
at startup. It becomes `truecolor` when stdout is a terminal and `NO_COLOR` is unset or empty, and
`plain` otherwise. An explicit encoding, from the flag or from `PEEK_COLOR`, always wins. For
example, `--color truecolor | less -R` keeps the colors.

### File info screen ✅

To reach the info screen, press Tab to cycle between content and info, or press `i` to jump to info.
The info screen is available for every file type, through `--info` or interactively through Tab and
`i`. It uses semantic coloring throughout: timestamps colored by age, sizes colored by size, and
permissions colored per character.

The screen shows these sections:

- **General**: the filename, the size in exact and human-readable form (for example
  `59,521,024 bytes (56.74 MiB)`), the MIME type, the permissions, and the timestamps.
- **Images**: the dimensions, megapixels, color mode, bit depth, ICC profile, high dynamic range
  (HDR) detection, animation stats, EXIF, and XMP.
- **Documents and text**: the counts of lines, words, and characters, the blank lines, the longest
  line, the line endings, the indent style, the encoding, and the shebang.
- **Markdown**: the heading count for each level, the count and languages of fenced code blocks,
  inline code, links, images, tables, list items, task progress, blockquote lines, footnotes, the
  frontmatter kind, the prose word count, and an estimate of the reading time.
- **SQL**: a guess at the dialect, the statement count by category (DDL, DML, DQL, and TCL), an
  inventory of created objects (tables, views, indexes, functions, and triggers), the count of
  comment lines, and a flag for PL/pgSQL blocks.
- **CSS**: the rule count, the selector count with a histogram by kind, the count of custom
  properties, the counts of `@media` and `@keyframes`, the `@import` list with external URLs
  flagged, and a grid of color-palette swatches.
- **Structured data**: the top-level kind, the count of keys or elements, the maximum nesting depth,
  the total node count, and the XML root and namespaces.
- **SVG**: the `viewBox`, the declared dimensions, the element counts (paths, groups, rectangles,
  circles, and text), flags for scripts and external `href`s, and the stats of the source text.
- **Binary**: the format detected from magic bytes, such as Mach-O, ELF, PE, ZIP, or SQLite.

#### JSON output ✅

`--info --json` emits the info screen as a single JSON object for shell pipelines, for example
`peek x --info --json | jq .size_bytes`.

- The core metadata is fully typed. `size_bytes` stays a number. Timestamps are ISO-8601 UTC
  strings, whether or not you pass `--utc`. MIME entries carry a machine `category`, such as
  `registered`, `vendor`, or `convention`.
- Optional fields that are absent, such as created, compression, and warnings, are left out instead
  of set to null.
- Each file type contributes a typed object nested under its own key, such as `pdf`, `archive`,
  `image`, or `sqlite`. The object holds raw typed values and lowercase machine tokens for enum
  fields, such as `line_endings: "crlf"` and `top_level_kind: "object"`.
- A type with no typed encoder falls back to a `details` array of text, but every shipping type
  provides an encoder.
- `--json` requires `--info`. peek rejects it otherwise.

Both outputs come from **one view model per type**. The view model is a struct that derives
`serde::Serialize` for JSON and `#[derive(InfoView)]` for the themed terminal render. Labels, skip
rules, and the formatting of each field are declared once, so print and JSON can't drift apart. A
section block on screen corresponds to a nested JSON object. The human formatting of each field,
such as sizes, gradients, and muted secondary text, lives in `InfoValue` implementations, while the
same field serializes its machine value.

#### Where the image and text stats come from

- **EXIF** shows the camera make and model, lens, orientation, resolution and DPI, exposure,
  aperture, ISO, focal length, flash, white balance, date taken, GPS, artist, and copyright.
- **ICC profile**: peek parses the profile name from the `desc` or `mluc` tag of the embedded
  profile.
- **Animation stats** show the frame count, total duration, average frames per second (FPS), and
  loop count. They come from walking the headers of GIF chunks and from parsing the WebP RIFF `ANIM`
  and `ANMF` chunks.
- **XMP metadata** is scraped from the head bytes for Dublin Core and XMP fields: title, subject,
  description, creator, rights, rating, and label.
- **Structured-data stats** come from a parse pass.
- **Text stats** come from a single streaming pass that also detects the encoding from the byte
  order mark (BOM).
- **HDR detection** scans for Ultra HDR gain map markers.

### Line numbers ✅

You can turn on line numbers in text-based views. These are the `ContentMode` views: source,
structured raw and pretty, plain text, and SVG XML. Line numbers are off by default. To turn them on
at startup, pass `--line-numbers` (`-n`). To toggle them in the viewer, press `l`. The gutter is
right-aligned, is at least 2 digits wide, and uses the gutter color of the theme. In pretty mode,
the numbers count the pretty-printed lines that you see, not the lines of the source bytes.

### Line wrapping ✅

Soft wrap is on by default for `ContentMode`: text, source, structured pretty and raw, and SVG XML.
`wrap_styled` slices each visible logical line into visual rows of width `term_cols - gutter_width`.
This way, the row budget accounts for wrapped continuation rows, and the status line never scrolls
out of view.

To toggle wrapping, press `w`. When wrap is on, vertical scrolling (`j` and `k`, PgUp and PgDn, Home
and End) moves one **visual row** at a time. As a result, a long line no longer makes a single
keypress jump over all its wrapped rows. The line-number gutter shows the real, logical line number
on the first segment. Continuation rows get a blank gutter of the same width, so wrapped content
lines up under its first row.

The status bar shows `Wrap` only when wrap is on. Wrap is on by default, so the label's absence
means that wrap is off.

### Horizontal scrolling ✅

Horizontal scrolling goes with wrap-off mode. Left and Right pan the viewport horizontally by 8
columns for each press, which feels like `less -S`. Panning works only when wrap is off. With wrap
on, Left and Right do nothing, because the content is already fully visible. The gutter doesn't pan
and stays anchored to the left edge.

### Text search ✅

#### Run a search

To search, press `/`. A search prompt opens over the status line. Type a query and press Enter to
run it.

- By default, the query matches an **exact substring**.
- To toggle **regular expression** matching, press `Ctrl-R` inside the prompt. This mode uses the
  linear-time `regex` engine, so it can't backtrack catastrophically. The prompt title shows the
  active mode: `Search (literal)` or `Search (regex)`.
- Both modes use **smart case**. A query in all lowercase matches without regard to case. Any
  uppercase character makes the whole query case-sensitive.
- If a regular expression is malformed, peek briefly shows the parse error and leaves the previous
  search untouched.
- While a regular expression search is active, the status segment puts `regex` before the count.
- peek matches a regular expression against one logical line at a time. As a result, `^` and `$`
  anchor to the line bounds, and a pattern can't span a line break.

#### Where search works

Search works in every view that renders text:

- Source, plain text, structured raw and pretty, and SVG XML, which are the `ContentMode` views.
- The rendered HTML view.
- The EPUB **Read** view.
- The DOCX, ODT, and RTF **Read** views.
- The PDF **Text** view.
- The CSV and TSV **Table** view, and the SQLite contents view that shares it. Here a match stays
  within one cell, so a query can't span a delimiter. Over a SQLite table, the scan currently covers
  only the buffered window.
- Every listing TOC. Here the query matches only the last path segment, so `sub/` finds nothing.

The shared search primitives in `viewer/search.rs` back all of these views. Each view scans its own
content through one compiled `SearchQuery`. As a result, literal and regular expression search reach
every present and future view without wiring for each type.

#### Move between matches

When you confirm the query, the viewer jumps to the first match. To cycle forward and backward
through every match, press `n` and `p`. They wrap around at the ends and scroll the line of each
match into view. In the EPUB Read view, `n` and `p` normally step through chapters. While a search
is active, they move between matches instead. To get chapter stepping back, press `Esc` to clear the
search.

A match gets an explicit pair of background **and** foreground colors. The syntax color underneath
is dropped, so matched text looks the same no matter what it was. The syntax color resumes after the
match. The colors of both states derive from the `accent` hue of the theme:

- The current `n` or `p` match is vivid (`search_current_style`).
- The other matches are muted and dark (`search_match_style`).

Each state pairs its background with a neutral foreground that contrasts with it. While a search is
active, the status line shows `cur/total`. If the query isn't found, it shows `no match`.

#### Limits and clearing

The scan is a single pass over the lines of the active view and stops at 100,000 matches.

These actions clear the search:

- Pressing Enter on an empty query.
- Pressing `Esc` while a search is active. The first press clears the matches. A second press does
  the normal back or quit action.
- A change to the set of scanned lines. This happens when you toggle raw and pretty in
  `ContentMode`, when you step to another EPUB chapter, or when the terminal resizes. On resize, the
  search clears because the read-mode views key match indices to wrapped lines.

#### Planned improvements

These improvements don't block the feature. The scope that this section describes is the supported
feature.

- Incremental search as you type.
- Search in the file-info and hex views.
- A lazy or bounded "search from here" pass for multi-GB files.

See [planned.md](planned.md#text-search).

### Help screen ✅

To open the help screen, press `h` or `?`. It shows the keyboard shortcuts and the active theme. The
shortcut list has sections. A **Global** block comes first. Then each loaded mode gets a block, with
the mode's label as the heading, for the extra keys of that mode. For example, the help for an EPUB
file shows a **Read** section for chapter navigation and a **TOC** section for pinning the parent
path. They appear under separate headings instead of in one flat list that mixes them. If an entry
of a mode duplicates a global key, peek drops the entry from the mode's section. The screen still
lists every mode of the file at once. It doesn't filter the list down to the active mode.

### About screen ✅

To open the About screen, press `a`. It shows the gradient peek logo, the version, the tagline, and
the full palette of the active theme as colored swatches. It also lists a few pointers: the
homepage, the license, and common keys. The screen doubles as a theme showcase. If you cycle themes
with `t` while About is open, you see how each theme paints the full palette.

While About is open, the logo is animated. The animation lives in `viewer/logo_anim.rs` and runs on
the standard `Mode::next_tick` and `tick` contract. The gradient slides across the wordmark in a
seamless ping-pong. Every few seconds, two short bright runners start from the left corner and trace
the outline of the wordmark in opposite directions until they meet at the far edge. The runners are
white on dark themes and black on light themes. Both painters share one glyph-walking loop,
`output::paint_logo_with`. Only the per-glyph color closure differs.

In plain mode, the animation is off. You get plain mode with `--color plain` or by cycling with `c`.
About then paints the static logo and stops ticking until color comes back. To pause or resume the
animation, press `Space` (`Action::PlayPause`, the same key that the image animation modes use).
Pausing freezes the animation on the current frame and stops the tick scheduling entirely.

### Built-in manual ✅

`peek --manual` opens the user manual from `manual/src/`. The `peek-manual` crate embeds the manual
at build time. The manual is a virtual container, `FileType::Manual`:

- The table of contents (TOC) follows `SUMMARY.md`. Part headings become group rows, and chapters
  nest under them.
- Each chapter extracts as an in-memory `.md` source and opens in the Markdown viewer.
- `--list` prints `path<TAB>title`.
- `-x <path>` works with `-p` or `-o` as it does for any container.
- `--info` shows the version, the chapter count, and the text size.

The build drops images, strips relative link targets, and applies the `<!-- peek: … -->` and
`<!-- peek-skip -->` markers. The rules for these markers are in `manual/README.md`. If the manual
is broken, the build fails.

### Extraction ✅

Extraction pulls an inner item out of a container as a standalone file. You can extract from these
sources:

- **Archive entries** (`.zip`, `.tar[.gz|.bz2|.xz|.zst|.lz4|.br]`, `.7z`, `.cpio[.gz]`, and `.ar`).
  You extract a single file by its inner path.
  - Stored zip members and uncompressed tar members are a verbatim slice of the backing source. They
    extract as a zero-copy `FileRange` view. They need no spool and no copy.
  - Other entries of 16 MiB or more spool to a `NamedTempFile` in `$TMPDIR/peek-*`. This gives
    random-access reads without holding the whole payload in RAM. Smaller entries stay in `Bytes`.
  - The temporary file is deleted automatically when the last reference to the extracted
    `InputSource` drops. This is resource acquisition is initialization (RAII) through
    `Arc<NamedTempFile>`. It also covers a `FileRange` carved from a spooled entry. That range
    carries the guard, so a recursive view keeps its backing temporary file alive.
  - The 256 MiB cap applies only on the in-memory fallback path. To force RAM-only behavior, pass
    `--no-tempfile`. This drops the cap, because you have opted in to the risk of running out of
    memory.
  - Extraction from tar, cpio, ar, and 7z streams the walk over a seekable reader. A windowed range
    adapter backs `FileRange` sources. Locating one member never reads the whole archive into RAM,
    and a compressed tar inflates only up to the matched entry. Every codec streams: `gz`, `bz2`,
    `xz`, `zst`, `lz4`, and `br`.
  - 7z also streams per entry. Solid blocks still decode up to the match, which is inherent to the
    format, but the member never buffers.
- **ISO entries** (`.iso`). You extract a single file through a zero-copy `FileRange` view over the
  backing image. There is no decompression and no buffering, so multi-GB ISO images are unaffected.
  A recursive ISO image inside a spooled archive entry also yields a guarded `FileRange` instead of
  buffering the range into RAM.
- **Animation frames** (`.gif`, `.webp`, and animated SVG). You extract a single composited frame as
  a PNG at the native pixel size of the source. An SVG smaller than 512 pixels scales up to 512 on
  its longest axis. To override the size, use `--extract-size`.
- **PDF embeds and inline images**. Embeds are `/EmbeddedFiles` attachments, extracted from a memory
  source. Inline images are image XObjects, addressed with `pages/page{N}/image{M}.{ext}`
  pseudo-paths.
- **Audio embeds**. Each visual extracts as `pictures/<usage>.<ext>`. Lyrics extract as
  `lyrics/lyrics.txt`.
- **SQLite entities**. `<kind>/<name>.sql` extracts the data definition language (DDL).
  `<kind>/<name>.csv` extracts the contents of a table or view.
- **LMDB values**. The path is `<db>/<key>[/<dup>]`. Use `@main` for the main database and `x:<hex>`
  for binary keys.
- **Spreadsheet sheets**. `<sheet>.csv` streams one worksheet to CSV. Raw ZIP paths extract the
  underlying workbook part.
- **Document embeds** (DOCX, ODT, and RTF). You extract an embedded image by its inner path.

On the command line, run `peek <file> --extract <KEY> [-o PATH]`. For containers, `<KEY>` is an
entry path. For animations, it's a 1-based frame index. `-o PATH` overrides the suggested filename.
With `-o -`, or when you pipe stdout, peek streams the raw bytes.

If you add `--print` or `--info` instead, the extracted item replaces the active source, and the
rest of the pipeline runs against it. This is recursive peek. For example,
`peek archive.zip --extract foo.py --print` syntax-highlights the inner file.

In the viewer, press `e` in a listing TOC to extract the selected file. In an animation, `e`
extracts the current frame. In both cases, a status-line prompt opens with the suggested filename
filled in. Press `Esc` to cancel or `Enter` to write. Before any TOC lookup, path safety rejects
traversal (`..`).

peek intentionally doesn't support extraction from DMG images. Universal Disk Image Format (UDIF)
block decompression is a separate effort.

## Keyboard shortcuts

These shortcuts apply to viewer mode. Keys marked _(context)_ depend on the file type.

### Navigation

| Key                                     | Action                                                              |
| --------------------------------------- | ------------------------------------------------------------------- |
| `q`                                     | Quit                                                                |
| `Esc`                                   | Pop the session stack (exit at depth 1, return to parent otherwise) |
| `Up` or `k`                             | Scroll up                                                           |
| `Down` or `j`                           | Scroll down                                                         |
| `Page Up`, `u`, `Ctrl+B`, or `Ctrl+U`   | Page up                                                             |
| `Page Down`, `d`, `Ctrl+F`, or `Ctrl+D` | Page down                                                           |
| `Home` or `g`                           | Go to top                                                           |
| `End` or `G`                            | Go to bottom                                                        |
| `Enter`                                 | Descend into selection (recursive peek)                             |
| `e`                                     | Extract selected entry or current frame                             |
| `s`                                     | Toggle sticky parent-directory breadcrumb in listing TOCs           |

### Views and modes

| Key         | Action                                      |
| ----------- | ------------------------------------------- |
| `Tab`       | Cycle the file's view modes forward         |
| `Shift+Tab` | Cycle the file's view modes backward        |
| `i`         | Jump to file info screen                    |
| `h` or `?`  | Toggle help screen                          |
| `t` or `T`  | Cycle theme forward or backward             |
| `c` or `C`  | Cycle output color mode forward or backward |
| `x`         | Toggle hex dump (no-op when hex is default) |
| `a`         | Toggle about or status screen               |

### Search _(context: text, source, and structured views)_

| Key        | Action                |
| ---------- | --------------------- |
| `/`        | Open search prompt    |
| `n`        | Next search match     |
| `p` or `N` | Previous search match |

### Text views _(context)_

| Key | Action                                            |
| --- | ------------------------------------------------- |
| `l` | Toggle line numbers                               |
| `w` | Toggle line wrapping                              |
| `r` | Toggle pretty-print vs raw (structured data only) |

### Image views _(context)_

| Key               | Action                                                                                |
| ----------------- | ------------------------------------------------------------------------------------- |
| `m` or `M`        | Cycle rendering mode forward or backward (`full`, `block`, `geo`, `ascii`, `contour`) |
| `b` or `B`        | Cycle background forward or backward (`auto`, `black`, `white`, `checkerboard`)       |
| `f`               | Cycle fit mode (`Contain`, `FitWidth`, `FitHeight`)                                   |
| `Left` or `Right` | Pan horizontally (`FitHeight`)                                                        |
| `+` or `-`        | Zoom in or out in 1.25× steps, anchored at the viewport center                        |
| `0`               | Reset zoom to 1× and pan to origin                                                    |
| `1` through `9`   | Jump to a whole-number preset zoom (1× through 9×)                                    |

### Animated image views _(context: animated GIF and WebP)_

| Key               | Action                                              |
| ----------------- | --------------------------------------------------- |
| `Space`           | Play or pause animation                             |
| `n` or `p`        | Next or previous frame                              |
| `f`               | Cycle fit mode (`Contain`, `FitWidth`, `FitHeight`) |
| `Left` or `Right` | Pan horizontally under `FitHeight`                  |
| `b`               | Cycle background                                    |
| `m`               | Cycle render mode                                   |

`Left` and `Right` are pan keys in both static and animated image views. To step through frames, use
`n` and `p`. The earlier `Left` and `Right` frame-step bindings are gone.

### Streaming table for CSV, TSV, SQLite contents, and LMDB rows _(context)_

| Key       | Action                                                    |
| --------- | --------------------------------------------------------- |
| `Shift+H` | Toggle header row on or off (CSV: override the heuristic) |
| `Shift+R` | Recompute column widths from the visible viewport         |

LMDB rows turn on the row cursor. `Up`, `Down`, and the page keys move the selected row instead of
scrolling. `Enter` opens the value of the row, and `e` saves it.

### Font specimen _(context)_

| Key        | Action                                        |
| ---------- | --------------------------------------------- |
| `n` or `p` | Step to the next or previous face in a `.ttc` |

The help screen (`h`) is the authoritative in-app reference. All bindings derive from a single
source, `crates/peek-foundation/src/viewer/ui/keys.rs::Action::bindings`.

## Color and rendering

### Theme selection ✅

To choose a theme, use `--theme` or the `PEEK_THEME` environment variable. peek embeds twelve custom
`.tmTheme` themes:

| Theme                | Based on or style                              |
| -------------------- | ---------------------------------------------- |
| `idea-dark`          | JetBrains IDEA default Dark                    |
| `idea-light`         | JetBrains IntelliJ Light                       |
| `solarized-light`    | Solarized Light                                |
| `github-light`       | GitHub Light                                   |
| `vscode-dark-modern` | VS Code Dark Modern                            |
| `vscode-dark-2026`   | VS Code Dark 2026                              |
| `vscode-monokai`     | VS Code Monokai                                |
| `iceberg`            | Iceberg (iceberg.vim), cold bluish dark        |
| `iceberg-light`      | Iceberg Light (iceberg.vim), cold bluish light |
| `graveyard`          | Gothic moonlit night                           |
| `candy-floss`        | Pastel candy on dark plum                      |
| `victorian`          | Parlor parchment with oxblood                  |

The default theme adapts to the terminal background, which peek probes with an Operating System
Command (OSC) 11 query. On a light terminal, the default is `idea-light`. On a dark terminal, or
when output is piped, the default is `idea-dark`. An explicit `--theme` or `PEEK_THEME` wins.

In the interactive viewer, `t` cycles themes live.

### Theme architecture ✅

Syntect themes provide colors for syntax highlighting scopes, such as keywords, strings, and
comments. They also provide about 30 editor UI color slots, such as foreground, background,
selection, gutter, find highlight, and accent. peek needs colored output beyond syntax highlighting.
File info screens, help text, `--help`, status indicators, line-number gutters, search highlights,
and other UI all need consistent theming.

`PeekTheme` defines semantic color roles:

| Role           | Purpose                                | Derived from (syntect)               |
| -------------- | -------------------------------------- | ------------------------------------ |
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
| `selection`    | Selected or active item                | `settings.selection`                 |

The theme system has these layers:

1. **Syntect theme**. peek loads it from custom embedded `.tmTheme` files. It provides syntax scope
   colors and editor UI slots.
2. **peek theme roles**. peek derives them automatically from the syntect theme. They're the
   semantic colors for all output that isn't syntax highlighting.
3. **Common rendering layer**. All colored text output goes through this layer. Syntax-highlighted
   code uses syntect colors, and everything else uses peek roles.
4. **Override support**. Custom peek themes could override individual roles if the derived mapping
   doesn't look right for a particular syntect theme. The format and mechanism aren't decided yet.

The rendering layer is also where color compatibility modes plug in. It can downgrade colors from
24-bit to 256 colors, 16 colors, or none.

`PeekTheme` derives the roles from the active syntect theme. All UI that isn't syntax highlighting,
such as info screens, help, and `--help`, uses these roles through `PeekTheme::paint()`. The
`.tmTheme` files are embedded at compile time with `include_str!`. The `gutter` role drives the
line-number column in `ContentMode`. The `search_match` role paints search-result backgrounds in the
text views.

### Compatibility modes ◐

Rendering has two axes:

| Axis      | Modes                                                                      | Status                                                                      |
| --------- | -------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| Color     | auto, truecolor, 256, 16, grayscale, plain                                 | ✅ (see [Color Modes](#color-modes-))                                       |
| Character | Full Unicode, ASCII-only (image rendering only, with `--image-mode ascii`) | ◐ Image side done. UI and glyph fallback for non-Unicode terminals not done |

`StyleMode` handles color. All callers paint truecolor RGB, and the active mode decides the wire
form. Image rendering goes through the same point with `StyleMode::write_fg` and `write_fg_bg`.

Character compatibility is partial. For terminals without block or quadrant glyphs,
`--image-mode ascii` falls back to a luminance density ramp. The rest of the UI, such as the status
line and the info screen, still uses Unicode box-drawing characters and dashes.

Output from libraries such as syntect needs the same treatment. Syntect hardcodes 24-bit color in
`as_24_bit_terminal_escaped`. `viewer::ranges_to_escaped_trim_newline` replaces it with a version
that goes through `StyleMode::fg_seq`, so syntax-highlighted code is downgraded along with
everything else.

## Command-line options

| Option           | Short | Description                                                                                                             | Status |
| ---------------- | ----- | ----------------------------------------------------------------------------------------------------------------------- | ------ |
| `--help`         | `-h`  | Show help screen and exit (short and long forms)                                                                        | ✅     |
| `--version`      | `-V`  | Show version info and exit                                                                                              | ✅     |
| `--manual`       |       | Open the built-in user manual                                                                                           | ✅     |
| `--print`        | `-p`  | Force print mode (direct stdout)                                                                                        | ✅     |
| `--plain`        | `-P`  | Sterile output: no highlighting, pretty-printing, or colors                                                             | ✅     |
| `--raw`          | `-r`  | Output verbatim source (no pretty-print)                                                                                | ✅     |
| `--theme`        | `-t`  | Syntax highlighting theme                                                                                               | ✅     |
| `--color`        | `-C`  | Output color encoding: auto, truecolor, 256, 16, grayscale, or plain. Auto means plain when piped.                      | ✅     |
| `--language`     | `-L`  | Force syntax language                                                                                                   | ✅     |
| `--width`        | `-w`  | Image rendering width in characters (capped at 2048, `MAX_FORCED_WIDTH_CELLS`)                                          | ✅     |
| `--image-mode`   | `-m`  | Image rendering mode                                                                                                    | ✅     |
| `--edge-density` |       | Edge density target for `--image-mode contour`                                                                          | ✅     |
| `--info`         | `-i`  | Show file info instead of contents                                                                                      | ✅     |
| `--json`         |       | Emit `--info` as machine-readable JSON (requires `--info`)                                                              | ✅     |
| `--list`         | `-l`  | Print container TOC to stdout: archives, ISO images, directories, and PDF, EPUB, DOCX, ODT, RTF, audio, or comic embeds | ✅     |
| `--utc`          |       | Show timestamps in UTC (default: local time with offset)                                                                | ✅     |
| `--background`   |       | Image transparency background: auto, black, white, or checkerboard                                                      | ✅     |
| `--margin`       |       | Image margin in transparent pixels                                                                                      | ✅     |
| `--cell-aspect`  |       | Override terminal cell aspect ratio (height ÷ width)                                                                    | ✅     |
| `--no-svg-anim`  |       | Force static render for animated SVG                                                                                    | ✅     |
| `--line-numbers` | `-n`  | Enable line numbers (toggle with `l` in the viewer)                                                                     | ✅     |
| `--extract`      | `-x`  | Extract a single inner item from a container by key                                                                     | ✅     |
| `--output`       | `-o`  | Output path for `--extract` (or `-` for stdout)                                                                         | ✅     |
| `--extract-size` |       | Output pixel size for animation or SVG frame extract                                                                    | ✅     |
| `--no-tempfile`  |       | Keep archive extracts in RAM (skip the `$TMPDIR` spool path)                                                            | ✅     |
| `--update`       |       | Check for a newer release and re-run `install.sh`                                                                       | ✅     |
| `--completions`  |       | Print the bash, zsh, fish, elvish, or PowerShell completion script, with theme, color, and language values              | ✅     |

`--plain` is the single option for sterile output. It implies `--color plain` and also turns off
syntax highlighting and structured pretty-printing. HTML and SVG drop their rendered or rasterized
view and fall back to raw source. Other rich views, such as image, PDF, DOCX, and EPUB, still
compose but render without color.

`--raw` is narrower. It skips pretty-printing of structured and SVG sources but keeps colors, font
styles, and rich renders. To get raw structure without colors while HTML and SVG still render, use
`--raw --color plain`.

`--print` (`-p`) forces print mode whether or not the output is a TTY.

### `--help` screen ✅

`-h` (short) and `--help` (long) produce two custom-themed screens instead of the default clap
output:

- **`-h` (concise)**. The gradient logo, the version and tagline, the usage line, and common
  options. It covers the 90% case without the wall of options.
- **`--help` (full)**. Everything in `-h`. It adds rarely used options and the full theme listing,
  with the active theme marked. The rarely used options are theme, color, language, width,
  image-mode, background, margin, and UTC.

Both screens share the gradient-painted logo in small-slant style:

```
                 __
   ___  ___ ___ / /__
  / _ \/ -_) -_)  '_/
 / .__/\__/\__/_/\_\
/_/
```

The active theme styles the entire output, including headings, option names, and descriptions. You
can use `--help --theme <name>` as a theme preview or showcase.

### `--version` ✅

`--version` (`-V`) prints a single line: `peek X.Y.Z`. The line is unstyled, so it suits shell
scripts such as `peek --version | awk ...`. The themed logo banner is intentionally left out. For a
styled banner with version info, use `-h`, `--help`, or the `a` view in the interactive viewer.

## Distribution ✅

GitHub Releases has release artifacts with prebuilt binaries for these platforms:

- macOS: `aarch64` and `x86_64`
- Linux: `aarch64` and `x86_64`
- Windows: `x86_64`

The POSIX `install.sh` at the repository root fetches the right archive and verifies its SHA-256
checksum. It installs to `$HOME/.local/bin`, or to `$PEEK_INSTALL_DIR` if you set it. Windows users
download the `.zip` manually.

To cut a release, dispatch `.github/workflows/release.yml`. The workflow reads the version from
`Cargo.toml`. If `vX.Y.Z` already exists on `origin`, it refuses to run. Otherwise, it creates and
pushes the tag itself.
