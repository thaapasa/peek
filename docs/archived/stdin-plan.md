# Plan: Stdin Input Support

> **Status: Completed 2026-04-20.** Archived for reference.

## Context

peek currently requires file path arguments and has no stdin support. The goal is to allow piping
content into peek (e.g., `curl ... | peek`, `cat file | peek`) with the full interactive viewer
experience.

## Behavior

| Scenario         | Stdin is TTY                     | Stdin is piped                    |
|------------------|----------------------------------|-----------------------------------|
| `peek` (no args) | Error: "no files specified"      | Read stdin, render                |
| `peek -`         | Read stdin (blocks until Ctrl-D) | Read stdin, render                |
| `peek file.rs`   | View file normally               | View file normally (ignore stdin) |
| `peek - file.rs` | Read stdin + view file           | Read stdin + view file            |

After reading piped stdin, reopen fd 0 from `/dev/tty` so crossterm keyboard input works normally.
This enables the full interactive viewer (theme cycling, Tab to info, scrolling) for stdin content.

## Core Abstraction: `InputSource`

Introduce an enum that decouples "where data comes from" from "how it's displayed":

```rust
// src/input.rs (new file)
pub enum InputSource {
    File(PathBuf),
    Stdin { data: Vec<u8> },
}

impl InputSource {
    pub fn read_bytes(&self) -> Result<Vec<u8>>;   // Full content as bytes
    pub fn read_text(&self) -> Result<String>;      // Full content as UTF-8
    pub fn name(&self) -> &str;                     // Display name: filename or "<stdin>"
    pub fn path(&self) -> Option<&Path>;            // Filesystem path (None for stdin)
    pub fn extension(&self) -> Option<&str>;        // File extension (None for stdin)
}
```

`File` variant reads from the filesystem on demand. `Stdin` variant returns the pre-buffered data.
This positions well for future extensions (mmap for large files, range reads for hex dump) — the
enum can grow or be replaced with a trait later.

All downstream code that currently takes `&Path` for content access switches to `&InputSource`.

## Implementation Steps

### Step 1: Add `src/input.rs`

New module with the `InputSource` enum and its methods. Add `mod input;` to `main.rs`.

### Step 2: Update `src/detect.rs`

Change `detect()` to accept `&InputSource` instead of `&Path`:

- `File` variant: existing logic (check extension, then read for magic bytes)
- `Stdin` variant: check magic bytes from buffer, try content-based format sniffing (starts with
  `{`/`[` → JSON, `---` → YAML, `<` → XML), fall back to `--language` flag or plain text

The `path.exists()` check moves inside the `File` arm.

### Step 3: Update `src/info.rs`

Change `gather()` to accept `&InputSource`:

- `File` variant: existing logic (fs::metadata, EXIF, etc.)
- `Stdin` variant: limited metadata — size from buffer length, line/word/char counts for text,
  detected type. No timestamps/permissions (display as N/A or omit).

### Step 4: Update `src/viewer/mod.rs`

Change `Viewer` trait and `Registry` methods:

- `Viewer::render(&self, source: &InputSource, file_type: &FileType, output: &mut Output)`
-
`Registry::content_renderer(source: &InputSource, file_type: &FileType) -> Result<ContentRenderer>`
- `Registry::syntax_token_for(source: &InputSource, file_type: &FileType)` — uses extension from
  source, falls back to `--language`

### Step 5: Update individual viewers

Each viewer replaces `fs::read_to_string(path)` with `source.read_text()`:

- `src/viewer/syntax.rs` — `source.read_text()?` instead of `fs::read_to_string(path)?`
- `src/viewer/structured.rs` — same change
- `src/viewer/text.rs` — same change
- `src/viewer/image/mod.rs` — `source.read_bytes()?` + `image::load_from_memory()` for stdin,
  `image::open(path)` for files. Stretch goal: initially bail on image stdin.
- `src/viewer/image/svg.rs` — similar: `source.read_bytes()?` for SVG data

### Step 6: Update `src/main.rs`

1. **Detect stdin input**: Check `args.files` for `"-"`, OR stdin is not a TTY and no files given
2. **Read stdin once**: `stdin().read_to_end(&mut buf)?` → `InputSource::Stdin { data: buf }`
3. **Reopen fd 0 from `/dev/tty`** (Unix) so crossterm works:
   ```rust
   #[cfg(unix)]
   {
       let tty = std::fs::File::open("/dev/tty")?;
       unsafe { libc::dup2(tty.as_raw_fd(), 0); }
   }
   ```
4. **Build inputs**: Map `args.files` to `Vec<(InputSource, FileType)>`, replacing `"-"` entries
   with the stdin source
5. **Rest of main dispatch is unchanged** — all code paths work with `&InputSource` now

### Step 7: Update docs

- `docs/features.md` — Update "Input" section status
- `README.md` — Add stdin usage examples
- `CLAUDE.md` — Add `input.rs` to architecture map

## Files Modified

| File                        | Change                                                 |
|-----------------------------|--------------------------------------------------------|
| `src/input.rs`              | **New** — `InputSource` enum                           |
| `src/main.rs`               | Stdin detection, reading, fd reopen, use `InputSource` |
| `src/detect.rs`             | Accept `&InputSource`, add content-based detection     |
| `src/info.rs`               | Accept `&InputSource`, handle stdin metadata           |
| `src/viewer/mod.rs`         | Trait + Registry accept `&InputSource`                 |
| `src/viewer/syntax.rs`      | Read from source instead of path                       |
| `src/viewer/structured.rs`  | Read from source instead of path                       |
| `src/viewer/text.rs`        | Read from source instead of path                       |
| `src/viewer/image/mod.rs`   | Read from source (files only initially)                |
| `src/viewer/interactive.rs` | Pass source to info::gather                            |
| `src/viewer/ui.rs`          | Display name from source                               |

## Scope Boundaries

**In scope:**

- Text/code/structured data from stdin with full interactive viewer
- Content-based format detection for stdin
- `/dev/tty` reopen for keyboard input after consuming stdin

**Stretch goals (not blocking):**

- Images from stdin (`image::load_from_memory()`)
- SVG from stdin
- Windows support (`CONIN$` instead of `/dev/tty`)

**Future (separate work):**

- Lazy/streaming reads for large files (extend `InputSource` or replace with trait)
- Hex dump viewer with range reads

## Verification

```sh
# Basic piping — full interactive viewer
echo '{"a":1}' | peek                    # JSON pretty-print, interactive
cat src/main.rs | peek -l rust           # Syntax-highlighted Rust, interactive

# Print mode
echo '{"a":1}' | peek --print           # Direct stdout
echo '{"a":1}' | peek | cat             # Piped output

# Explicit stdin
peek -                                   # Type text, Ctrl-D to view (interactive)

# TTY with no args (must NOT stall)
peek                                     # Error: "no files specified"

# Mixed
peek - src/main.rs                       # Both stdin and file

# Info
echo "hello world" | peek --info        # Limited metadata

# Theme cycling works with stdin
echo '{"a":1}' | peek                    # Press 't' to cycle themes
```
