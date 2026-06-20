# Binary

For files peek doesn't have a specialized viewer for — firmware, unknown formats — the
baseline is the [hex dump viewer](../viewer/hex-dump.md) plus a file info screen reachable via
`i` / Tab.

(Executables — ELF, Mach-O, PE — and Java `.class` files have dedicated viewers; see
[Object files](./object-files.md) and [Java classfiles](./classfiles.md).)

Binary files default to the hex dump when interactive; piped binary streams a hex dump. The hex
view is reachable from any file type with `x` — see [Hex dump](../viewer/hex-dump.md).

## File info

For binary files without a dedicated viewer, the Info view shows:

- File type / MIME (detected via magic bytes through the
  [infer](https://crates.io/crates/infer) crate)
- Size (exact bytes + human-readable)
- Filesystem metadata (permissions, timestamps)
- Detected binary format from magic (SQLite, video, firmware, …)
