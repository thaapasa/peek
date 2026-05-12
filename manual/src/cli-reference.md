# CLI options

| Option           | Short | Description                                                   |
|------------------|-------|---------------------------------------------------------------|
| `--help`         | `-h`  | Show help (short form; `--help` prints the long form)         |
| `--version`      | `-V`  | Show version info and exit                                    |
| `--print`        | `-p`  | Force print mode (direct stdout)                              |
| `--viewer`       | `-v`  | Force viewer mode                                             |
| `--plain`        | `-P`  | Disable syntax highlighting and pretty-printing               |
| `--raw`          | `-r`  | Output verbatim source (no pretty-print)                      |
| `--theme`        | `-t`  | Syntax highlighting theme — see [Themes](./viewer/themes.md)  |
| `--color`        | `-C`  | Output color encoding — see [Color modes](./viewer/colors.md) |
| `--language`     | `-l`  | Force syntax language                                         |
| `--width`        |       | Image rendering width in characters                           |
| `--image-mode`   |       | Image render mode (full / block / geo / ascii / contour)      |
| `--background`   |       | Image transparency background (auto / none / black / white / checkerboard) |
| `--margin`       |       | Image margin in transparent pixels                            |
| `--edge-density` |       | Tune contour line count (image-mode contour)                  |
| `--no-svg-anim` |       | Force static render for animated SVG                          |
| `--info`         |       | Print file info and exit                                      |
| `--list`         |       | Print container TOC to stdout (archives / disks / PDF embeds) |
| `--utc`          |       | Show timestamps in UTC (default: local + offset)              |
| `--line-numbers` | `-n`  | Enable line numbers (toggle with `l` in the viewer)           |
| `--extract`      |       | Extract a single inner item — see [Extraction](./viewer/extraction.md) |
| `-o` / `--output`|       | Output path for `--extract` (or `-` for stdout)               |
| `--extract-size` |       | Output size for animation frame extract                       |
| `--update`       |       | Check for newer release and re-run `install.sh`               |

## Notes

- `--plain` and `--raw` are orthogonal. `--raw` preserves original file structure (no
  pretty-printing) but still applies colors and font styles. `--plain` disables all console
  enhancements (colors, bold, italic) but doesn't change structure. Combinable: `--plain --raw`
  gives completely unmodified content with no styling.
- `--print` / `-p` forces print mode regardless of TTY.
- `--help --theme <name>` doubles as a theme preview — the help screen is themed.

## Help screens

- **`-h`** (concise) — gradient logo, version + tagline, usage line, common options.
- **`--help`** (full) — everything in `-h`, plus rarely-used options (theme, color, language,
  width, image-mode, background, margin, utc) and the full theme listing with the active
  marker.

Both are custom-themed — not the default clap output.
