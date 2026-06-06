# peek

Modern terminal file viewer. Syntax highlighting, structured-data pretty-print, image rendering.

**Single-file viewer.** One path (or stdin) at time. No batch mode, no file list, no `cat`-style
concatenation — those belong to other tools.

## Build & Run

```sh
cargo build --workspace      # debug build (all crates)
cargo build --release        # release build
cargo run -- [args]          # run with arguments
cargo test --workspace       # run ALL tests — bare `cargo test` runs only the bin's
cargo clippy --workspace     # lint ALL crates — bare `cargo clippy` skips the member crates
```

**Always pass `--workspace`** for test / clippy. The reader/viewer/parser layers are member
crates (`peek-foundation`, `peek-types`, …); without `--workspace` cargo touches only the root
`peek` bin, silently skipping the ~670 library-crate tests.

No external runtime deps. Image rendering built in. PDF support use Pdfium — ships beside binary in
release tarball, loaded dynamically at startup. Ghostscript available if found on path,

## Architecture map

Top-level only. The tree below is the file map. Per-file detail (what a module does and *why*)
lives in each file's `//!` module doc-comment — read the file's header when unsure where logic
lives or how a piece works. No separate map doc to keep in sync.

Cargo workspace, five library crates under the thin `peek` binary. The layering is
Cargo-enforced: detection and the parser layer are barred from naming the binary's session layer
(the compose / gather / extract dispatch hubs + the interactive event loop), so a bug parsing
hostile bytes can't reach process / terminal control.

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
  peek-theme/          — theming leaf: PeekTheme semantic roles + paint helpers; PeekThemeName +
                         embedded .tmTheme data (themes/); StyleMode + SGR encoders/tokenizer +
                         ActiveStyle; ThemeManager. Depends on nothing in-tree (parallel to
                         peek-io). Aliased as `crate::theme` via `use peek_theme as theme`.
  peek-foundation/     — reader/viewer toolkit + info base. Sits above theme/io/detect, below
                         peek-types; barred from the bin. lib.rs façade: `theme` alias + `input`
                         re-export of peek-io/peek-detect (so moved modules' `crate::*` resolve).
                         A `testing` feature exposes a few test helpers to the other crates'
                         test builds (off in release).
    viewer/            — Mode trait + ModeId + RenderCtx + ExtractTarget; shared modes
                         (content / pretty_view / gutter / hex / info / about / rendered_text<R>);
                         listing/ (tree TOC); table/ (TableMode + RowsTableMode via RowSource);
                         ui/ primitives (status line / ScreenBuffer / Prompt / Action keys /
                         term-size); image_render vocab (ImageConfig / ImageMode / zoom / scroll /
                         ZoomPanState); paged (PagedImageMode<R> + PageRenderer); search
                         primitives; wrap_scroll; cell_size; highlight. NB: compose_modes /
                         ViewerState / the event loop are NOT here — they're the bin's session layer.
    info/              — FileInfo + InfoExtras trait + Extras (Box<dyn InfoExtras>); render/
                         (dynamic trait dispatch, themed sections) + time fmt. (gather hub → bin.)
    output/print       — PrintOutput (write-once stdout for --print / pipes / --info) + the
                         theme-gradient logo painter (shared with the bin's help screen).
    extract            — extract vocabulary: Extracted / ExtractOptions / ExtractError + the path
                         sanitiser / forward-slash-key helpers. (The dispatch hub → bin.)
    base64, xml        — shared standard-alphabet base64 + XML attribute-unescape helpers.
  peek-types/          — per-file-type readers, one module per type (reader + info + view-mode;
                         the format enum + sniff helpers live in peek-detect, re-exported at each
                         module root). Depends on foundation/detect/io/theme — Cargo bars it from
                         naming the bin's session layer. lib.rs façade mirrors foundation's +
                         `pub mod types`. Owns the parser dependency set (object, cafebabe,
                         rusqlite, pdfium, calamine, symphonia, ttf-parser, fontdue, mail-parser,
                         x509-parser, …). Types:
                         binary, text, markdown, notebook (ipynb), sql, sqlite (read-only via
                         bundled rusqlite), css, structured (JSON/YAML/TOML/XML), csv,
                         spreadsheet (xlsx/xlsm/ods), image (+ ASCII pipeline + SVG anim), html,
                         email (eml/mbox), ebook (epub), document (docx/odt/rtf), pdf, eps (eps/ps),
                         comic (cbz), svg, audio, archive (zip/tar/7z/cpio/ar), directory,
                         disk_image (iso/dmg), objfile, classfile, cert (PEM X.509 / CSR / CRL /
                         keys / SSH pubkey), font (TTF/OTF/TTC — fontdue-rasterised specimen)
  peek-theme/themes/   — Embedded .tmTheme files (idea-dark default + vscode variants)
src/                   — the bin: the thin session layer (CLI + the three dispatch hubs + the
                         interactive event loop). Names member crates directly — `peek_io`,
                         `peek_detect`, `peek_theme`, `peek_foundation::{viewer,info,extract,…}`,
                         `peek_types::types`; no re-export shims.
  main.rs              — CLI entry: resolve source, build Registry, dispatch (info/list/interactive/pipe)
  cli.rs               — Args (clap derive) + `compose_opts()` projection (keeps clap out of the readers)
  update.rs            — `--update` flow: GitHub Releases check + pipe install.sh into sh
  input.rs             — CLI-level stdin/source dispatch (build_source, needs Args). The input
                         foundation itself is peek-io / peek-detect, named directly.
  output.rs            — CLI help + version screens (PrintOutput / logo come from peek-foundation)
  compose.rs           — Registry + the FileType→types::<x>::compose dispatch hub (holds ComposeOpts)
  gather/              — the FileType→types::<x> info-gather dispatch hub
  extract/             — the FileType→types::<x> extract dispatch hub + write (Extracted → disk/stdout)
  viewer_session/      — ViewerState (mode stack + scroll/view cache + extract/descend dispatch +
                         prompt slot) + the interactive event loop
docs/                  — Builder / agent reference (architecture.md = design + index)
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
- **CLAUDE.md file map + module `//!` doc-comments** — the per-file breakdown. When you add / move /
  remove a file, update the tree above and the moved file's `//!` header; there is no separate map doc
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
      references in other docs must point at archived path. No entry in CLAUDE.md's file map —
      archived files are graveyard, not part of the live map.
    - Otherwise, delete it.
    - Either way, must not stay in `docs/` root as "landed" plan.
- **Active instructions belong in own doc** (or as section of existing general doc like
  `architecture.md` / `conventions.md`), never inside plan file. Example: "adding new file type"
  checklist lives in `architecture.md`, not buried in refactor plan future readers won't know to
  open.