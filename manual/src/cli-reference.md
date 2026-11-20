# Command-line options

| Option           | Short | Description                                                                                                                                                 |
| ---------------- | ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `--help`         | `-h`  | Show help. `-h` prints the short form and `--help` the long form.                                                                                           |
| `--version`      | `-V`  | Show version info and exit.                                                                                                                                 |
| `--manual`       |       | Open this manual in peek. See [Help and about](./viewer/help-about.md#manual).                                                                              |
| `--print`        | `-p`  | Force print mode, which writes directly to stdout.                                                                                                          |
| `--plain`        | `-P`  | Sterile output: no highlighting, pretty-printing, or colors.                                                                                                |
| `--raw`          | `-r`  | Output the source verbatim, without pretty-printing.                                                                                                        |
| `--theme`        | `-t`  | Syntax highlighting theme. See [Themes](./viewer/themes.md).                                                                                                |
| `--color`        | `-C`  | Output color encoding: `auto`, `truecolor`, `256`, `16`, `grayscale`, or `plain`. Default `auto`. See [Color modes](./viewer/colors.md).                    |
| `--language`     | `-L`  | Force the syntax language.                                                                                                                                  |
| `--width`        | `-w`  | Image rendering width in characters. `0` fits the terminal. Capped at 2048. Default `0`.                                                                    |
| `--image-mode`   | `-m`  | Image render mode: `full`, `block`, `geo`, `ascii`, or `contour`. Default `full`.                                                                           |
| `--background`   |       | Background behind image transparency: `auto`, `black`, `white`, or `checkerboard` (alias `checker`). Default `auto`.                                        |
| `--margin`       |       | Image margin in transparent pixels. Default `0`.                                                                                                            |
| `--cell-aspect`  |       | Override the terminal cell aspect ratio, given as height ÷ width.                                                                                           |
| `--edge-density` |       | Tune the contour line count for `--image-mode contour`. Default `0.1`.                                                                                      |
| `--no-svg-anim`  |       | Force a static render for animated SVG.                                                                                                                     |
| `--info`         | `-i`  | Print file info and exit.                                                                                                                                   |
| `--json`         |       | Emit `--info` as JSON for pipelines. Requires `--info`.                                                                                                     |
| `--list`         | `-l`  | Print a container's table of contents to stdout. Works for archives, ISOs, directories, and embeds in PDF, EPUB, DOCX, ODT, RTF, and audio and comic files. |
| `--utc`          |       | Show timestamps in UTC. By default, peek shows local time with its offset.                                                                                  |
| `--line-numbers` | `-n`  | Turn on line numbers. In the viewer, press `l` to toggle them.                                                                                              |
| `--extract`      | `-x`  | Extract a single inner item. See [Extraction](./viewer/extraction.md).                                                                                      |
| `--output`       | `-o`  | Output path for `--extract`. Use `-` for stdout.                                                                                                            |
| `--extract-size` |       | Output pixel size when you extract an animation frame or an SVG frame.                                                                                      |
| `--no-tempfile`  |       | Keep archive extracts in RAM instead of spooling them to `$TMPDIR`.                                                                                         |
| `--yes`          | `-y`  | Pre-grant slow operations. Skips the load prompt for big compressed files.                                                                                  |
| `--update`       |       | Check for a newer release and re-run `install.sh`.                                                                                                          |
| `--completions`  |       | Print the shell completion script. See [Shell completions](./installation.md#shell-completions).                                                            |

## Notes

### Plain and raw output

`--plain` is the single switch for sterile output. It implies `--color plain`, and it also turns off
syntax highlighting and structured pretty-printing. HTML and SVG drop their rendered or rasterized
view and fall back to the raw source. Other rich views, such as image, PDF, DOCX, and EPUB, still
compose but render without color. Use `--plain` when you pipe into tools that expect the unchanged
bytes.

`--raw` is narrower. It skips pretty-printing of structured and SVG sources, but it keeps colors,
font styles, and rich renders. If you want the raw structure without colors but still want HTML and
SVG rendered, use `--raw --color plain`.

### Print mode

`--print` (`-p`) forces print mode, whether or not the output is a terminal.

### JSON info

`--json` together with `--info` prints the info screen as a single JSON object for shell pipelines:

```sh
peek file.pdf --info --json | jq .size_bytes
```

Core metadata is typed. Numbers stay numbers, and timestamps are ISO 8601 in UTC. Statistics for a
specific file type sit under a key named after that type:

```sh
peek book.pdf --info --json | jq .pdf.page_count
```

### Slow operations

`--yes` (`-y`) pre-grants slow operations. Without it, peek asks before it does slow work:

- A big file with transparent compression, such as `.gz`, `.xz`, or `.zst`, opens on its Info screen
  with an `Enter to …` hint instead of decompressing up front.
- A compressed tar archive, such as `.tar.gz` or `.tar.xz`, does the same when its table of contents
  needs a full decompression pass.
- When you extract or descend into a large container entry of over 256 MB, peek asks you to confirm
  with yes or no before it spools the entry.

For the first two cases, press `Enter` to run the slow work. This also unlocks the session, so later
large files in the same session don't ask again. `--yes` skips all these prompts. Every
non-interactive path, such as a pipe, `--print`, `--info`, or `--list`, is always pre-granted
because it can't answer a prompt.

Apart from the prompts, the spill path for temporary files has a hard ceiling of 8 GiB. Because of
it, a decompression bomb fails cleanly instead of filling `$TMPDIR`.

### Theme preview

`--help --theme <name>` doubles as a theme preview, because the help screen uses the theme.

## Help screens

- **`-h`** is the concise screen. It shows the gradient logo, the version and tagline, the usage
  line, and the common options.
- **`--help`** is the full screen. It shows everything in `-h`, plus the rarely used options
  (`--theme`, `--color`, `--language`, `--width`, `--image-mode`, `--background`, `--margin`, and
  `--utc`). It also lists all themes and marks the active one.

Both screens use peek's own themed layout instead of the default clap output.
