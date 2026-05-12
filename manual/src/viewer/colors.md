# Color modes

Set with `--color` / `-C`, or `PEEK_COLOR`. Press `c` / `C` in the viewer to cycle live.

| Mode        | Encoding                                            |
|-------------|-----------------------------------------------------|
| `truecolor` | 24-bit RGB (`\x1b[38;2;r;g;bm`) — default           |
| `256`       | xterm 256-color palette (`\x1b[38;5;Nm`)            |
| `16`        | 16 base ANSI colors (`\x1b[3Nm` / `\x1b[9Nm`)       |
| `grayscale` | 24-bit luminance only — preserves shading           |
| `plain`     | No escapes — strip all color from the output        |

All callers paint truecolor RGB; the color mode owns the conversion and is the single point
where the encoding is decided. Image rendering routes through the same point, so ASCII-art
images downgrade along with everything else.

Plain mode emits text content with zero ANSI escapes (no SGR resets), so piped output is safe
to compose with other tools:

```sh
peek -C plain README.md | wc -l
peek --color plain src/main.rs > stripped.rs
```
