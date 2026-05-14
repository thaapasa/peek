# peek

```
                 __  
   ___  ___ ___ / /__
  / _ \/ -_) -_)  '_/
 / .__/\__/\__/_/\_\ 
/_/ a file previewer
```

Modern terminal file viewer — preview any file, any format.

- **Syntax highlighting** for 100+ languages via syntect
- **Pretty-printing** for JSON / YAML / TOML / XML
- **ASCII-art image rendering** with glyph-matched 24-bit color. Animated gifs!
- **Documents** — PDF, DOCX, ODT, RTF, EPUB, CBZ
- **Containers** — ZIP / tar / 7z / cpio archives, ISO disk images, audio metadata
- **Hex dump** fallback for binary, reachable from any view with `x`
- **Interactive viewer** with live theme cycling, info screen, extraction, text search

peek is a single-file viewer: one path (or stdin) at a time. Run peek once per file.

## Install

macOS / Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/thaapasa/peek/main/install.sh | sh
```

Installs the latest release into `~/.local/bin`. Supports `aarch64` and `x86_64`. Pin a version
with `PEEK_VERSION=v0.1.0` or relocate with `PEEK_INSTALL_DIR=/usr/local/bin`.

Manual downloads, Windows, building from source, and updating: see the
[Installation chapter](https://thaapasa.github.io/peek/installation.html) of the manual.

## Usage

```sh
peek src/main.rs        # source code (syntax highlighted, interactive viewer)
peek photo.jpg          # image (glyph-matched ASCII art)
peek config.json        # structured data (pretty-printed + highlighted)
peek book.epub          # paged read with TOC + metadata views
peek archive.tar.gz     # listing view + per-entry extract
peek -                  # explicit stdin
echo '{"a":1}' | peek   # piped stdin auto-detected
```

Run `peek -h` for the short option list, `peek --help` for the full set, or read the
[manual](https://thaapasa.github.io/peek/) for per-format details, keyboard shortcuts,
extraction, themes, and the full CLI reference.

## Manual

Full user manual: **<https://thaapasa.github.io/peek/>**.

Sources in [`manual/`](manual/) — built with [mdbook](https://rust-lang.github.io/mdBook/).
Local preview:

```sh
cargo install mdbook
mdbook serve manual    # opens http://localhost:3000
```

## License

MIT
