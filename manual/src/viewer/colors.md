# Color modes

To set the color mode, pass `--color` (`-C`) or set `PEEK_COLOR`. To cycle modes live in the viewer,
press `c` or `C`.

| Mode        | Encoding                                               |
| ----------- | ------------------------------------------------------ |
| `auto`      | Default. `truecolor` on a terminal, `plain` otherwise. |
| `truecolor` | 24-bit RGB (`\x1b[38;2;r;g;bm`)                        |
| `256`       | xterm 256-color palette (`\x1b[38;5;Nm`)               |
| `16`        | 16 base ANSI colors (`\x1b[3Nm` or `\x1b[9Nm`)         |
| `grayscale` | 24-bit luminance only. Keeps the shading.              |
| `plain`     | No escape sequences. Strips all color from the output. |

Every part of peek paints in truecolor RGB. The color mode converts the colors, and it's the only
place that decides the encoding. Image rendering goes through the same place, so ASCII art images
downgrade along with everything else.

Plain mode emits the text content with zero ANSI escape sequences, including no SGR (Select Graphic
Rendition) resets. With the `auto` default, peek picks plain mode whenever stdout is a pipe or a
file. This lets piped output work with other tools as is:

```sh
peek README.md | wc -l
peek src/main.rs > stripped.rs
```

`auto` also gives plain output when the `NO_COLOR` environment variable is set. For details, see
[no-color.org](https://no-color.org). To keep colors in a pipe, name an encoding. An encoding that
you name always wins over `auto` and `NO_COLOR`:

```sh
peek app.log --color truecolor | less -R
peek app.log -C truecolor | grep ERROR       # matching lines, still highlighted
```

To make truecolor the default everywhere, set `PEEK_COLOR=truecolor` in your shell profile.
