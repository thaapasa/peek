# peek

Modern terminal file viewer with syntax highlighting, structured-data pretty-printing, and image
rendering.

**Single-file viewer.** One path (or stdin) at a time. No batch mode, no file list, no `cat`-style
concatenation — those use cases belong to other tools.

## Build & Run

```sh
cargo build                  # debug build
cargo build --release        # release build
cargo run -- [args]          # run with arguments
cargo test                   # run all tests
cargo clippy                 # lint
```

No external runtime dependencies. Image rendering is built in. (PDF support uses Pdfium —
shipped alongside the binary in the release tarball, dynamically loaded at startup; no system
install needed.)

## Architecture map

Top-level only. Full file/module breakdown: [docs/architecture-map.md](docs/architecture-map.md) —
read when adding files, modifying a module, or unsure where logic lives.

```
src/
  main.rs              — CLI entry point: dispatches inputs to viewers
  cli.rs               — Args struct (clap derive)
  update.rs            — `--update` flow: GitHub Releases check + pipe install.sh into sh
  input/               — InputSource (File / Memory / FileRange / TempFile) + ByteSource +
                         LineSource (streaming, anchor-indexed); detect (magic-byte / extension /
                         sniff); mime (RFC 6838 classification); stream (ByteSource → io::Read /
                         io::BufRead); compression (gz/bz2/xz/zst/lz4); stdin reopen
  extract/             — FileType → per-type extractor dispatch; Extracted / Options / Error;
                         path sanitiser; stdout-stream or file write
  output/              — PrintOutput (write-once stdout for --print / pipes / --info);
                         CLI help and version screens
  info/                — FileInfo + FileExtras (per-type stat wrappers); gather/ (per-source
                         collection) + render/ (themed terminal section rendering); time fmt
  theme/               — PeekTheme semantic roles + paint helpers; PeekThemeName + embedded
                         .tmTheme data; StyleMode (truecolor/256/16/grayscale/plain); SGR
                         encoders + tokenizer + ActiveStyle; ThemeManager
  types/               — Per-file-type modules (each owns reader + info + view-mode):
                         binary, text, markdown, notebook (ipynb — cells rendered
                         via the markdown pipeline + JSON source), sql, sqlite
                         (read-only via bundled rusqlite — schema listing +
                         streaming row viewer), css,
                         structured (JSON/YAML/TOML/XML), csv, image (+ ASCII pipeline +
                         SVG anim), html, ebook (epub), document (docx/odt/rtf), pdf,
                         comic (cbz), svg, audio, archive (zip/tar/7z/cpio/ar), directory,
                         disk_image (iso/dmg), objfile, classfile,
                         cert (PEM X.509 / CSR / CRL / keys / SSH pubkey),
                         font (TTF/OTF/TTC — metadata + fontdue-rasterised specimen)
  viewer/              — Mode trait + ModeId + RenderCtx + ExtractTarget; compose_modes
                         dispatch table; interactive event loop; search primitives (SearchState,
                         reveal_h_scroll); wrap_scroll geometry; paged (PagedImageMode<R> +
                         image-config cycling); cell-size detection
    listing/           — ListingMode (tree TOC: perms / size / mtime / path) + shared row
                         primitives + ListingViewport (scroll / selection / sticky chain)
    modes/             — Shared modes: content (streamed text/syntax/structured/SVG),
                         pretty_view, gutter, hex, info, help, about, rendered_text<R> (generic
                         whole-document read mode for DOCX/ODT/RTF/HTML/Markdown/PDF text)
    table/             — Two aligned-table flavours under one roof: TableMode (materialised:
                         objfile / classfile) + RowsTableMode (streaming via the RowSource
                         trait: CSV + SQLite contents). Shared visual shape, separate
                         data models
    ui/                — alternate-screen / status line / term-size; ViewerState (mode stack +
                         extract dispatch + prompt slot); Prompt overlay; ScreenBuffer (diff
                         redraw); Action keybindings; help screen
themes/                — Embedded .tmTheme files (idea-dark default + vscode variants)
docs/                  — Builder / agent reference (see architecture-map.md for the index)
manual/                — User-facing manual (mdbook). `mdbook serve manual` to browse
.github/workflows/     — ci.yml (build + test on push/PR) + release.yml (5-target build matrix) +
                         manual.yml (mdbook → Pages)
install.sh             — POSIX installer for curl | sh on macOS/Linux
```

## Workflow

- **Don't commit unless asked.** The user decides what and when.
- **Don't push, open PRs, or trigger GitHub Actions on your own initiative.** Local commits only.
  The user pushes / opens PRs / merges themselves so they can amend locally first. Open a PR
  only when the user explicitly asks for one.
- **Run `cargo fmt` after editing Rust code** so formatting drift doesn't pile up across unrelated
  files. Cheap; keeps diffs focused on real changes.
- **Keep checkup-finding IDs (H4, M2, L1, …) out of commit subjects.** The findings doc is
  temporary — once an item ships and the entry is deleted, the ID stops resolving and the
  subject becomes a dangling reference. Body may mention an ID when the commit itself touches
  the findings doc (so the diff explains the ID's last appearance), but the subject reads
  by intent, not by tracker ID.

## Collaboration

Three north stars:

1. **Clean, robust, maintainable architecture.** New abstractions earn their place by reducing total
   surface area or making extension easier. Modules have narrow responsibilities. `main.rs` stays
   short — file-type-specific logic lives in `compose_modes` and the modes themselves.
2. **Stream, don't load.** Multi-GB files are first-class. Prefer
   `InputSource::open_byte_source()` (random access) or chunked iteration over `read_bytes()` /
   `read_text()` (whole-file). Whole-file reads only when the feature truly needs it (full-file
   pretty-print of structured data, image decode) — never as a casual default.
3. **Keep cognitive load low.** What matters is what the next reader has to hold in their head.
   Abstractions can reduce that load (named trait → stop thinking about mechanism) or add to it
   (chasing four files for one operation). Inlining cuts both ways. Type count, line count, and
   call-site count aren't the test — what the reader has to track is.

Be a critical collaborator. Push back when a change would:

- **Damage architecture quality** — leak abstractions, blur boundaries, conflate orthogonal
  concerns (mixing print-mode + interactive paths), or re-introduce a `match file_type` chain that
  `compose_modes` was meant to eliminate.
- **Add cognitive load without payoff** — deep branching, scattered state synced by hand, mechanism
  leaking through call sites, indirection that doesn't earn the click-through, hypothetical-future
  abstractions whose concept isn't real yet.
- **Hurt performance** — redundant re-renders, hot-path allocations, full-file reads where streaming
  or seeking would do, eager work that should be lazy.

Surface the trade-off concretely; propose an alternative.

## Conventions

[docs/conventions.md](docs/conventions.md).

## Documentation

Keep these in sync with code changes:

- **README.md** — project overview, feature summary, usage examples
- **manual/src/** — user-facing manual (mdbook). Update the relevant chapter when a
  user-visible feature changes
- **docs/architecture.md** — design, data flow, key abstractions, how to extend
- **docs/architecture-map.md** — full file/module breakdown. Update when files / modules are
  added, moved, or removed
- **docs/features.md** — currently shipped features (✅ + ◐). Engineering-detail superset of
  the manual; manual stays concise
- **docs/planned.md** — planned features and open ideas (☐ + ❓)
- **docs/conventions.md** — coding conventions
- **docs/release.md** — release pipeline and recovery
- **CLAUDE.md** — top-level architecture overview (update when top-level structure changes)

### Docs hygiene

- `docs/` holds **live reference only** — features, planned, conventions, architecture,
  and in-progress plans. Anything here must reflect current code.
- **Plans are temporary.** When a plan is done:
  - If it has lasting historical value (design rationale, why-we-rejected, postmortem),
    move it to `docs/archived/`. Add a status blockquote right under the title:
    `> **Status: Completed YYYY-MM-DD.** Archived for reference.` Title stays the same.
    Linked references in other docs must point at the archived path. No entry in
    `architecture-map.md` — archived files are a graveyard, not part of the live map.
  - Otherwise, delete it.
  - Either way, it must not stay in `docs/` root as a "landed" plan.
- **Active instructions belong in their own doc** (or as a section of an existing general
  doc like `architecture.md` / `conventions.md`), never inside a plan file. Example: an
  "adding a new file type" checklist lives in `architecture.md`, not buried in a refactor
  plan that future readers won't know to open.
