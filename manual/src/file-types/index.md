# File types

peek auto-detects the file type by extension and magic bytes. Where the detected type has a
dedicated viewer, that viewer runs; otherwise peek falls back to the hex dump.

| Category          | Formats                                                     |
|-------------------|-------------------------------------------------------------|
| Source code       | 100+ languages via syntect (Rust, Python, Go, TS, …)        |
| Markup            | Markdown, HTML, XML, SQL                                    |
| Structured data   | JSON / JSONC / JSON5 / JSONL, YAML, TOML, XML               |
| Documents         | DOCX, ODT, RTF                                              |
| PDF               | `.pdf` (paged render + text + embeds)                       |
| Ebooks            | EPUB                                                        |
| Images            | PNG, JPEG, GIF, WebP, BMP, TIFF, ICO, AVIF, PNM, TGA, EXR, QOI, DDS |
| Vector            | SVG (incl. CSS `@keyframes` animation)                      |
| Audio             | MP3, FLAC, Ogg, Opus, WAV, MPEG-4, AAC, AIFF, CAF, MKA, WMA |
| Archives          | ZIP, tar (+gz/bz2/xz/zst/lz4), 7-Zip, cpio                  |
| Compression       | gzip, bzip2, xz, zstd, lz4 (bare wrappers — open through)   |
| Comic archives    | CBZ                                                         |
| Disk images       | ISO 9660, DMG (UDIF trailer)                                |
| Filesystem        | Directories (one-level listing)                             |
| Binary / unknown  | Hex dump (`hexdump -C` style)                               |

Detection logic: [`src/input/detect.rs`](https://github.com/thaapasa/peek/blob/main/src/input/detect.rs).
