# `.DS_Store`

macOS Finder writes a `.DS_Store` file in each folder it displays. The file stores **view
settings**, not your file contents. These settings include icon positions, window size and position,
the view style (icon, list, or column), sort order, background, and modification dates. Finder
leaves these files everywhere it browses, including network shares and the roots of archives. This
is why they often turn up where you don't want them, and why repositories list them in `.gitignore`.

peek decodes the file's "Bud1" container, which is Apple's buddy-allocator block store, and lists
every stored property. peek detects the file by its magic bytes as well as by the canonical
`.DS_Store` name. This means that peek also recognizes a renamed store or one piped through stdin.

## Views

To switch between the two views, press Tab.

- **Records** is the default view. It's a table with one row for each stored property and these
  columns:
  - **File**: the item that the setting applies to. This is a child of the folder, or `.` for the
    folder itself.
  - **Property**: a readable name for the property.
  - **Code**: the raw four-character code.
  - **Value**: the decoded value.

  The table has a pinned header. To pan between columns, press `Left` and `Right`. To search, press
  `/`.

- **Info** is a summary. It shows the total number of records and how many distinct files they
  describe. If the store records them, it also shows the folder's own view style and background.

## Decoded values

peek decodes some properties into a readable form instead of showing raw bytes:

- **Icon location** (`Iloc`) shows as `(x, y)` pixel coordinates, or `auto`.
- **Window frame** (`fwi0`) shows as the `(left, top) – (right, bottom)` bounds plus the view style.
- **View style** (`vstl`) shows as `Icon view`, `List view`, `Column view`, `Gallery view`, or
  `Cover Flow`.
- **Background** (`BKGD`) shows as `default`, `color #RRGGBB`, or `picture`.
- **Date modified** (`modD` or `moDD`) shows as a date in UTC.

In a modern store, most settings live in embedded binary property lists (`bwsp`, `icvp`, and
`lsvp`). peek reports these as `binary plist, N bytes` instead of expanding them.

## Limitations

Records are view settings, not files, so you can't extract them with `e`. There is no source view,
because the container is opaque binary. To see the raw hex dump, press `x`.
