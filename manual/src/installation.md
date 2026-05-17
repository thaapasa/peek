# Installation

## macOS / Linux (recommended)

```sh
curl -fsSL https://raw.githubusercontent.com/thaapasa/peek/main/install.sh | sh
```

Installs the latest release into `~/.local/bin`. Supported targets: `aarch64` and `x86_64` on
both platforms.

Overrides:

| Variable            | Effect                                |
|---------------------|---------------------------------------|
| `PEEK_VERSION`      | Pin a release tag, e.g. `v0.1.10`     |
| `PEEK_INSTALL_DIR`  | Install to a custom directory         |

`curl` doesn't tag downloads with `com.apple.quarantine`, so macOS runs the binary directly — no
Gatekeeper prompt.

## Manual download

Grab the `.tar.gz` for your platform from the
[Releases page](https://github.com/thaapasa/peek/releases), verify against the `.sha256`,
extract, and move `peek` onto your `PATH`. The archive ships the Pdfium shared library
(`libpdfium.dylib` on macOS, `libpdfium.so` on Linux) alongside the binary — keep them in the
same directory so PDF support loads at startup. Without the dylib next to `peek` (or available
on the system loader path), PDF rendering is disabled; all other formats still work.

On macOS, if a browser quarantined the archive:

```sh
xattr -d com.apple.quarantine peek
```

## Windows

Download the `.zip` for `x86_64-pc-windows-msvc` from the Releases page, extract, and add the
folder containing `peek.exe` to your `PATH`. Keep `pdfium.dll` (bundled in the archive) in the
same folder as `peek.exe` so PDF rendering loads at startup. Piping text into `peek.exe`
reopens the console via `CONIN$` after consuming the pipe, so the interactive viewer launches
the same as on Unix.

## From source

```sh
just pdfium      # fetch Pdfium dylib for PDF support (skip if you don't need PDF)
just install     # cargo build --release; install to $PEEK_INSTALL_DIR
```

`just pdfium` downloads [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries)
for your host platform and unpacks it into `.pdfium/`. `just install` copies both `peek` and
`libpdfium.*` into the install dir.

Pure-cargo alternative (works for everything except PDF unless the dylib is dropped next to the
binary):

```sh
cargo install --path .
```

## Updating

```sh
peek --update
```

Checks GitHub Releases and re-runs `install.sh` if a newer version is available.
