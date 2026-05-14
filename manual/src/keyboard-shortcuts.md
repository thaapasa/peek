# Keyboard shortcuts

All shortcuts apply to the interactive viewer. Press `h` or `?` in the viewer for the live
help screen — that's the authoritative reference.

## Navigation

| Key                   | Action                |
|-----------------------|-----------------------|
| `q` / `Esc`           | Quit                  |
| `Up` / `k`            | Scroll up             |
| `Down` / `j`          | Scroll down           |
| `PgUp`                | Page up               |
| `PgDn` / `Space`      | Page down             |
| `Home` / `g`          | Go to top             |
| `End` / `G`           | Go to bottom          |
| `Left` / `Right`      | Pan horizontally / step pages (context) |

## Views and modes

| Key                 | Action                                                  |
|---------------------|---------------------------------------------------------|
| `Tab` / `Shift+Tab` | Cycle this file's view modes (forward / reverse)        |
| `i`                 | Jump to file info screen                                |
| `x`                 | Toggle hex dump                                         |
| `h` / `?`           | Toggle help screen                                      |
| `a`                 | Toggle about screen                                     |
| `t` / `T`           | Cycle theme (forward / reverse)                         |
| `c` / `C`           | Cycle output color mode (forward / reverse)             |

## Text views

| Key       | Action                                       |
|-----------|----------------------------------------------|
| `l`       | Toggle line numbers                          |
| `w`       | Toggle soft line wrap                        |
| `r`       | Toggle pretty / raw (structured data)        |
| `/`       | Open the search prompt                       |
| `n` / `p` | Next / previous search match                 |

Search is exact-substring with smart-case: an all-lowercase query matches case-insensitively;
any uppercase character makes the query case-sensitive. Type the query and press Enter to run
it — the viewer jumps to the first match. `n` / `p` cycle through matches (wrapping at the
ends); the status line shows `cur/total`. `Esc` while a search is active clears the matches
(press it again to leave the viewer); an empty-query Enter also clears the search.

## Image views

| Key       | Action                                                 |
|-----------|--------------------------------------------------------|
| `m` / `M` | Cycle render mode (full / block / geo / ascii / contour) |
| `b` / `B` | Cycle background (auto / black / white / checkerboard) |
| `f`       | Cycle fit mode (Contain / FitWidth / FitHeight)        |
| `e`       | Extract current animation frame                        |

## Animation views (GIF / WebP / animated SVG)

| Key       | Action                  |
|-----------|-------------------------|
| `Space`   | Play / pause            |
| `n` / `p` | Next / previous frame   |

## Listings (archives, PDF embeds, audio embeds, ISO, directories)

| Key       | Action                                |
|-----------|---------------------------------------|
| `Up`/`Down` | Move selection                      |
| `Enter`   | Descend into selected entry (recursive peek) |
| `e`       | Extract selected entry                |
| `s`       | Toggle sticky parent breadcrumb       |

## Multi-page / multi-chapter (PDF, EPUB, CBZ)

| Key       | Action               |
|-----------|----------------------|
| `n` / `p` | Next / previous page (or chapter) |
