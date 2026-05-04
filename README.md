# peek

```
                 __  
   ___  ___ ___ / /__
  / _ \/ -_) -_)  '_/
 / .__/\__/\__/_/\_\ 
/_/ a file previewer
```

Modern file viewer for the terminal. Like `cat`, but it actually tries to show you what's in the
file.

peek is a **single-file** viewer: it takes one path (or stdin), not a list. Run peek once per file.

- **Syntax highlighting** for source code (syntect / TextMate grammars)
- **Pretty-printing** for JSON, YAML, TOML, XML — with syntax highlighting
- **ASCII-art image rendering** with glyph-matched character mapping and 24-bit color
- **Hex dump** for binary files — `hexdump -C` style, terminal-width aware, streamed (no full-file
  load); reach from any view with `x`
- **Interactive viewer** with scrolling, file info, help screen, and live theme cycling
- **Four custom dark themes** — JetBrains IDEA Dark (default), VS Code Dark Modern, VS Code Dark
  2026, VS Code Monokai
- **True color throughout**, with graceful fallback to 256 / 16 / grayscale / plain

## Install

**macOS / Linux (recommended):**

```sh
curl -fsSL https://raw.githubusercontent.com/thaapasa/peek/main/install.sh | sh
```

Installs the latest release into `~/.local/bin`. Override with `PEEK_VERSION=v0.1.0` to pin a
version, or `PEEK_INSTALL_DIR=/usr/local/bin` to relocate. Supports `aarch64` and `x86_64` on both
platforms. `curl` doesn't tag downloads with `com.apple.quarantine`, so macOS runs the binary
directly — no Gatekeeper prompt.

**Manual download (macOS / Linux):**

Grab the `.tar.gz` for your platform from
the [Releases page](https://github.com/thaapasa/peek/releases), verify against the `.sha256`,
extract, move `peek` onto your `PATH`. On macOS, if the browser quarantined the archive:

```sh
xattr -d com.apple.quarantine peek
```

(or right-click → Open once in Finder).

**Windows:**

Download the `.zip` for `x86_64-pc-windows-msvc` from
the [Releases page](https://github.com/thaapasa/peek/releases), extract, and add the folder
containing `peek.exe` to your `PATH`. Note: piping text into `peek.exe` on Windows renders once to
stdout but does not open the interactive viewer (no Windows equivalent for the Unix tty-reopen trick
yet).

**From source:**

```sh
cargo install --path .
```

No external runtime dependencies.

## Usage

```sh
# View a file (syntax highlighted, interactive viewer)
peek src/main.rs

# Structured data (pretty-printed + highlighted)
peek config.json
peek data.yaml

# Image (glyph-matched ASCII art)
peek photo.jpg

# SVG (rasterized to ASCII art; Tab cycles to XML source / file info)
peek icon.svg

# Pipe output (no viewer, still highlighted)
peek data.json | less -R

# Read from stdin (auto-detects JSON / YAML / XML, or pass -l for syntax)
echo '{"a":1}' | peek
curl -s https://example.com/data.json | peek
cat src/main.rs | peek -l rust
peek -           # explicit stdin (blocks until Ctrl-D when interactive)

# Force direct output on a TTY
peek --print file.txt
peek -p file.txt

# Raw source (no pretty-printing, still highlighted)
peek --raw config.json
peek -r data.xml

# No syntax highlighting or pretty-printing
peek --plain file.txt
peek -P file.txt

# Theme
peek --theme vscode-dark-modern src/main.rs

# Color encoding (truecolor / 256 / 16 / grayscale / plain)
peek --color 256 src/main.rs
peek -C plain src/main.rs   # strip all ANSI escapes

# Image with white background (auto / black / white / checkerboard)
peek --background white logo.png

# Image with transparent margin padding
peek --margin 20 icon.svg

# File metadata (includes EXIF for images)
peek --info photo.jpg

# Timestamps in UTC instead of local + offset
peek --info --utc photo.jpg
```

## Interactive Viewer

When stdout is a TTY, peek opens a full-screen viewer. Piped or `--print` → output goes directly to
stdout.

### Keyboard Shortcuts

| Key              | Action                          |
|------------------|---------------------------------|
| `q` / `Esc`      | Quit                            |
| `Up` / `k`       | Scroll up                       |
| `Down` / `j`     | Scroll down                     |
| `PgUp`           | Page up                         |
| `PgDn` / `Space` | Page down                       |
| `Home` / `g`     | Top                             |
| `End` / `G`      | Bottom                          |
| `Tab`            | Cycle file's view modes         |
| `i`              | File info                       |
| `h` / `?`        | Toggle help                     |
| `t`              | Cycle theme                     |
| `c`              | Cycle color mode                |
| `r`              | Toggle raw / pretty (structured)|
| `x`              | Toggle hex dump                 |
| `a`              | About / status screen           |
| `m`              | Cycle image render mode         |
| `b`              | Cycle image background          |
| `f`              | Cycle image fit mode            |
| `Left` / `Right` | Pan horizontally (FitHeight)    |
| `p`              | Play / pause animation          |
| `n` / `N`        | Next / previous animation frame |

Source of truth: [`src/viewer/ui/keys.rs`](src/viewer/ui/keys.rs).

## Themes

Selectable via `--theme` or `PEEK_THEME`:

| Theme                | Description                           |
|----------------------|---------------------------------------|
| `idea-dark`          | JetBrains IDEA default Dark (default) |
| `vscode-dark-modern` | VS Code Dark Modern                   |
| `vscode-dark-2026`   | VS Code Dark 2026                     |
| `vscode-monokai`     | VS Code Monokai                       |

Press `t` in the interactive viewer to cycle live.

CLI names: [`src/theme/name.rs`](src/theme/name.rs). `.tmTheme` sources: [`themes/`](themes/).

## Color Modes

`--color` / `-C` or `PEEK_COLOR`. All paint helpers route through a single `ColorMode` so callers
always hand off truecolor RGB and the mode decides the on-the-wire form.

| Mode        | Encoding                                      |
|-------------|-----------------------------------------------|
| `truecolor` | 24-bit RGB (`\x1b[38;2;r;g;bm`) — default     |
| `256`       | xterm 256-color palette (`\x1b[38;5;Nm`)      |
| `16`        | 16 base ANSI colors (`\x1b[3Nm` / `\x1b[9Nm`) |
| `grayscale` | 24-bit luminance only — preserves shading     |
| `plain`     | no escapes — strip all color from the output  |

Press `c` in the interactive viewer to cycle live.

Encoding logic and CLI names: [`src/theme/color_mode.rs`](src/theme/color_mode.rs).

## Supported File Types

### Syntax highlighting

All languages supported by the default Sublime Text / TextMate grammar set — hundreds of languages
including Rust, Python, TypeScript, Go, C/C++, Java, Ruby, Shell, Markdown.

### Pretty-printing

| Format | Extensions                                  |
|--------|---------------------------------------------|
| JSON   | `.json`, `.geojson`, `.jsonl`               |
| YAML   | `.yaml`, `.yml`                             |
| TOML   | `.toml`                                     |
| XML    | `.xml`, `.svg`, `.html`, `.xhtml`, `.plist` |

Extension → format mapping: [`src/input/detect.rs`](src/input/detect.rs).

### Image rendering

All formats supported by the `image` crate: PNG, JPEG, GIF, BMP, TIFF, WebP, ICO, and more. Rendered
using glyph-matched character selection with two-color clustering and 24-bit ANSI color. Modes via
`--image-mode`: `full`, `block`, `geo`, `ascii`. Mode definitions and glyph sets: [
`src/viewer/image/mod.rs`](src/viewer/image/mod.rs).

## Configuration

| Variable     | Description               | Default     |
|--------------|---------------------------|-------------|
| `PEEK_THEME` | Syntax highlighting theme | `idea-dark` |
| `PEEK_COLOR` | Output color encoding     | `truecolor` |

## Test Files

`test-data/` and `test-images/` contain sample files for trying out peek's viewers — minified
JSON/XML/HTML for pretty-printing, source code in several languages for syntax highlighting,
photographs for image rendering.

## License

MIT
