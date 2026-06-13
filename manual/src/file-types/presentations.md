# Presentations

`.pptx`, `.pptm`, `.ppsx` (PowerPoint / Office Open XML) and `.odp` (OpenDocument
Presentation) open as **slide-by-slide text**. Apple Keynote (`.key`) opens as a **preview**.

## PowerPoint and OpenDocument

### Modes

Cycled with Tab:

- **Read** (default) — one slide at a time. The slide title renders as a heading and the body
  text below it, the same styled prose view documents use.
- **Files** — the deck's raw internal entries (a `.pptx` / `.odp` is a zip archive), browsable
  and extractable like any archive.
- **Info** — slide / word / image counts, plus document properties (title, author, dates,
  creating application) when the file records them.

### Navigating slides

`n` and `p` step to the next / previous slide; the status line shows `slide X/Y`. `/` searches
the current slide's text (while a search is active, `n` / `p` step matches instead — `Esc`
clears the search to get slide stepping back). `--print` writes every slide in order,
separated by a blank line.

Embedded pictures appear as `[Image: name]` markers in the slide text. The pictures themselves
aren't drawn inline, but they're listed in the **Files** view and can be extracted or peeked
into directly.

## Keynote

A `.key` file opens to its embedded **preview** — the deck thumbnail Keynote stores for
QuickLook — rendered as an image with the usual zoom / pan / fit controls. The **Files** view
lists the package contents and **Info** shows the creating Keynote build.

Keynote stores its actual slide text in an undocumented internal format, so peek doesn't
extract slide text from `.key` files — the preview and the file listing stand in.

## Notes

- Detection is by extension: like every Office / OpenDocument file these are zip archives, so a
  deck with no extension opens as a plain archive instead.
- `.key` shares its extension with PEM private keys; peek tells them apart by content (the
  Keynote package is a zip, a key is text).
- Legacy binary `.ppt` is not supported; use `.pptx` / `.odp`.
