# Extraction

Extraction pulls an inner item out of a container and saves it as a standalone file.

## Sources

- **Archive entries**: you extract a single file by its inner path. This works for ZIP, tar (plain
  or compressed with `.gz`, `.bz2`, `.xz`, `.zst`, `.lz4`, or `.br`), 7-Zip, cpio, and ar archives.
  peek spools entries of 16 MiB or more to a temporary file in `$TMPDIR/peek-*`. This gives random
  access without holding the whole payload in RAM. Smaller entries stay in memory. peek deletes the
  temporary file automatically when you close the extracted view.
- **ISO entries** (`.iso`): peek reads the entry in place through a `FileRange` view over the image,
  with no copy. There is no decompression and no buffering, so the size of the image doesn't matter,
  even for multi-GB ISOs.
- **Attachments in `.eml` messages**: you extract a single MIME attachment by its inner path. peek
  holds it in memory, and the recursive peek pipeline detects its type again.
- **PDF embedded files**: peek extracts `/EmbeddedFiles` attachments into memory.
- **PDF inline images**: image XObjects have pseudo-paths of the form
  `pages/page{N}/image{M}.{ext}`.
- **Audio embeds**: each picture has the path `pictures/<usage>.<ext>`. Lyrics are at
  `lyrics/lyrics.txt`.
- **SQLite entities**: `<kind>/<name>.sql` holds the `CREATE …` statement of an entity.
  `<kind>/<name>.csv` holds the contents of a table or view, streamed with `SELECT *`.
- **Spreadsheet sheets**: `<sheet>.csv` streams one worksheet to CSV. Raw paths inside the ZIP
  container extract the underlying workbook part.
- **Document and presentation embeds**: in DOCX, ODT, RTF, PPTX, PPTM, PPSX, ODP, and Keynote files,
  you extract an embedded image by its inner path.
- **Directory entries**: peek resolves a listed child name against the directory path and returns
  the file itself. `..` walks up one level. peek detects the type of the result like any other file.
- **Animation frames** (`.gif`, `.webp`, animated SVG): you extract a single composited frame as a
  PNG at the native pixel size of the source. An SVG smaller than 512 pixels scales up to 512 on its
  longest axis. To set a different size, use `--extract-size`.

## Command line

```sh
peek <file> --extract <KEY> [-o PATH]
```

- For archives, ISOs, PDFs, and audio, `<KEY>` is the path of an entry. For animations, it's a frame
  index that starts at 1.
- `-o PATH` sets the output filename instead of the suggested one.
- `-o -` streams the raw bytes to stdout. Piping stdout does the same.

If you add `--print` or `--info` instead, peek replaces the active source with the extracted item
and runs the rest of the pipeline on it. This is a recursive peek:

```sh
peek archive.zip --extract foo.py --print              # syntax-highlight the inner file
peek song.mp3 --extract pictures/front_cover.jpg --info  # info screen on the embedded cover
```

## Viewer

To extract the selected file in a listing TOC, press `e`. In an animation, `e` extracts the current
frame. In both cases, a prompt opens in the status line with a suggested filename. To cancel, press
`Esc`. To write the file, press `Enter`. Before it looks up the TOC, peek rejects any path that
tries to leave the container with `..`.

peek doesn't support extraction from DMG files. Decompressing their UDIF blocks is a separate piece
of work.

## `--no-tempfile`

To make archive extraction keep payloads in RAM instead of spooling them to disk, pass
`--no-tempfile`. This skips the safety cap that normally rejects a memory-only entry larger than 256
MiB. Use it only when you'd rather risk running out of memory than write to `$TMPDIR`. For example,
you might use it on a read-only filesystem or in an unusual sandbox.
