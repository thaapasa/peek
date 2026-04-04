# peek

A modern file viewer for the terminal. Like `cat`, but it actually tries to show you what's in the file.

- **Syntax highlighting** for source code (powered by syntect/TextMate grammars)
- **Pretty-printing** for structured data: JSON, YAML, TOML, XML, and more
- **ASCII art rendering** for images with character-density mapping and true color
- **Built-in pager** — scrollable by default on interactive terminals, like `less`
- **True color support** — uses the full 24-bit color range of modern terminals

## Install

```sh
cargo install --path .
```

### Dependencies

No external runtime dependencies. Image rendering uses a built-in ASCII art renderer.

## Usage

```sh
# View a file (syntax highlighted, paged)
peek src/main.rs

# View structured data (auto-formatted)
peek config.json
peek data.yaml

# View an image (ASCII art)
peek photo.jpg

# Pipe output (pager disabled, plain text)
peek data.json | jq .

# Force plain output (no highlighting, no pager)
peek --plain file.txt

# Disable pager even on interactive terminal
peek --no-pager file.txt
```

## Behavior

When stdout is an **interactive terminal**, peek opens its built-in pager so you can scroll through the output. When stdout is **piped** or redirected, peek writes directly to stdout with no paging (but still highlights unless `--plain` is used).

## Supported file types

### Syntax highlighting

All languages supported by the default Sublime Text / TextMate grammar set — hundreds of languages including Rust, Python, TypeScript, Go, C/C++, Java, Ruby, Shell, Markdown, and many more.

### Pretty-printing

| Format | Extensions |
|--------|------------|
| JSON   | `.json`, `.geojson`, `.jsonl` |
| YAML   | `.yaml`, `.yml` |
| TOML   | `.toml` |
| XML    | `.xml`, `.svg`, `.html`, `.xhtml` |

### Image rendering

All formats supported by the `image` crate: PNG, JPEG, GIF, BMP, TIFF, WebP, ICO, and more. Rendered using character-density mapping with 24-bit ANSI color.

## Configuration

Peek respects the following environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `PEEK_THEME` | Syntax highlighting theme | `base16-ocean.dark` |
| `PEEK_PAGER` | Enable/disable built-in pager (`1`/`0`) | `1` (on TTY) |
| `PEEK_STYLE` | Output style: `full`, `plain`, `grid`, `header` | `full` |

## License

MIT
