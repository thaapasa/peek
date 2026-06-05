# peek

Modern terminal file viewer. Syntax highlighting, structured-data pretty-print, image rendering.

**Single-file viewer.** One path (or stdin) at time. No batch mode, no file list, no `cat`-style
concatenation — those belong to other tools.

## Build & Run

```sh
cargo build                  # debug build
cargo build --release        # release build
cargo run -- [args]          # run with arguments
cargo test                   # run all tests
cargo clippy                 # lint
```

No external runtime deps. Image rendering built in. PDF support use Pdfium — ships beside binary in
release tarball, loaded dynamically at startup. Ghostscript available if found on path,

## Architecture map

Top-level only. Full file/module breakdown: [docs/architecture-map.md](docs/architecture-map.md) —
read when adding files, modifying module, or unsure where logic lives.

Cargo workspace. Two leaf crates sit below the `peek` binary so detection can be
reviewed/hardened in isolation and is barred (by Cargo) from depending on the reader/viewer layer:

```
crates/
  peek-io/             — input foundation: InputSource (File / Memory / FileRange / TempFile) +
                         ByteSource + LineSource (streaming, anchor-indexed) + ByteStream; the
                         bare single-stream codecs (gz/bz2/xz/zst/lz4/br) + CompressionFormat;
                         stdin read + /dev/tty reopen. Depends on nothing in-tree.
  peek-detect/         — file-type detection: FileType + every per-type format enum +
                         magic-byte / extension / content-sniff classification (detect/) + mime
                         (RFC 6838) + transparent decompress-then-redetect (resolve_transparent).
                         types/<type>.rs = one module per file type (format enum + pure sniff
                         helpers). Depends on peek-io only — NOT the readers.
src/
  main.rs              — CLI entry point: dispatches inputs to viewers
  cli.rs               — Args struct (clap derive)
  base64.rs            — shared standard-alphabet base64 decoder (crate-wide; notebook image extract is first user)
  update.rs            — `--update` flow: GitHub Releases check + pipe install.sh into sh
  xml.rs               — shared XML attribute-unescape helper (docx / odt / epub / structured-xml / spreadsheet)
  input/               — thin façade re-exporting peek-io + peek-detect under the historical
                         `crate::input::*` paths (so the reader layer is unchanged); plus the
                         CLI-level stdin/source dispatch (build_source, needs Args) that stays
                         in the binary
  extract/             — FileType → per-type extractor dispatch; Extracted / Options / Error;
                         path sanitiser; stdout-stream or file write
  output/              — PrintOutput (write-once stdout for --print / pipes / --info);
                         CLI help and version screens
  info/                — FileInfo + FileExtras (per-type stat wrappers); gather/ (per-source
                         collection) + render/ (themed terminal section rendering); time fmt
  theme/               — PeekTheme semantic roles + paint helpers; PeekThemeName + embedded
                         .tmTheme data; StyleMode (truecolor/256/16/grayscale/plain); SGR
                         encoders + tokenizer + ActiveStyle; ThemeManager
  types/               — Per-file-type modules (each owns reader + info + view-mode; the format
                         enum + detection helpers live in `peek-detect`, re-exported at each
                         module root):
                         binary, text, markdown, notebook (ipynb — cells rendered
                         via the markdown pipeline + JSON source), sql, sqlite
                         (read-only via bundled rusqlite — schema listing +
                         streaming row viewer), css,
                         structured (JSON/YAML/TOML/XML), csv,
                         spreadsheet (xlsx/xlsm/ods), image (+ ASCII pipeline +
                         SVG anim), html, email (eml/mbox), ebook (epub),
                         document (docx/odt/rtf), pdf, eps (eps/ps),
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

- **Don't commit unless asked.** User decides what and when.
- **Don't push, open PRs, or trigger GitHub Actions on own initiative.** Local commits only. User
  pushes / opens PRs / merges themselves so they can amend locally first. Open PR only when user
  explicitly asks.
- **Run `cargo fmt` after editing Rust code** so formatting drift no pile up across unrelated files.
  Cheap; keeps diffs focused on real changes.
- **Keep checkup-finding IDs (H4, M2, L1, …) out of commit subjects.** Findings doc temporary — once
  item ships and entry deleted, ID stops resolving and subject becomes dangling reference. Body may
  mention ID when commit itself touches findings doc (so diff explains ID's last appearance), but
  subject reads by intent, not by tracker ID.

## Collaboration

Three north stars:

1. **Clean, robust, maintainable architecture.** New abstractions earn place by reducing total
   surface area or making extension easier. Modules have narrow responsibilities. `main.rs` stays
   short — file-type-specific logic lives in `compose_modes` and the modes themselves.
2. **Stream, don't load.** Multi-GB files first-class. Prefer `InputSource::open_byte_source()` (
   random access) or chunked iteration over `read_bytes()` / `read_text()` (whole-file). Whole-file
   reads only when feature truly needs it (full-file pretty-print of structured data, image
   decode) — never as casual default.
3. **Keep cognitive load low.** What matters: what next reader must hold in head. Abstractions can
   cut that load (named trait → stop thinking about mechanism) or add to it (chasing four files for
   one operation). Inlining cuts both ways. Type count, line count, call-site count aren't the
   test — what reader must track is.

Be critical collaborator. Push back when change would:

- **Damage architecture quality** — leak abstractions, blur boundaries, conflate orthogonal
  concerns (mixing print-mode + interactive paths), or re-introduce `match file_type` chain that
  `compose_modes` meant to eliminate.
- **Add cognitive load without payoff** — deep branching, scattered state synced by hand, mechanism
  leaking through call sites, indirection that no earn the click-through, hypothetical-future
  abstractions whose concept not real yet.
- **Hurt performance** — redundant re-renders, hot-path allocations, full-file reads where streaming
  or seeking would do, eager work that should be lazy.

Surface trade-off concretely; propose alternative.

## Conventions

[docs/conventions.md](docs/conventions.md).

## Documentation

Keep in sync with code changes:

- **README.md** — project overview, feature summary, usage examples
- **manual/src/** — user-facing manual (mdbook). Update relevant chapter when user-visible feature
  changes
- **docs/architecture.md** — design, data flow, key abstractions, how to extend
- **docs/architecture-map.md** — full file/module breakdown. Update when files / modules added,
  moved, or removed
- **docs/features.md** — currently shipped features (✅ + ◐). Engineering-detail superset of manual;
  manual stays concise
- **docs/planned.md** — planned features and open ideas (☐ + ❓)
- **docs/conventions.md** — coding conventions
- **docs/release.md** — release pipeline and recovery
- **CLAUDE.md** — top-level architecture overview (update when top-level structure changes)

### Docs hygiene

- `docs/` holds **live reference only** — features, planned, conventions, architecture, in-progress
  plans. Anything here must reflect current code.
- **Plans are temporary.** When plan done:
    - If lasting historical value (design rationale, why-we-rejected, postmortem), move to
      `docs/archived/`. Add status blockquote right under title:
      `> **Status: Completed YYYY-MM-DD.** Archived for reference.` Title stays same. Linked
      references in other docs must point at archived path. No entry in `architecture-map.md` —
      archived files are graveyard, not part of live map.
    - Otherwise, delete it.
    - Either way, must not stay in `docs/` root as "landed" plan.
- **Active instructions belong in own doc** (or as section of existing general doc like
  `architecture.md` / `conventions.md`), never inside plan file. Example: "adding new file type"
  checklist lives in `architecture.md`, not buried in refactor plan future readers won't know to
  open.