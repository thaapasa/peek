# CLI options

| Option           | Short | Description                                                   |
|------------------|-------|---------------------------------------------------------------|
| `--help`         | `-h`  | Show help (short form; `--help` prints the long form)         |
| `--version`      | `-V`  | Show version info and exit                                    |
| `--print`        | `-p`  | Force print mode (direct stdout)                              |
| `--viewer`       | `-v`  | Force viewer mode                                             |
| `--plain`        | `-P`  | Sterile output: no highlighting, pretty-printing, or colors   |
| `--raw`          | `-r`  | Output verbatim source (no pretty-print)                      |
| `--theme`        | `-t`  | Syntax highlighting theme — see [Themes](./viewer/themes.md)  |
| `--color`        | `-C`  | Output color encoding — see [Color modes](./viewer/colors.md) |
| `--language`     | `-L`  | Force syntax language                                         |
| `--width`        | `-w`  | Image rendering width in characters                           |
| `--image-mode`   | `-m`  | Image render mode (full / block / geo / ascii / contour)      |
| `--background`   |       | Image transparency background (auto / none / black / white / checkerboard) |
| `--margin`       |       | Image margin in transparent pixels                            |
| `--edge-density` |       | Tune contour line count (image-mode contour)                  |
| `--no-svg-anim` |       | Force static render for animated SVG                          |
| `--info`         | `-i`  | Print file info and exit                                      |
| `--list`         | `-l`  | Print container TOC to stdout (archives / disks / PDF embeds) |
| `--utc`          |       | Show timestamps in UTC (default: local + offset)              |
| `--line-numbers` | `-n`  | Enable line numbers (toggle with `l` in the viewer)           |
| `--extract`      |       | Extract a single inner item — see [Extraction](./viewer/extraction.md) |
| `-o` / `--output`|       | Output path for `--extract` (or `-` for stdout)               |
| `--extract-size` |       | Output size for animation frame extract                       |
| `--no-tempfile`  |       | Keep archive extracts in RAM (skip the `$TMPDIR` spool path)  |
| `--update`       |       | Check for newer release and re-run `install.sh`               |

## Notes

- `--plain` is the single "sterile output" knob: implies `--color plain` and additionally
  disables syntax highlighting, structured pretty-printing, and rich renders (HTML / EPUB /
  DOCX / image / PDF fall back to raw text or hex). Use it when piping into tools that
  expect bytes-as-typed.
- `--raw` is narrower: it skips pretty-printing of structured / SVG sources but keeps colors,
  font styles, and rich renders. Pair `--raw --color plain` if you want raw structure
  without colors but still want HTML / DOCX rendered.
- `--print` / `-p` forces print mode regardless of TTY.
- `--help --theme <name>` doubles as a theme preview — the help screen is themed.

## Help screens

- **`-h`** (concise) — gradient logo, version + tagline, usage line, common options.
- **`--help`** (full) — everything in `-h`, plus rarely-used options (theme, color, language,
  width, image-mode, background, margin, utc) and the full theme listing with the active
  marker.

Both are custom-themed — not the default clap output.
