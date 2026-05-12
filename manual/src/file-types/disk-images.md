# Disk images

| Format | Extension | Spec |
|--------|-----------|------|
| ISO    | `.iso`    | [ISO 9660](https://en.wikipedia.org/wiki/ISO_9660) (+ Joliet, El Torito) |
| DMG    | `.dmg`    | [Apple Disk Image — UDIF](https://en.wikipedia.org/wiki/Apple_Disk_Image) |

Both parsers are hand-rolled — no extra crate. Hex view (`x`) still works on the raw image
bytes.

## ISO

Opens to a TOC view: one row per file / directory with size, mtime, and 8.3 / Joliet name;
depth tracked by indented tree glyphs. The walker reads the root directory extent from the PVD
(or SVD when Joliet is present — preferred for longer Unicode names) and recurses through child
extents. Bounded depth + entry caps defend against malformed images.

Per-entry permissions are not surfaced (Rock Ridge SUSP isn't parsed); defaults are
`rwxr-xr-x` for dirs and `rw-r--r--` for files.

Entries can be extracted via `--extract <path>` or `e` in the viewer. ISO extract is
zero-copy — a `FileRange` view over the backing image, no decompression, no buffering.

The Info view surfaces volume label, volume set, system ID, publisher, data preparer,
application, volume size in blocks, and the four PVD timestamps (creation / modification /
expiration / effective). Joliet and El Torito presence are flagged.

## DMG

Opens straight to the file info screen — there's no listing path because the inner filesystem
(HFS+ / APFS / FAT) would need its own walker.

The Info view parses the 512-byte "koly" trailer at the end of the file: UDIF version, image
variant (device / partition / mounted system), total uncompressed size, data-fork length,
embedded XML partition-map size, segment number / count, data + master checksum algorithms,
and the documented trailer flag bits (flattened, internet-enabled). The XML partition map
itself isn't parsed yet; it shows up as a presence + size row.

DMG extract is intentionally unsupported — UDIF block decompression is a separate effort.
