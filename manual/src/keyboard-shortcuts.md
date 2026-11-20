# Keyboard shortcuts

All shortcuts apply to the interactive viewer. For the authoritative reference, press `h` or `?` in
the viewer to open the live help screen.

## Navigation

| Key                             | Action                                                                   |
| ------------------------------- | ------------------------------------------------------------------------ |
| `q`, `Ctrl+c`                   | Quit                                                                     |
| `Esc`                           | Go back, close the current peek, or clear the search. Quits at top level |
| `Up`, `k`                       | Scroll up                                                                |
| `Down`, `j`                     | Scroll down                                                              |
| `PgUp`, `u`, `Ctrl+B`, `Ctrl+U` | Page up                                                                  |
| `PgDn`, `d`, `Ctrl+F`, `Ctrl+D` | Page down                                                                |
| `Home`, `g`                     | Go to top                                                                |
| `End`, `G`                      | Go to bottom                                                             |
| `Left`, `Right`                 | Pan horizontally                                                         |

The `u`, `d`, and `Ctrl` paging aliases match the keys of `less` and vim. Wherever `n` and `p` step
through items, `N` is an alias for `p`. This applies to search matches, pages, chapters, and frames.

## Views and modes

| Key                | Action                                        |
| ------------------ | --------------------------------------------- |
| `Tab`, `Shift+Tab` | Cycle this file's view modes forward, reverse |
| `i`                | Jump to the file info screen                  |
| `x`                | Toggle the hex dump                           |
| `h`, `?`           | Toggle the help screen                        |
| `a`                | Toggle the about screen                       |
| `t`, `T`           | Cycle the theme forward, reverse              |
| `c`, `C`           | Cycle the output color mode forward, reverse  |

## Text views

| Key      | Action                                                     |
| -------- | ---------------------------------------------------------- |
| `l`      | Toggle line numbers                                        |
| `w`      | Toggle soft line wrap                                      |
| `r`      | Toggle pretty and raw view of structured data              |
| `/`      | Open the search prompt                                     |
| `Ctrl-R` | In the prompt, toggle literal and regular expression match |
| `n`, `p` | Next or previous search match                              |

### Search

Search matches an exact substring by default. To search with a regular expression, press `Ctrl-R` in
the prompt. The prompt title shows `Search (literal)` or `Search (regex)`.

Both kinds of search are smart-case. An all-lowercase query ignores case. A query with any uppercase
character is case-sensitive.

To run a search, type the query and press `Enter`. The viewer jumps to the first match. Press `n` or
`p` to cycle through the matches, wrapping around at the ends. The status line shows `cur/total`.
While a regular expression search is active, the status line adds the prefix `regex`.

If a regular expression has a syntax error, the viewer shows the parse error and keeps your previous
search. A regular expression matches one line at a time. This means that `^` and `$` anchor to the
start and end of a line, and a pattern can't span a line break.

To clear the matches of an active search, press `Esc`. Press `Esc` again to leave the viewer. If you
press `Enter` with an empty query, the viewer also clears the search.

On very large files, the text view and the CSV and SQLite table views scan only the first 256 MB for
each query. This keeps the viewer responsive on a multi-gigabyte file. If the scan hits this cap,
the counts show as partial, such as `12/3400+` or `no match (partial scan)`, and the viewer shows a
warning.

The same `/` search works in these views:

- The rendered HTML view
- The EPUB Read view
- The DOCX, ODT, and RTF read views
- The PDF read view

In the EPUB Read view, `n` and `p` normally step through chapters. While a search is active, they
move between matches instead. To get chapter stepping back, press `Esc` to clear the search.

## Image views

| Key      | Action                                                                                |
| -------- | ------------------------------------------------------------------------------------- |
| `m`, `M` | Cycle the render mode: `full`, `block`, `geo`, `ascii`, `contour`                     |
| `b`, `B` | Cycle the background: `auto`, `black`, `white`, `checkerboard`. `checker` is an alias |
| `f`      | Cycle the fit mode: `Contain`, `FitWidth`, `FitHeight`                                |
| `+`, `=` | Zoom in by 1.25× for each step, up to 16×                                             |
| `-`      | Zoom out by 1.25× for each step                                                       |
| `0`      | Reset the zoom to 1× and pan to the origin                                            |
| `1`..`9` | Jump to a whole-number zoom from 1× to 9×                                             |

Zoom anchors on the center of the viewport. The pixel under the center stays in place when you press
`+` or `-`. The same zoom keys work in animation, PDF, CBZ, and font specimen views. For the full
behavior, see [Zoom and pan](./viewer/zoom-pan.md).

## Animation views (GIF, WebP, animated SVG)

| Key      | Action                             |
| -------- | ---------------------------------- |
| `Space`  | Play or pause                      |
| `n`, `p` | Next or previous frame             |
| `e`      | Extract the current frame as a PNG |

## Listings (archives, PDF embeds, audio embeds, ISO, directories)

| Key          | Action                                              |
| ------------ | --------------------------------------------------- |
| `Up`, `Down` | Move the selection                                  |
| `Enter`      | Descend into the selected entry (recursive peek)    |
| `Backspace`  | Go up one directory level                           |
| `z`          | Toggle between exact bytes and human-readable sizes |
| `e`          | Extract the selected entry                          |
| `s`          | Toggle the sticky parent breadcrumb                 |
| `/`          | Search leaf names, the last path segment only       |
| `n`, `p`     | Next or previous match                              |

### Go up a level

`Backspace` walks up one directory. Its behavior depends on the listing:

- In a directory listing on disk, it opens the parent directory with the cursor on the subdirectory
  you came from. You can press it again to climb the tree, or step straight back in.
- In the table of contents (TOC) of an archive or container, such as ZIP, tar, ISO, or EPUB, it
  moves the selection to the parent directory row. You can select directory rows, so the cursor
  lands right before their contents. Each press climbs one level.

### Size column

`z` toggles the size column between exact byte counts with thousands separators and human-readable
units (KiB, MiB, GiB). While human-readable units are on, the status line shows `human sizes`. Exact
bytes are the default. The output of `--list` and `--print` always shows exact bytes.

### Listing search

Listing search matches only the last path segment of each row. For example, `sub/` finds nothing,
because no leaf name contains a slash. Search also matches directory names, so a search for the name
of a parent directory brings its subtree into view. The file selection moves only when the active
match is on a file row. This way, Extract and Descend still act on an entry you can open.

## CSV and TSV tables

| Key             | Action                                                           |
| --------------- | ---------------------------------------------------------------- |
| `Shift+H`       | Toggle the header row                                            |
| `Shift+R`       | Fit column widths to the visible rows. Only this can shrink them |
| `Left`, `Right` | Pan one column left or right                                     |
| `/`             | Search inside cells, one cell at a time                          |
| `n`, `p`        | Next or previous match                                           |

CSV search looks inside one cell at a time. A query that would span the delimiter between two
columns finds nothing. Press `n` or `p` to cycle through matches. The view pans and scrolls to bring
the cell of each match into view.

LMDB row tables add a row cursor. `Up` and `Down` move the cursor instead of scrolling. `Enter`
opens the selected value as its own file, and `e` extracts it. For details, see
[LMDB databases](file-types/lmdb.md).

## Multi-page and multi-chapter files (PDF, EPUB, CBZ)

| Key      | Action                                                      |
| -------- | ----------------------------------------------------------- |
| `n`, `p` | Next or previous page or chapter                            |
| `o`      | In the PDF read view, toggle the reconstructed text overlay |

## Font specimen (multi-face `.ttc`, `.otc`)

| Key      | Action                |
| -------- | --------------------- |
| `n`, `p` | Next or previous face |
