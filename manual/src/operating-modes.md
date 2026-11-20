# Operating modes

peek runs in one of four output modes. A fifth option, `--extract` or `-x`, writes a single inner
item to disk or stdout.

| Mode      | Trigger                             | Behavior                                                                                                                        |
| --------- | ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| Viewer    | Stdout is a TTY (default)           | Full-screen interactive viewer                                                                                                  |
| Print     | `--print`, `-p`, or stdout is piped | Direct stdout, no interactivity                                                                                                 |
| Info-only | `--info`                            | Print metadata and exit                                                                                                         |
| List-only | `--list`                            | Print the container TOC and exit. Works for archives, ISOs, directories, and PDF, EPUB, DOCX, ODT, RTF, audio, and comic embeds |

In interactive mode, binary files open in the hex dump viewer by default. When you pipe a binary
file, peek streams a hex dump.

## Input

peek is a **single-file viewer**. It takes at most one positional argument. To view several files,
run peek once for each file.

| Scenario                 | Stdin is a TTY                     | Stdin is piped              |
| ------------------------ | ---------------------------------- | --------------------------- |
| `peek` with no arguments | Show short help                    | Read stdin, render          |
| `peek -`                 | Read stdin, waiting until `Ctrl-D` | Read stdin, render          |
| `peek file.rs`           | View the file normally             | View the file, ignore stdin |

peek detects the type of stdin automatically. It uses magic bytes for images and binary data, and
content sniffing for JSON, YAML, XML, SVG, and HTML. For plain text, peek uses `--language` to
choose the syntax highlighting.

After peek reads piped stdin, it reopens the terminal so that keyboard input in the interactive
viewer still works.

## Output

- **Viewer**: peek uses the full screen in the terminal's alternate screen, so it leaves nothing in
  the scrollback. When you quit, the terminal returns to its previous state.
- **Print**: peek streams to stdout. You can safely pipe the output into `less`, `grep`, or `head`.
  Piped output is plain text by default. To keep the colors, use `--color truecolor`. For details,
  see [Color modes](./viewer/colors.md). `--plain` or `-P` also turns off pretty-printing and rich
  renders.
- **`--info`**: peek prints the [file info screen](./viewer/info-screen.md) and exits.
- **`--list`**: peek prints the container TOC and exits. This works for archives, ISOs, directories,
  and for the page and file embed listings of PDF, EPUB, DOCX, ODT, RTF, audio, and comic files.
