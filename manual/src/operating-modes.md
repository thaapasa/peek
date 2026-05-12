# Operating modes

peek runs in one of three output modes:

| Mode      | Trigger                          | Behavior                                  |
|-----------|----------------------------------|-------------------------------------------|
| Viewer    | Stdout is a TTY (default)        | Full-screen interactive viewer            |
| Print     | `--print`/`-p` or stdout is piped| Direct stdout, no interactivity           |
| Info-only | `--info`                         | Print metadata and exit                   |
| List-only | `--list`                         | Print container TOC and exit              |

`--viewer` / `-v` forces viewer mode regardless of TTY. Binary files default to the hex-dump
viewer when interactive; piped binary streams a hex dump.

## Input

peek is a **single-file viewer**: at most one positional argument. To view several files, run
peek once per file.

| Scenario         | Stdin is TTY                     | Stdin is piped              |
|------------------|----------------------------------|-----------------------------|
| `peek` (no args) | Show short help                  | Read stdin, render          |
| `peek -`         | Read stdin (blocks until Ctrl-D) | Read stdin, render          |
| `peek file.rs`   | View file normally               | View file (stdin ignored)   |

Stdin is auto-detected by magic bytes (images, binary) and content sniffing (JSON, YAML, XML,
SVG, HTML). Plain text falls back to `--language` for syntax highlighting.

After consuming piped stdin, peek reopens the terminal so the interactive viewer's keyboard
input still works.

## Output

- **Viewer**: full-screen, alt-screen, no scrollback pollution. Quit returns the terminal
  unchanged.
- **Print**: streams to stdout. Safe to pipe into `less`, `grep`, `head`. `--plain` / `-P`
  strips ANSI escapes, pretty-printing, and rich renders.
- **`--info`**: prints the [file info screen](./viewer/info-screen.md) and exits.
- **`--list`**: prints the container TOC (archives, disk images, PDF embeds) and exits.
