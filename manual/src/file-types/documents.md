# Documents

| Format | Extensions | Format spec                                                                                                |
| ------ | ---------- | ---------------------------------------------------------------------------------------------------------- |
| DOCX   | `.docx`    | [Office Open XML, ECMA-376](https://ecma-international.org/publications-and-standards/standards/ecma-376/) |
| ODT    | `.odt`     | [OpenDocument](https://www.oasis-open.org/standard/opendocument/)                                          |
| RTF    | `.rtf`     | [Rich Text Format](https://en.wikipedia.org/wiki/Rich_Text_Format)                                         |

## Modes

DOCX and ODT files are both packaged as ZIP archives. They get three views. To switch between them,
press `Tab`:

- **Read** is the default. It shows the body text with styling:
  - Headings show in bold, in the theme's heading color.
  - Bold, italic, underlined, and strikethrough text shows with terminal text styles (SGR, Select
    Graphic Rendition).
  - Text colors that the document sets explicitly apply.
  - Bulleted lists use a `•` marker, indented by nesting level.
  - Embedded images show inline as `[Image: <basename>]` placeholders.
  - Tables flatten to rows, with `|` between the cells.
  - Word wrap fits the terminal width and wraps again when you resize the terminal.
- **TOC** shows the table of contents: the raw file tree of the ZIP archive. Use it to inspect the
  inner XML parts and the embedded media. To descend into an entry recursively, press `Enter`.
  `--extract word/media/imageN.png` works the same as for any ZIP archive.
- **Info** shows the title, author, subject, keywords, created and modified timestamps, and the
  paragraph, word, and image counts.

RTF files open with the Read and Info views. If the file embeds images as `\pict` groups, a TOC view
also appears and lists each embedded image. To extract an image, press `e` or use `--extract`.

## Limitations

- DOCX lists show as flat bullets. peek doesn't resolve list numbering from `numbering.xml`, so
  every item with `numPr` shows with a `•` marker.
- peek doesn't follow the style inheritance chain in the `styles.xml` file of an ODT document.
  LibreOffice and OpenOffice write all the styling that a document uses directly into the automatic
  styles in `content.xml`, and peek reads the styles from there.
