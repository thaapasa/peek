# Installation

## macOS and Linux (recommended)

```sh
curl -fsSL https://raw.githubusercontent.com/thaapasa/peek/main/install.sh | sh
```

The script installs the latest release into `~/.local/bin`. It supports `aarch64` and `x86_64` on
both platforms.

To change what the script installs, set these variables:

| Variable           | Effect                                  |
| ------------------ | --------------------------------------- |
| `PEEK_VERSION`     | Pin a release tag, for example `v0.3.2` |
| `PEEK_INSTALL_DIR` | Install to a custom directory           |

`curl` doesn't tag downloads with `com.apple.quarantine`, so macOS runs the binary directly without
a Gatekeeper prompt.

## Manual download

To install peek by hand, follow these steps:

1. Download the `.tar.gz` file for your platform from the
   [Releases page](https://github.com/thaapasa/peek/releases).
1. Verify the download against the `.sha256` file.
1. Extract the archive.
1. Move `peek` to a directory on your `PATH`.

The archive includes the Pdfium shared library next to the binary. On macOS, the library is
`libpdfium.dylib`, and on Linux, it's `libpdfium.so`. Keep the library in the same directory as
`peek` so that PDF support loads at startup. peek can also load the library from the system loader
path. If peek can't find the library, PDF rendering is turned off, but all other formats still work.

On macOS, if a browser quarantined the archive, run this command:

```sh
xattr -d com.apple.quarantine peek
```

## Windows

1. Download the `.zip` file for `x86_64-pc-windows-msvc` from the Releases page.
1. Extract the archive.
1. Add the folder that contains `peek.exe` to your `PATH`.

Keep `pdfium.dll` from the archive in the same folder as `peek.exe` so that PDF rendering loads at
startup.

You can pipe text into `peek.exe`. After peek reads the pipe, it reopens the console through
`CONIN$`, so the interactive viewer starts the same way as on Unix.

## From source

On x86_64, the build needs [nasm](https://www.nasm.us/) 2.14 or later on your `PATH`. The AVIF
decoder uses it for its SIMD code. To install nasm, run `brew install nasm`, `apt install nasm`, or
`choco install nasm`. ARM builds don't need nasm. The build tasks run with
[mise](https://mise.jdx.dev/).

```sh
mise install         # Rust toolchain + build tools
mise run pdfium      # fetch Pdfium dylib for PDF support (skip if you don't need PDF)
mise run install     # cargo build --release; install to $PEEK_INSTALL_DIR
```

`mise run pdfium` downloads Pdfium from
[`bblanchon/pdfium-binaries`](https://github.com/bblanchon/pdfium-binaries) for your host platform
and unpacks it into `.pdfium/`. `mise run install` copies both `peek` and `libpdfium.*` into the
install directory.

You can also build with Cargo alone:

```sh
cargo install --path .
```

This build supports everything except PDF, unless you place the Pdfium library next to the binary.

## Updating

To update peek, run this command:

```sh
peek --update
```

peek checks GitHub Releases. If a newer version is available, it runs `install.sh` again.

## Shell completions

peek can complete its options and their values in your shell. The values include themes, color
modes, and `--language` names. To turn on completions, add the line for your shell to its startup
file:

```sh
# bash: ~/.bashrc
command -v peek >/dev/null && eval "$(peek --completions bash)"
# zsh: ~/.zshrc
command -v peek >/dev/null && eval "$(peek --completions zsh)"
# fish: ~/.config/fish/config.fish
command -q peek; and peek --completions fish | source
```

The shell gets the script from the installed binary each time it starts, so the script stays in sync
after an update. The `command -v` check lets the shell start cleanly if you remove peek. The zsh
script runs `compinit` if it hasn't run yet. `--completions` also accepts `elvish` and `powershell`.
