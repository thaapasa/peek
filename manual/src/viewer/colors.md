# Color modes

Set with `--color` / `-C`, or `PEEK_COLOR`. Press `c` / `C` in the viewer to cycle live.

| Mode        | Encoding                                               |
| ----------- | ------------------------------------------------------ |
| `auto`      | `truecolor` on a terminal, `plain` otherwise — default |
| `truecolor` | 24-bit RGB (`\x1b[38;2;r;g;bm`)                        |
| `256`       | xterm 256-color palette (`\x1b[38;5;Nm`)               |
| `16`        | 16 base ANSI colors (`\x1b[3Nm` / `\x1b[9Nm`)          |
| `grayscale` | 24-bit luminance only — preserves shading              |
| `plain`     | No escapes — strip all color from the output           |

All callers paint truecolor RGB; the color mode owns the conversion and is the single point where
the encoding is decided. Image rendering routes through the same point, so ASCII-art images
downgrade along with everything else.

Plain mode emits text content with zero ANSI escapes (no SGR resets). With the `auto` default, peek
picks it whenever stdout is a pipe or a file, so piped output composes with other tools as is:

```sh
peek README.md | wc -l
peek src/main.rs > stripped.rs
```

`auto` also gives plain output when the `NO_COLOR` environment variable is set (see
[no-color.org](https://no-color.org)). To keep colors in a pipe, name an encoding; it always wins
over `auto` and `NO_COLOR`:

```sh
peek app.log --color truecolor | less -R
peek app.log -C truecolor | grep ERROR       # matching lines, still highlighted
```

Set `PEEK_COLOR=truecolor` in the shell profile to make that the default everywhere.
