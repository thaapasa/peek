# PDF

`.pdf` files use [Pdfium](https://pdfium.googlesource.com/pdfium/) (Google's PDF library,
dynamically loaded from `libpdfium.*` shipped alongside peek — no system install needed).

## Modes

Cycled with Tab:

- **Read** (default) — paged image render. Each page is rasterized via Pdfium and ASCII-rendered
  through the shared image pipeline. `n` / `p` step pages; the status line shows `page X/Y`.
  Per-page cache keyed by terminal size + render settings; resize or mode cycling re-renders
  only the visible page.
- **Text** — width-wrapped text extraction across the whole document, separated by muted
  `--- Page N ---` markers.
- **Embeds** — when the PDF carries `/EmbeddedFiles` attachments, a listing of those. `Enter` /
  `e` extracts the selected attachment as a memory-backed source that re-enters peek (an
  attached CSV opens in a CSV view, an image in the image viewer, …). Hidden when the PDF has
  no attachments.
- **Info** — PDF version, title, author, subject, keywords, creation / modification dates, page
  count, attachment count.

Print mode (`--print`) walks every page in order separated by blank lines. `cat file.pdf | peek`
detects the `%PDF-` magic and routes through the PDF mode stack.

Encrypted / password-protected PDFs surface the open error in the Info section instead of
crashing.
