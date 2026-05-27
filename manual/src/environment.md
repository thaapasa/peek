# Environment variables

| Variable           | Description                                        | Default      |
|--------------------|----------------------------------------------------|--------------|
| `PEEK_THEME`       | Syntax highlighting theme                          | `idea-dark`  |
| `PEEK_COLOR`       | Output color encoding                              | `truecolor`  |
| `PEEK_VERSION`     | Pin a specific release for `install.sh`            | latest       |
| `PEEK_INSTALL_DIR` | Install location for `install.sh`                  | `~/.local/bin` |
| `COLUMNS`          | Terminal width fallback for piped hex dump (≥ 24)  | 80 if unset    |
| `TMPDIR`           | Spool directory for large extract payloads (`peek-*`) | system default |

CLI flags override environment variables.
