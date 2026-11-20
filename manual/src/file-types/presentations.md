# Presentations

PowerPoint files in Office Open XML format (`.pptx`, `.pptm`, and `.ppsx`) and OpenDocument
Presentation files (`.odp`) open as **text, one slide at a time**. Apple Keynote files (`.key`) open
as a **preview**.

## PowerPoint and OpenDocument

### Modes

To switch between modes, press `Tab`:

- **Read** is the default. It shows one slide at a time. The slide title shows as a heading, with
  the body text below it. This is the same styled prose view that documents use.
- **TOC** shows the table of contents of the file. A `.pptx` or `.odp` file is a ZIP archive, and
  this view lists its raw internal entries. You can browse and extract them like any archive.
- **Info** shows the slide, word, and image counts. If the file records them, it also shows the
  document properties: title, author, subject, keywords, dates, and the app that created the file.

### Navigating slides

To go to the next slide, press `n`. To go to the previous slide, press `p`. The status line shows
`slide X/Y`.

To search the text of the current slide, press `/`. While a search is active, `n` and `p` step
through matches instead of slides. To get slide stepping back, press `Esc` to clear the search.

With `--print`, peek writes every slide in order, with a blank line between slides.

Embedded pictures show as `[Image: name]` markers in the slide text. peek doesn't draw the pictures
inline. They're listed in the **TOC** view, where you can extract them or peek into them directly.

## Keynote

A `.key` file opens to its embedded **preview**. This is the deck thumbnail that Keynote stores for
Quick Look. peek shows it as an image with the usual zoom, pan, and fit controls. The **TOC** view
lists the contents of the package, and the **Info** view shows the Keynote build that created the
file.

Keynote stores the slide text in an undocumented internal format, so peek doesn't extract slide text
from `.key` files. The preview and the file listing take its place.

## Notes

- peek detects these files by their extension. Like every Office and OpenDocument file, they're ZIP
  archives. A deck with no extension opens as a plain archive.
- PEM private keys also use the `.key` extension. peek tells the two apart by their content: a
  Keynote package is a ZIP archive, and a key is text.
- peek doesn't support the legacy binary `.ppt` format. Use `.pptx` or `.odp` instead.
