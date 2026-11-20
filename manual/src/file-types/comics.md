# Comic archives

| Format | Extension | Spec                                                                                     |
| ------ | --------- | ---------------------------------------------------------------------------------------- |
| CBZ    | `.cbz`    | [Comic book ZIP](https://en.wikipedia.org/wiki/Comic_book_archive), a ZIP of page images |

## Modes

A comic archive has three modes. To cycle through them, press `Tab`.

- **Read** (default): shows one page at a time through the shared image pipeline.
  - To step through pages, press `n` or `p`. The status line shows `page X/Y`.
  - To zoom and pan, use the standard keys. For details, see [Zoom and pan](../viewer/zoom-pan.md).
  - peek caches each page by the columns, rows, style, image mode, background, and fit mode. When
    you resize the terminal or cycle a render setting, peek renders only the visible page again.
- **TOC**: shows the raw ZIP file tree. To open the selected entry as a standalone file, press
  `Enter`.
- **Info**: shows the format, the page count, and the total uncompressed size of the images.

In print mode, peek writes every page in order, with blank lines between them.

peek treats an entry as a page if it has an image extension: `png`, `jpg`, `jpeg`, `webp`, `gif`,
`bmp`, `tif`, or `tiff`. peek skips `__MACOSX/` entries and sorts the pages by name.

peek doesn't support other comic archive containers, such as `.cbr`, `.cb7`, or `.cbt`.
