# Extraction

Pull an inner item out of a container as a standalone file.

## Sources

- **Archive entries** (ZIP, tar [+ gz/bz2/xz/zst/lz4], 7-Zip, cpio) — extract a single file by
  its inner path. Always decompressed into memory with a 256 MB cap.
- **ISO entries** (`.iso`) — zero-copy via a `FileRange` view over the backing image. No
  decompression, no buffering, multi-GB ISOs unaffected.
- **PDF embedded files** (`/EmbeddedFiles` attachments) — extracted as a memory source.
- **PDF inline images** — `pages/page{N}/image{M}.{ext}` pseudo-paths for image XObjects.
- **Audio embeds** — `pictures/<usage>.<ext>` per visual, plus `lyrics/lyrics.txt`.
- **Animation frames** (`.gif`, `.webp`, animated SVG) — extract a single composited frame as
  a PNG at the source's native pixel size (sub-512px SVG scales up to 512 on the longest
  axis; override with `--extract-size`).

## CLI

```sh
peek <file> --extract <KEY> [-o PATH]
```

- `<KEY>` is an entry path for archives / ISOs / PDFs / audio, or a 1-based frame index for
  animations.
- `-o PATH` overrides the suggested filename.
- `-o -` (or piping stdout) streams raw bytes.

Adding `--print` or `--info` instead replaces the active source with the extracted item and
runs the rest of the pipeline against it — recursive peek:

```sh
peek archive.zip --extract foo.py --print     # syntax-highlight the inner file
peek photo.heic --extract thumbnail --info    # info screen on the extracted thumbnail
```

## Viewer

In a listing TOC, `e` extracts the selected file. In an animation, `e` extracts the current
frame. Either way, a status-line prompt opens prefilled with a suggested filename — `Esc`
cancels, `Enter` writes. Path safety rejects traversal (`..`) before any TOC lookup.

DMG extract is intentionally unsupported — UDIF block decompression is a separate effort.
