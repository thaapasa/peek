# Keyboard shortcuts

All shortcuts apply to the interactive viewer. Press `h` or `?` in the viewer for the live
help screen — that's the authoritative reference.

## Navigation

| Key                             | Action                                                        |
|---------------------------------|---------------------------------------------------------------|
| `q`, `Ctrl+c`                   | Quit                                                          |
| `Esc`                           | Back / close current peek / clear search (quits at top level) |
| `Up`, `k`                       | Scroll up                                                     |
| `Down`, `j`                     | Scroll down                                                   |
| `PgUp`, `u`, `Ctrl+B`, `Ctrl+U` | Page up                                                       |
| `PgDn`, `d`, `Ctrl+F`, `Ctrl+D` | Page down                                                     |
| `Home`, `g`                     | Go to top                                                     |
| `End`, `G`                      | Go to bottom                                                  |
| `Left` / `Right`                | Pan horizontally                                              |

The `u` / `d` and Ctrl paging aliases follow `less` / vim muscle memory. `N` is an alias
for `p` everywhere `n` / `p` step (search matches, pages, chapters, frames).

## Views and modes

| Key                 | Action                                           |
|---------------------|--------------------------------------------------|
| `Tab` / `Shift+Tab` | Cycle this file's view modes (forward / reverse) |
| `i`                 | Jump to file info screen                         |
| `x`                 | Toggle hex dump                                  |
| `h`, `?`            | Toggle help screen                               |
| `a`                 | Toggle about screen                              |
| `t` / `T`           | Cycle theme (forward / reverse)                  |
| `c` / `C`           | Cycle output color mode (forward / reverse)      |

## Text views

| Key       | Action                                 |
|-----------|----------------------------------------|
| `l`       | Toggle line numbers                    |
| `w`       | Toggle soft line wrap                  |
| `r`       | Toggle pretty / raw (structured data)  |
| `/`       | Open the search prompt                 |
| `Ctrl-R`  | Toggle literal / regex (in the prompt) |
| `n` / `p` | Next / previous search match           |

Search defaults to exact-substring; press `Ctrl-R` in the prompt to switch to a regular
expression (the prompt title shows `Search (literal)` or `Search (regex)`). Either way matching
is smart-case: an all-lowercase query matches case-insensitively, any uppercase character makes
it case-sensitive. Type the query and press Enter to run it — the viewer jumps to the first
match. `n` / `p` cycle through matches (wrapping at the ends); the status line shows `cur/total`,
prefixed with `regex` while a regex search is active. A malformed regex shows the parse reason
and keeps your previous search. Regex matches one line at a time, so `^` and `$` anchor to line
bounds and a pattern can't span a line break. `Esc` while a search is active clears the matches
(press it again to leave the viewer); an empty-query Enter also clears the search.

On very large files the text view and the CSV / SQLite table view scan the first 256 MB per
query so the viewer never freezes for a multi-gigabyte read; a capped scan marks its counts
as partial (`12/3400+`, `no match (partial scan)`) and raises a warning.

The same `/` search works in the rendered HTML view, the EPUB, DOCX / ODT / RTF, and PDF read
views. In the EPUB Read view `n` / `p` normally step chapters — while a search is active they
navigate matches instead, and `Esc` clears the search to get chapter stepping back.

## Image views

| Key            | Action                                                                                 |
|----------------|----------------------------------------------------------------------------------------|
| `m` / `M`      | Cycle render mode (full / block / geo / ascii / contour)                               |
| `b` / `B`      | Cycle background (auto / black / white / checkerboard; `checker` aliases checkerboard) |
| `f`            | Cycle fit mode (Contain / FitWidth / FitHeight)                                        |
| `+`, `=` / `-` | Zoom in / out (1.25× per step, capped at 16×)                                          |
| `0`            | Reset zoom to 1× and pan to origin                                                     |
| `1`..`9`       | Jump to whole-number zoom (1× .. 9×)                                                   |

Zoom anchors on the viewport centre — the pixel under the centre
stays put across `+` / `-`. The same zoom keys work in animation,
PDF / CBZ, and font specimen views. See
[Zoom & pan](./viewer/zoom-pan.md) for full behaviour.

## Animation views (GIF / WebP / animated SVG)

| Key       | Action                         |
|-----------|--------------------------------|
| `Space`   | Play / pause                   |
| `n` / `p` | Next / previous frame          |
| `e`       | Extract current frame as a PNG |

## Listings (archives, PDF embeds, audio embeds, ISO, directories)

| Key           | Action                                       |
|---------------|----------------------------------------------|
| `Up` / `Down` | Move selection                               |
| `Enter`       | Descend into selected entry (recursive peek) |
| `Backspace`   | Up one directory level                       |
| `z`           | Toggle exact bytes / human-readable sizes    |
| `e`           | Extract selected entry                       |
| `s`           | Toggle sticky parent breadcrumb              |
| `/`           | Search leaf names (last path segment only)   |
| `n` / `p`     | Next / previous match                        |

`Backspace` walks up a directory. In an on-disk directory listing it opens the parent
directory with the cursor on the subdirectory you came from (so repeated presses climb the
tree and you can step straight back in). In an archive / container TOC (zip, tar, ISO, epub, …)
it moves the selection onto the parent directory row — directory rows are selectable, so the
cursor lands right above their contents and each press climbs one level.

`z` toggles the size column between exact byte counts (thousands-separated) and human-readable
units (KiB/MiB/GiB); the status line shows `human sizes` while the latter is active. Exact bytes
are the default, and `--list` / `--print` output always stays exact.

Listing search matches the last path segment of each row only — `sub/` finds nothing because
no leaf carries a slash. Directory leaves participate so a search for an ancestor name brings
that subtree into view; the file selection only moves when the active match lands on a file
row, so Extract / Descend still target a descendable entry.

## CSV / TSV table

| Key              | Action                                                 |
|------------------|--------------------------------------------------------|
| `Shift+H`        | Toggle the header row                                  |
| `Shift+R`        | Reflow column widths from the viewport (opt-in shrink) |
| `Left` / `Right` | Pan one column left / right                            |
| `/`              | Search inside cells (single-cell scope)                |
| `n` / `p`        | Next / previous match                                  |

CSV search is scoped to a single cell — a query that would span the delimiter between two
columns yields nothing. `n` / `p` cycle through matches and pan / scroll to bring each
match's cell into view.

## Multi-page / multi-chapter (PDF, EPUB, CBZ)

| Key       | Action                                            |
|-----------|---------------------------------------------------|
| `n` / `p` | Next / previous page (or chapter)                 |
| `o`       | Toggle reconstructed-text overlay (PDF read view) |

## Font specimen (multi-face `.ttc` / `.otc`)

| Key       | Action                       |
|-----------|------------------------------|
| `n` / `p` | Next / previous face         |
