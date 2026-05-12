# Archives

Container archives open in a **TOC view** — one row per entry with permissions, uncompressed
size, mtime, and path. Listing reads only the per-entry headers, so multi-GB archives open
instantly.

| Format        | Extensions                     | Spec |
|---------------|--------------------------------|------|
| ZIP           | `.zip`, `.jar`, `.war`, `.apk` | [PKWARE APPNOTE](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT) |
| Tar           | `.tar`                         | [POSIX ustar](https://pubs.opengroup.org/onlinepubs/9699919799/utilities/pax.html) |
| Tar + gzip    | `.tar.gz`, `.tgz`              |      |
| Tar + bzip2   | `.tar.bz2`, `.tbz2`            |      |
| Tar + xz      | `.tar.xz`, `.txz`              |      |
| Tar + zstd    | `.tar.zst`, `.tzst`            |      |
| Tar + lz4     | `.tar.lz4`, `.tlz4`            |      |
| 7-Zip         | `.7z`                          | [7-Zip format](https://www.7-zip.org/7z.html) |
| cpio          | `.cpio` (+ `.cpio.gz`)         | newc / ODC headers; old-binary not supported   |

## Navigation

`Up` / `Down` move a file-selection cursor (skipping directories). The selected leaf gets a
highlighted background + arrow marker. `Top` / `End` jump to first / last file; PgUp/PgDn
page-scroll then snap selection to the first visible file.

A **sticky parent breadcrumb** pins the current top row's ancestor chain to the upper viewport
rows when scrolled. Toggle with `s`.

`Enter` descends into the selected entry (recursive peek — opens it through the full peek
pipeline as if it were a standalone file). `e` extracts; see
[Extraction](../viewer/extraction.md).

## Info view

Entry count, file count, directory count, total uncompressed size. Listing failures (corrupt
archive, unsupported variant) surface as a warning row.

## Single-stream compression

Bare codec wrappers (without a tar inside) decompress transparently — peek opens straight to
the inner content rendered as whatever it actually is (source, JSON, image, …), and the Info
view adds a Compression row showing the codec plus before / after sizes.

| Format | Extension |
|--------|-----------|
| gzip   | `.gz`     |
| bzip2  | `.bz2`    |
| xz     | `.xz`     |
| zstd   | `.zst`    |
| lz4    | `.lz4`    |

Decompressed output is capped at 256 MiB. Larger streams surface a warning and the viewer falls
back to a hex view of the raw compressed bytes.
