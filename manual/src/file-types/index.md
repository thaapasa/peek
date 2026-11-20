# File types

peek detects the file type automatically from the extension and the magic bytes. If the detected
type has a dedicated viewer, peek opens it. Otherwise, peek falls back to the hex dump.

| Category               | Formats                                                                                              |
| ---------------------- | ---------------------------------------------------------------------------------------------------- |
| Source code            | More than 100 languages through syntect, such as Rust, Python, Go, and TypeScript                    |
| Markup                 | Markdown, HTML, XML, SQL                                                                             |
| Email                  | `.eml` messages, `.mbox` mailboxes                                                                   |
| Calendars and contacts | iCalendar (`.ics`), vCard (`.vcf`)                                                                   |
| Notebooks              | Jupyter (`.ipynb`)                                                                                   |
| Structured data        | JSON, JSONC, JSON5, JSONL, YAML, TOML, XML                                                           |
| Tabular data           | CSV, TSV, shown as an aligned table                                                                  |
| Databases              | SQLite, shown as a schema listing plus a streaming row viewer                                        |
| Spreadsheets           | `.xlsx`, `.xlsm`, `.ods`, shown as a sheet listing plus a table for each sheet                       |
| Presentations          | `.pptx`, `.pptm`, `.ppsx`, `.odp` with slide text. `.key` with a preview                             |
| Documents              | DOCX, ODT, RTF                                                                                       |
| PDF and Illustrator    | `.pdf`, `.ai`, with paged render, text, and embeds                                                   |
| EPS and PostScript     | `.eps`, `.ps`, with embedded preview, Ghostscript render, and DSC info                               |
| Ebooks                 | EPUB                                                                                                 |
| Images                 | PNG, JPEG, GIF, WebP, BMP, TIFF, ICO, AVIF, PNM, TGA, EXR, QOI, DDS                                  |
| Vector                 | SVG, including CSS `@keyframes` animation                                                            |
| Audio                  | MP3, FLAC, Ogg, Opus, WAV, MPEG-4, AAC, AIFF, CAF, MKA, WMA                                          |
| Archives               | ZIP, tar (also with `.gz`, `.bz2`, `.xz`, `.zst`, `.lz4`, `.br`), 7-Zip, cpio, ar (including `.deb`) |
| Compression            | Bare gzip, bzip2, xz, zstd, lz4, and brotli wrappers                                                 |
| Comic archives         | CBZ                                                                                                  |
| Disk images            | ISO 9660, DMG (UDIF trailer)                                                                         |
| Filesystem             | Directories, as a one-level listing                                                                  |
| Executables            | ELF, Mach-O, PE and COFF, and WebAssembly (`.wasm`) object files                                     |
| Java classfiles        | `.class` files, with header, fields, and methods                                                     |
| Certificates           | PEM X.509 certificates, CSRs, CRLs, keys, and OpenSSH public keys                                    |
| Fonts                  | TrueType, OpenType, WOFF, WOFF2, and TTC or OTC collections. Shows metadata plus a specimen render   |
| `.DS_Store`            | Apple Finder store, as a table of icon, window, and view settings records                            |
| Binary or unknown      | Hex dump in the style of `hexdump -C`                                                                |

To see the detection logic, read
[`crates/peek-detect/`](https://github.com/thaapasa/peek/blob/main/crates/peek-detect/src/detect.rs).
