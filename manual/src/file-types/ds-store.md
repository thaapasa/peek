# `.DS_Store`

`.DS_Store` files are written by macOS Finder, one per folder it has displayed. They store
**view settings**, not your file contents — icon positions, window size and position, the view
style (icon / list / column), sort order, background, and modification dates. Finder leaves them
behind everywhere it browses, including network shares and the roots of archives, which is why
they so often turn up where you don't want them (and why repositories `.gitignore` them).

peek decodes the file's "Bud1" container — Apple's Buddy-allocator block store — and lists every
stored property. Detection is by magic bytes as well as the canonical `.DS_Store` name, so a
renamed or stdin-piped store is still recognised.

## Views

Tab cycles two views:

- **Records** (default) — a table with one row per stored property: the **File** the setting
  applies to (a child of the folder, or `.` for the folder itself), a friendly **Property** name,
  the raw four-character **Code**, and the decoded **Value**. The table has a pinned header,
  `Left` / `Right` column pan, and `/` search.
- **Info** — a summary: total records, how many distinct files they describe, and the folder's
  own view style and background when recorded.

## Decoded values

Some properties are decoded into readable form rather than shown as raw bytes:

- **Icon location** (`Iloc`) → `(x, y)` pixel coordinates, or `auto`.
- **Window frame** (`fwi0`) → the `(left, top) – (right, bottom)` bounds plus the view style.
- **View style** (`vstl`) → `Icon view` / `List view` / `Column view` / `Gallery view` /
  `Cover Flow`.
- **Background** (`BKGD`) → `default`, `color #RRGGBB`, or `picture`.
- **Date modified** (`modD` / `moDD`) → a UTC date.

The bulk of a modern store's settings live in embedded binary property lists (`bwsp`, `icvp`,
`lsvp`); peek reports these as `binary plist, N bytes` rather than expanding them.

## Limitations

Records are views, not extractable files, so there is no `e` extract. There is no source view —
the container is opaque binary; `x` still drops to the raw hex dump.
