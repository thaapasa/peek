# File info

Every file type has an Info view, reachable via:

- **`i`** in the viewer — jump straight there
- **`Tab`** — cycle into Info as one of the file's view modes
- **`--info`** on the CLI — print info and exit

## Universal fields

- **File** — name, path, size (exact + human-readable)
- **MIME** — detected via magic bytes
- **Permissions** — per-character coloring
- **Timestamps** — created / modified / accessed, age-based coloring

## Format-specific sections

| File kind        | Info section adds                                                                                  |
|------------------|----------------------------------------------------------------------------------------------------|
| Text / source    | Line / word / char counts, blank lines, longest line, line endings, indent style, encoding, shebang |
| Markdown         | Heading counts, fenced code + langs, links, images, tables, list items, task progress, reading time |
| SQL              | Dialect, statement counts by category, created-object inventory, comment count, PL/pgSQL flag       |
| Structured data  | Top-level kind, key/element count, max nesting depth, total node count                              |
| XML / SVG        | Root element, namespaces, element counts                                                            |
| Image            | Dimensions, megapixels, color mode, bit depth, ICC profile, HDR, EXIF, XMP                          |
| Animation        | Frame count, total duration, average FPS, loop count                                                |
| Audio            | Container, codec, channels, sample rate, bit depth, bitrate, duration; tag fields                   |
| Document         | Title, author, subject, keywords, dates, paragraph / word / image counts                            |
| PDF              | PDF version, metadata, page count, attachment count                                                 |
| EPUB             | Dublin Core metadata, spine length                                                                  |
| Archive          | Entry / file / directory counts, total uncompressed size                                            |
| ISO              | Volume label, system ID, publisher, application, four PVD timestamps                                |
| DMG              | UDIF version, image variant, sizes, partition-map presence, trailer flags                           |
| Compressed wrap  | Codec + size before / after                                                                         |
| Binary           | Detected format from magic (Mach-O, ELF, PE, SQLite, …)                                             |

Use `--utc` to show timestamps in UTC instead of local + offset.
