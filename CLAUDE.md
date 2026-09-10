# peek

Modern terminal file viewer. Syntax highlight, structured-data pretty-print, image render.

**Single-file viewer.** One path (or stdin) at a time. No batch, no file list, no `cat`-style
concat — those belong to other tools.

## Build & Run

```sh
cargo build --workspace      # debug, all crates
cargo build --release        # release
cargo run -- [args]
cargo test --workspace       # ALL tests — bare `cargo test` runs only the bin's
cargo clippy --workspace     # ALL crates — bare skips member crates
```

**Always `--workspace`** for test/clippy. Without it cargo targets only root `peek` bin and
skips library-crates.

**Toolchain pinned in `rust-toolchain.toml`** (1.97.1). Local + CI + release all resolve there;
rustup installs it on demand. Bump it in its own commit — a new rustc means new clippy lints.
MSRV (`rust-version` in root `Cargo.toml`) is a separate, lower floor and isn't CI-verified.

**Verify against the debug build** — `cargo run -- [args]` or `target/debug/peek`. Release builds
take minutes; never `cargo build --release` just to check a change. Reserve release for shipping.

No external runtime deps. Image render built in. PDF via Pdfium (ships beside binary, loaded
dynamically at startup). Ghostscript used if on path.

## Architecture map

Top-level only. Per-file detail (what + *why*) lives in each file's `//!` header — read it when
unsure where logic lives. No separate map doc.

Cargo workspace: 5 library crates under thin `peek` bin. **Layering Cargo-enforced**: detect +
parser layer barred from naming the bin's session layer (compose/gather/extract dispatch hubs +
event loop), so a bug parsing hostile bytes can't reach process/terminal control.

```
crates/
  peek-io/          input foundation. InputSource (File/Memory/FileRange/TempFile) + ByteSource +
                    LineSource (streaming, anchor-indexed) + ByteStream; single-stream codecs
                    (gz/bz2/xz/zst/lz4/br); stdin + /dev/tty reopen; limits (memory budget
                    classes). Depends on nothing in-tree.
  peek-detect/      file-type detection. FileType + per-type format enums + magic/extension/
                    content-sniff (detect.rs) + mime + transparent decompress-redetect. One
                    types/<type>.rs per type. Depends on peek-io only — NOT readers.
  peek-theme/       theming leaf. PeekTheme roles + paint; PeekThemeName + embedded .tmTheme
                    (themes/); StyleMode + SGR encode/tokenize + ActiveStyle; ThemeManager.
                    Depends on nothing in-tree. Named `peek_theme` everywhere.
  peek-foundation/  reader/viewer toolkit + info base. Above theme/io/detect, below peek-types;
                    barred from bin. Names lower crates directly. One `pub use peek_theme as theme`
                    only so `#[derive(InfoView)]` paths resolve. `testing` feature exposes test
                    helpers (off in release).
    viewer/         Mode trait + ModeId + RenderCtx + ExtractTarget; modes/ (shared modes: content/
                    pretty_view/gutter/hex/info/about/help/rendered_text<R>); listing/ (ListingMode over
                    ListSource — TreeListSource for TOCs, directory listing); table/ (TableMode +
                    RowsTableMode via RowSource); ui/ (Action/ScreenBuffer/Prompt/styled/status/
                    term); image_render (ImageConfig/ImageMode/zoom/scroll/ZoomPanState); paged
                    (PagedImageMode<R>); search; wrap_scroll; cell_size; highlight; logo_anim.
                    NB: compose_modes/ViewerState/event loop are NOT here — bin's session layer.
    info/           FileInfo + InfoExtras trait + Extras; render/ (trait dispatch, themed) + json
                    (--info --json) + time fmt; section (InfoNode tree [Row|Line|Block] + InfoView
                    + render_info + Value [Size/Count/Timestamp/Text/Split/…] + Role); rows (InfoRow
                    runtime model). 3 build modes: **derive** (`#[derive(Serialize,InfoView)]`, one
                    struct → both outputs), **InfoRow** (irregular row-shaped: cert, font),
                    **bespoke** (hand InfoView + Serialize). Display Block ≡ JSON sub-object.
    output/print    PrintOutput (write-once stdout for --print/pipes/--info) + logo painter.
    extract         Extracted / ExtractOptions / ExtractError + path sanitiser + key helpers.
    base64, xml     shared base64 + XML attr-unescape.
    derive/         `peek-foundation-derive` proc-macro for `#[derive(InfoView)]`. Walks view
                    struct → InfoNode tree: `#[info(label)]`→Row, `#[info(nest)]`→sub-view,
                    `#[info(skip)]`→JSON-only; title via `#[info(title|title_from)]`. Skips mirror
                    serde. Paths resolve only via `::peek_foundation`. Build-time only.
  peek-types/       per-file-type readers, one module per type (reader + info + view-mode; format
                    enum + sniff live in peek-detect, re-exported). Depends on foundation/detect/
                    io/theme — barred from bin. Foundation re-exported as crate::{base64,extract,
                    info,output,viewer,xml}. Owns parser deps (object, cafebabe, rusqlite, pdfium,
                    calamine, symphonia, ttf-parser, fontdue, mail-parser, x509-parser, …).
                    Types: binary, text, markdown, notebook, sql, sqlite, css, structured (JSON/
                    YAML/TOML/XML), csv, spreadsheet, presentation, image, html, email, ebook,
                    document, pdf, eps, comic, svg, audio, archive, directory, disk_image, objfile,
                    classfile, cert, font, vobject, ds_store.
  peek-theme/themes/  embedded .tmTheme (idea-dark default + light/solarized/github + vscode + iceberg +
                    graveyard/candy-floss/victorian).
src/                bin: thin session layer (CLI + 3 dispatch hubs + event loop). Names member
                    crates directly.
  main.rs           CLI entry: resolve source, build Registry, dispatch (info/list/interactive/pipe).
  cli.rs            Args (clap) + compose_opts() projection (keeps clap out of readers).
  update.rs         --update: GitHub Releases check + pipe install.sh into sh.
  input.rs          CLI stdin/source dispatch (build_source).
  output.rs         CLI help + version screens.
  compose.rs        Registry + FileType→types::<x>::compose hub (holds ComposeOpts).
  gather/           FileType→types::<x> info-gather hub.
  extract/          FileType→types::<x> extract hub + write (Extracted → disk/stdout).
  viewer_session/   ViewerState (state.rs = struct + key dispatch + mode switch; frame.rs =
                    SessionFrame + recursive-peek stack/descend/extract; prompt.rs = modal prompt;
                    render.rs = view cache + recovery + scroll + draw) + event loop.
docs/               builder/agent reference (architecture.md = design + index).
fuzz/               cargo-fuzz crate (excluded, nightly). `just fuzz`. Stable floor in
                    peek-detect/tests/fuzz_detect.rs.
manual/             user manual (mdbook). `mdbook serve manual`.
scripts/            dev tooling. capture-demos.sh = manual stills (freeze/tmux, `just demos`);
                    fetch-pdfium.sh = Pdfium dylib + .pdfium/VERSION (`just pdfium`);
                    bump-version.sh = Cargo.toml/lock version math, prints new ver (`just bump`).
.github/workflows/  ci.yml + release.yml (5-target matrix) + manual.yml.
install.sh          POSIX installer for curl | sh.
```

## Workflow

- **Don't commit unless asked.** User decides what + when.
- **Commit subject: sentence case, plain prose.** No Conventional Commits prefix. Write
  `Derive binary + directory info sections`, not `feat: …`. Capitalise first word.
- **Don't push, open PRs, or trigger CI on own initiative.** Local commits only. User pushes/opens/
  merges so they can amend first. Open PR only on explicit ask.
- **Run `just format` after editing Rust.** Keeps diffs focused. Not bare `cargo fmt`: the import
  options in `rustfmt.toml` are nightly-only (stable warns and skips them), and a single pass
  doesn't always converge — `just format` runs `cargo +nightly fmt` twice for that reason.
- **Keep checkup-finding IDs (H4, M2, …) out of commit subjects.** Findings doc temporary — ID
  dangles once entry deleted. Body may cite ID when commit touches findings doc; subject reads by
  intent.

## Collaboration

Three north stars:

1. **Clean, robust, maintainable architecture.** New abstractions earn place by cutting surface
   area or easing extension. Narrow responsibilities. `main.rs` stays short — file-type logic
   lives in `compose_modes` + the modes.
2. **Stream, don't load.** Multi-GB files first-class. Prefer `open_byte_source()` (random access)
   or chunked iteration over whole-file `read_bytes()`/`read_text()`. Whole-file only when feature
   needs it (pretty-print structured data, image decode) — never casual default.
3. **Keep cognitive load low.** What matters: what next reader holds in head. Abstractions cut load
   (named trait) or add it (chasing 4 files). Type/line/call-site count aren't the test — reader's
   tracking burden is.

Be critical collaborator. Push back when change would:

- **Damage architecture** — leak abstractions, blur boundaries, conflate concerns (print +
  interactive), or re-introduce a `match file_type` chain `compose_modes` killed.
- **Add cognitive load without payoff** — deep branching, hand-synced scattered state, mechanism
  leaking through call sites, indirection not earning the click, hypothetical-future abstractions.
- **Hurt performance** — redundant re-renders, hot-path allocs, full-file reads where stream/seek
  would do, eager work that should be lazy.

Surface trade-off concretely; propose alternative.

## Conventions

[docs/conventions.md](docs/conventions.md).

## Documentation

Keep in sync with code:

- **README.md** — overview, features, usage.
- **manual/src/** — user manual (mdbook). Update chapter on user-visible change.
- **docs/architecture.md** — design, data flow, abstractions, how to extend.
- **docs/memory-streaming.md** — size/streaming guard: threat model, budget classes, mechanisms,
  the rule every whole-file read follows.
- **docs/image-rendering.md** — image render algorithm + glyph-atlas regeneration.
- **docs/theme-conversion.md** — porting external themes into `.tmTheme`.
- **CLAUDE.md file map + `//!` headers** — per-file breakdown. Add/move/remove a file → update
  tree + the file's `//!` header.
- **docs/features.md** — shipped features (✅ ◐). Superset of manual.
- **docs/planned.md** — planned + ideas (☐ ❓).
- **docs/conventions.md** — coding conventions.
- **docs/release.md** — release pipeline + recovery.
- **CLAUDE.md** — top-level architecture (update on structure change).

### Docs hygiene

- `docs/` is **live reference only** — must reflect current code.
- **Plans temporary.** When done: lasting value (rationale, why-rejected, postmortem) → move to
  `docs/archived/` with `> **Status: Completed YYYY-MM-DD.** Archived for reference.` under title;
  fix links; no file-map entry. Else delete. Never leave a landed plan in `docs/` root.
- **Active instructions belong in their own doc** (or a section of architecture.md/conventions.md),
  never inside a plan file. E.g. "adding new file type" checklist lives in architecture.md.
- **Flag contradictions, don't resolve them silently.** When normal work has you reading these
  docs / CLAUDE.md / module headers and two instructions conflict, stop and notify the user — name
  both sources. Don't pick one, don't ignore it. User decides the fix.
- **Keep docs + comments terse.** Lead with the beef, cut boilerplate; tighten verbosity in any
  doc / `//!` header / comment you touch. Full rule: conventions.md → "Writing".
