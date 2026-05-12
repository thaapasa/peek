# Binary

For files peek doesn't have a specialized viewer for — executables, fonts, unknown formats —
the baseline is the hex dump viewer plus a file info screen reachable via `i` / Tab.

## Hex dump

`hexdump -C` style: 8-digit offset, two hex columns of N/2 bytes separated by an extra space,
then a printable-ASCII column between `|`s. Bytes-per-row scales with terminal width:
`14 + 4*bpr` columns, rounded down to a multiple of 8, minimum 8. Pipe mode honors `$COLUMNS`
(≥ 24) or falls back to 16.

Reads from disk on demand — no full-file slurp, no problem with multi-GB inputs.

Reachable from any view with `x`. The viewer tracks a logical `Position` (byte offset or line
index) on switch-out and restores it on switch-in. Entering hex from a text view positions the
top at the byte offset corresponding to the current line; returning to text re-aligns the line
scroll.

Pressing `x` again returns to the previous primary mode. When hex is the default for a binary
file, no primary mode exists — `x` is a no-op there.

## File info

For binary files without a dedicated viewer, the Info view shows:

- File type / MIME (detected via magic bytes through the
  [infer](https://crates.io/crates/infer) crate)
- Size (exact bytes + human-readable)
- Filesystem metadata (permissions, timestamps)
- Detected binary format from magic (Mach-O, ELF, PE, ZIP, SQLite, …)
