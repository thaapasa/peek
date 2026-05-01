# peek

```
                 __  
   ___  ___ ___ / /__
  / _ \/ -_) -_)  '_/
 / .__/\__/\__/_/\_\ 
/_/ a file previewer
```

A modern file viewer for the terminal. Like `cat`, but it actually tries to show you what's in the file.

- **Syntax highlighting** for source code (powered by syntect/TextMate grammars)
- **Pretty-printing** for structured data: JSON, YAML, TOML, XML — with syntax highlighting
- **ASCII art rendering** for images with glyph-matched character mapping and true color
- **Hex dump** for binary files — `hexdump -C` style, terminal-width aware, streamed (no full-file load); reachable from any viewer with `x`
- **Interactive viewer** with scrolling, file info, help screen, and live theme cycling
- **Three custom dark themes** — Islands Dark, Dark 2026, Vivid Dark
- **True color support** — 24-bit color throughout

## Install

**macOS / Linux (recommended):**

```sh
curl -fsSL https://raw.githubusercontent.com/thaapasa/peek/main/install.sh | sh
```

Installs the latest release into `~/.local/bin`. Override with
`PEEK_VERSION=v0.1.0` to pin a version or `PEEK_INSTALL_DIR=/usr/local/bin`
to install elsewhere. Supports `aarch64`/`x86_64` on both platforms. Because
`curl` does not tag downloads with `com.apple.quarantine`, macOS runs the
binary directly — no Gatekeeper prompt.

**Manual download (macOS / Linux):**

Grab the `.tar.gz` for your platform from the
[Releases page](https://github.com/thaapasa/peek/releases), verify against
the `.sha256` file, extract, and move `peek` onto your `PATH`. On macOS, if
the browser quarantined the archive, clear Gatekeeper with:

```sh
xattr -d com.apple.quarantine peek
```

(or right-click → Open once in Finder).

**Windows:**

Download the `.zip` for `x86_64-pc-windows-msvc` from the
[Releases page](https://github.com/thaapasa/peek/releases), extract it, and
add the folder containing `peek.exe` to your `PATH`. Note: piping text into
`peek.exe` on Windows renders once to stdout but does not open the
interactive viewer (the Unix tty reopen trick has no Windows equivalent
here yet).

**From source (contributors):**

```sh
cargo install --path .
```

No external runtime dependencies.

## Usage

```sh
# View a file (syntax highlighted, interactive viewer)
peek src/main.rs

# View structured data (pretty-printed + highlighted)
peek config.json
peek data.yaml

# View an image (glyph-matched ASCII art)
peek photo.jpg

# View an SVG (rasterized to ASCII art, r to toggle XML source)
peek icon.svg

# Pipe output (no viewer, still highlighted)
peek data.json | less -R

# Read from stdin (auto-detects JSON/YAML/XML, or pass -l for syntax)
echo '{"a":1}' | peek
curl -s https://example.com/data.json | peek
cat src/main.rs | peek -l rust
peek -           # explicit stdin (blocks until Ctrl-D when interactive)

# Force direct output on a TTY
peek --print file.txt
peek -p file.txt

# View raw source (no pretty-printing, still highlighted)
peek --raw config.json
peek -r data.xml

# Disable syntax highlighting and pretty-printing
peek --plain file.txt
peek -P file.txt

# Choose a theme
peek --theme vscode-dark-modern src/main.rs

# Choose a color encoding (truecolor / 256 / 16 / grayscale / plain)
peek --color 256 src/main.rs
peek -C plain src/main.rs   # strip all ANSI escapes

# Image with white background (auto/black/white/checkerboard)
peek --background white logo.png

# Image with transparent margin padding
peek --margin 20 icon.svg

# Show file metadata (includes EXIF for images)
peek --info photo.jpg

# Show timestamps in UTC instead of local time + offset
peek --info --utc photo.jpg
```

## Interactive Viewer

When stdout is an interactive terminal, peek opens a full-screen viewer. When piped or
with `--print`, output goes directly to stdout.

### Keyboard Shortcuts

| Key             | Action                     |
|-----------------|----------------------------|
| `q` / `Esc`     | Quit                       |
| `Up` / `k`      | Scroll up                  |
| `Down` / `j`    | Scroll down                |
| `PgUp` / `PgDn` | Page scroll                |
| `Space`         | Page down                  |
| `Home` / `End`  | Top / bottom               |
| `Tab`           | Toggle content / file info |
| `i`             | File info                  |
| `h` / `?`       | Toggle help                |
| `t`             | Cycle theme                |
| `c`             | Cycle color mode           |
| `r`             | Toggle raw / pretty        |
| `x`             | Toggle hex dump            |
| `a`             | About / status screen      |
| `m`             | Cycle image render mode    |
| `b`             | Cycle image background     |

## Themes

Four custom embedded themes, selectable via `--theme` or `PEEK_THEME` env var:

| Theme                | Description                               |
|----------------------|-------------------------------------------|
| `idea-dark`          | JetBrains IDEA default Dark (default)     |
| `vscode-dark-modern` | VS Code Dark Modern                       |
| `vscode-dark-2026`   | VS Code Dark 2026                         |
| `vscode-monokai`     | VS Code Monokai                           |

Press `t` in the interactive viewer to cycle between themes live.

## Color Modes

The output color encoding is controlled by `--color` (`-C`) or the `PEEK_COLOR`
env var. All paint helpers route through a single `ColorMode` so callers always
hand off truecolor RGB and the mode decides the on-the-wire form.

| Mode        | Encoding                                       |
|-------------|------------------------------------------------|
| `truecolor` | 24-bit RGB (`\x1b[38;2;r;g;bm`) — default      |
| `256`       | xterm 256-color palette (`\x1b[38;5;Nm`)       |
| `16`        | 16 base ANSI colors (`\x1b[3Nm` / `\x1b[9Nm`)  |
| `grayscale` | 24-bit luminance only — preserves shading      |
| `plain`     | no escapes — strip all color from the output   |

Press `c` in the interactive viewer to cycle through them live.

## Supported File Types

### Syntax highlighting

All languages supported by the default Sublime Text / TextMate grammar set — hundreds
of languages including Rust, Python, TypeScript, Go, C/C++, Java, Ruby, Shell,
Markdown, and many more.

### Pretty-printing

| Format | Extensions                        |
|--------|-----------------------------------|
| JSON   | `.json`, `.geojson`, `.jsonl`     |
| YAML   | `.yaml`, `.yml`                   |
| TOML   | `.toml`                           |
| XML    | `.xml`, `.svg`, `.html`, `.xhtml` |

### Image rendering

All formats supported by the `image` crate: PNG, JPEG, GIF, BMP, TIFF, WebP, ICO, and
more. Rendered using glyph-matched character selection with two-color clustering and
24-bit ANSI color. Multiple rendering modes available via `--image-mode`:
`full`, `block`, `geo`, `ascii`.

## Configuration

| Variable     | Description               | Default        |
|--------------|---------------------------|----------------|
| `PEEK_THEME` | Syntax highlighting theme | `idea-dark`    |
| `PEEK_COLOR` | Output color encoding     | `truecolor`    |

## Test Files

`test-data/` and `test-images/` contain sample files for trying out peek's various
viewers — minified JSON/XML/HTML for pretty-printing, source code in several languages
for syntax highlighting, and photographs for image rendering.

## License

MIT
