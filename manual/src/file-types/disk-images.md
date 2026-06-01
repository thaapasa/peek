# Disk images

| Format | Extension              | Spec |
|--------|------------------------|------|
| ISO    | `.iso`                 | [ISO 9660](https://en.wikipedia.org/wiki/ISO_9660) (+ Joliet, El Torito) |
| DMG    | `.dmg`                 | [Apple Disk Image — UDIF](https://en.wikipedia.org/wiki/Apple_Disk_Image) |
| Raw    | `.img`, `.bin`, `.dd`  | MBR partition table walk; no recognised filesystem header |

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
and the documented trailer flag bits (flattened, internet-enabled).

It also decodes the **partition map** from the embedded plist — one small read, no payload
bytes. Each partition shows its Apple type (`Apple_HFS`, `Apple_APFS`, `MBR`,
`Primary GPT Header`, …), logical size, and a compression summary: codec (zlib / bzip2 /
lzfse / lzma / ADC), stored size, ratio, and chunk count. Sparse `Apple_Free` regions show as
`(sparse)`. Example:

```
Partitions    8
  MBR         512 B → 31 B (zlib, 16.5×, 1 chunk)
  Apple_APFS  10.21 MiB → 201.90 KiB (zlib, 51.8×, 3 chunks)
  Apple_Free  3.00 KiB (sparse, 1 chunk)
```

Walking each partition's inner filesystem (HFS+ / APFS) is a separate, deferred effort — the
compression runs are read for their structure, not decompressed. DMG extract is likewise
unsupported.

## Raw

Generic raw disk images (`.img` / `.bin` / `.dd`) that don't match a recognised filesystem
header. The Info view parses the MBR partition table when one is present (partition type,
boot flag, LBA offset, sector count) and otherwise falls back to a `raw image` label.
Listing isn't supported — opening the inner filesystem would need a per-FS walker. Hex view
(`x`) works on the raw bytes.
