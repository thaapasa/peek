# Documents

| Format | Extensions | Format spec |
|--------|------------|-------------|
| DOCX   | `.docx`    | [Office Open XML — ECMA-376](https://ecma-international.org/publications-and-standards/standards/ecma-376/) |
| ODT    | `.odt`     | [OpenDocument](https://www.oasis-open.org/standard/opendocument/) |
| RTF    | `.rtf`     | [Rich Text Format](https://en.wikipedia.org/wiki/Rich_Text_Format) |

## Modes

**DOCX / ODT** (both ZIP-packaged) get three views, cycled with Tab:

- **Read** (default) — styled body text. Headings render bold + themed; bold / italic /
  underline / strikethrough runs render via SGR; explicit run colors apply. Bulleted lists use a
  `•` marker indented per nesting level. Embedded images surface inline as `[Image: <basename>]`
  placeholders. Tables flatten to ` | `-joined rows. Width-aware word wrap re-runs on resize.
- **TOC** — the raw ZIP file tree. Inspect inner XML parts and embedded media; `Enter` descends
  recursively. `--extract word/media/imageN.png` works as for any ZIP archive.
- **Info** — title, author, subject, keywords, created / modified timestamps, paragraph / word
  / image counts.

**RTF** is a single file (not a container), so it has only the Read and Info views.

## Caveats

- DOCX lists currently render as flat bullets — numbering cascade resolution from
  `numbering.xml` is not implemented; everything with `numPr` shows as `•`.
- ODT's `styles.xml` inheritance chain isn't consulted in v1. Real-world ODTs from LibreOffice /
  OpenOffice dump all directly-used styling into `content.xml`'s automatic-styles, which is
  what peek resolves.
