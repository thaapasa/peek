# EPUB

`.epub` files ([EPUB 3](https://www.w3.org/TR/epub-33/) — a ZIP container with HTML chapters +
OPF metadata) get three views, cycled with Tab:

- **Read** (default) — one chapter at a time via the shared HTML rendering pipeline (same
  `html2text` driver as the standalone [HTML viewer](./html.md)). `n` / `N` step forward /
  back through the spine; the status line shows `ch X/Y`. Each rendered chapter is cached at
  the current width so stepping back is instant; a terminal resize re-renders only the visible
  chapter. `<img>` tags with empty / missing `alt` get a fallback `image: <basename>` label.
  Cover-style chapters (almost no text + at least one image) render the first image as ASCII
  inline, so `peek book.epub` opens on the cover.
- **TOC** — the raw ZIP file tree. Useful for inspecting cover images, stylesheets, or the OPF
  / NCX metadata files. Recursive peek with `Enter` opens any selected entry.
- **Info** — Dublin Core metadata from the OPF: title, author (`dc:creator`), language,
  publisher, date, identifier, description, plus spine length.

Print mode walks every chapter in spine order separated by blank lines, so
`peek book.epub | less` renders the whole book.
