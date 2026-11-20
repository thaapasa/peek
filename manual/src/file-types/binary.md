# Binary

Some files have no specialized viewer in peek, such as firmware or unknown formats. For these files,
peek offers the [hex dump viewer](../viewer/hex-dump.md) and a file info screen. To open the info
screen, press `i` or `Tab`.

Executables have dedicated viewers. This covers ELF, Mach-O, and PE files, and Java `.class` files.
For details, see [Object files](./object-files.md) and [Java classfiles](./classfiles.md).

When you view a binary file interactively, it opens in the hex dump. When you pipe a binary file,
peek writes a hex dump to the output. To open the hex view from any file type, press `x`. For
details, see [Hex dump](../viewer/hex-dump.md).

## File info

For a binary file without a dedicated viewer, the Info view shows the following:

- The file type and MIME type. peek detects them from the magic bytes, using the
  [infer](https://crates.io/crates/infer) crate.
- The size, both as an exact byte count and in human-readable form.
- File system metadata, such as permissions and timestamps.
- The binary format detected from the magic bytes, such as SQLite, video, or firmware.
