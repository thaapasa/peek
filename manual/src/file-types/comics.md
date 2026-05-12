# Comic archives

| Format | Extension | Spec |
|--------|-----------|------|
| CBZ    | `.cbz`    | [Comic book ZIP](https://en.wikipedia.org/wiki/Comic_book_archive) — a ZIP of page images |

## Modes

Cycled with Tab:

- **Read** (default) — one page at a time via the shared image pipeline. `n` / `p` step pages,
  the status line shows `page X/Y`. Per-page cache keyed by `(cols, rows, style, image-mode,
  background, fit)`; resizing or cycling render settings re-renders only the visible page.
- **TOC** — the raw ZIP file tree. `Enter` opens any selected entry as a standalone file.
- **Info** — format, page count, total uncompressed image bytes.

Print mode walks every page in order separated by blank lines.

Pages are detected by image extension (`png`, `jpg`, `jpeg`, `webp`, `gif`, `bmp`, `tif`,
`tiff`), `__MACOSX/` entries are skipped, and the list is sorted by name.

Other comic-archive containers (`.cbr` / `.cb7` / `.cbt`) are not supported.
