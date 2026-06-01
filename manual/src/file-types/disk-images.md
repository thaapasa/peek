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
bytes. Real filesystems (`Apple_HFS`, `Apple_APFS`, …) each get a detail block; the format
scaffolding (protective MBR, primary/backup GPT header + table, free-space gaps) collapses into
one `Partition scheme` block, one line each. Nothing is hidden — the split just puts the real
content first. Each row carries its image offset (the byte position `mount -o offset=`, `dd
skip=`, or `mmls` would select on). Example:

```
Partitions    8 (1 filesystem, 7 scheme)

── Partition · HFS+ ───────────────────
  Name          disk image (Apple_HFS : 4)
  Type          HFS+ (Apple_HFS)
  Logical size  596.33 MiB
  Stored        159.64 MiB (27% of logical)
  Compression   zlib · 3.7×
  Chunks        408 (1 raw, 4 ignore, 403 zlib)
  Image offset  20,480 B (sector 40)

── Partition scheme ────────────────────
  MBR                512 B · zlib · @ 0 B
  Primary GPT Header 512 B · zlib · @ 512 B
  free space         3.00 KiB · sparse · @ 17,408 B
  ...
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
