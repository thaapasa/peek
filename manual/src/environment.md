# Environment variables

| Variable           | Description                                                         | Default        |
| ------------------ | ------------------------------------------------------------------- | -------------- |
| `PEEK_THEME`       | Syntax highlighting theme                                           | `idea-dark`    |
| `PEEK_COLOR`       | Output color encoding                                               | `auto`         |
| `NO_COLOR`         | If not empty, `auto` color gives plain output                       | unset          |
| `PEEK_VERSION`     | Pin a specific release for `install.sh`                             | latest         |
| `PEEK_INSTALL_DIR` | Install location for `install.sh`                                   | `~/.local/bin` |
| `COLUMNS`          | Fallback terminal width for piped and `--print` output, at least 24 | 80 if unset    |
| `TMPDIR`           | Spool directory for large extracted files (`peek-*`)                | system default |

Command-line options override environment variables.
