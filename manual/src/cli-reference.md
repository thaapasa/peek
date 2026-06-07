# CLI options

| Option           | Short | Description                                                   |
|------------------|-------|---------------------------------------------------------------|
| `--help`         | `-h`  | Show help (short form; `--help` prints the long form)         |
| `--version`      | `-V`  | Show version info and exit                                    |
| `--print`        | `-p`  | Force print mode (direct stdout)                              |
| `--plain`        | `-P`  | Sterile output: no highlighting, pretty-printing, or colors   |
| `--raw`          | `-r`  | Output verbatim source (no pretty-print)                      |
| `--theme`        | `-t`  | Syntax highlighting theme — see [Themes](./viewer/themes.md)  |
| `--color`        | `-C`  | Output color encoding — see [Color modes](./viewer/colors.md) |
| `--language`     | `-L`  | Force syntax language                                         |
| `--width`        | `-w`  | Image rendering width in characters                           |
| `--image-mode`   | `-m`  | Image render mode (full / block / geo / ascii / contour)      |
| `--background`   |       | Image transparency background (auto / black / white / checkerboard) |
| `--margin`       |       | Image margin in transparent pixels                            |
| `--cell-aspect`  |       | Override terminal cell aspect ratio (height ÷ width)          |
| `--edge-density` |       | Tune contour line count (image-mode contour)                  |
| `--no-svg-anim` |       | Force static render for animated SVG                          |
| `--info`         | `-i`  | Print file info and exit                                      |
| `--json`         |       | Emit `--info` as JSON for pipelines (requires `--info`)       |
| `--list`         | `-l`  | Print container TOC to stdout (archives, ISOs, directories, PDF / EPUB / DOCX / ODT / RTF / audio / comic embeds) |
| `--utc`          |       | Show timestamps in UTC (default: local + offset)              |
| `--line-numbers` | `-n`  | Enable line numbers (toggle with `l` in the viewer)           |
| `--extract`      | `-x`  | Extract a single inner item — see [Extraction](./viewer/extraction.md) |
| `-o` / `--output`|       | Output path for `--extract` (or `-` for stdout)               |
| `--extract-size` |       | Output pixel size for animation / SVG frame extract           |
| `--no-tempfile`  |       | Keep archive extracts in RAM (skip the `$TMPDIR` spool path)  |
| `--update`       |       | Check for newer release and re-run `install.sh`               |

## Notes

- `--plain` is the single "sterile output" knob: implies `--color plain` and additionally
  disables syntax highlighting and structured pretty-printing. HTML and SVG drop their
  rendered / rasterized view and fall back to raw source; other rich views (image, PDF,
  DOCX, EPUB) still compose but render without color. Use it when piping into tools that
  expect bytes-as-typed.
- `--raw` is narrower: it skips pretty-printing of structured / SVG sources but keeps colors,
  font styles, and rich renders. Pair `--raw --color plain` if you want raw structure
  without colors but still want HTML / SVG rendered.
- `--print` / `-p` forces print mode regardless of TTY.
- `--json` (with `--info`) prints the info screen as a single JSON object for shell pipelines —
  `peek file.pdf --info --json | jq .size_bytes`. Core metadata is typed (numbers stay numbers,
  timestamps are ISO-8601 UTC); per-type stats are nested under a key named for the file type
  (`peek book.pdf --info --json | jq .pdf.page_count`).
- `--help --theme <name>` doubles as a theme preview — the help screen is themed.

## Help screens

- **`-h`** (concise) — gradient logo, version + tagline, usage line, common options.
- **`--help`** (full) — everything in `-h`, plus rarely-used options (theme, color, language,
  width, image-mode, background, margin, utc) and the full theme listing with the active
  marker.

Both are custom-themed — not the default clap output.
