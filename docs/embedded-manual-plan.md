# Embedded manual plan

`peek --manual` opens the user manual inside peek. The manual chapters from `manual/src/` are built
into the binary, and peek shows them with its own Markdown viewer. The manual on the web stays the
same.

Goals:

- The manual always matches the installed version and works offline.
- The manual behaves like any other container. The table of contents (TOC) is a listing, and each
  chapter is an inner item that you can open, extract or print.
- The general Markdown renderer gets no special cases for the manual. Everything specific to the
  manual happens at build time in a new crate.

## User-facing behaviour

| Command                                        | Result                                           |
| ---------------------------------------------- | ------------------------------------------------ |
| `peek --manual`                                | Viewer opens on the TOC                          |
| `peek --manual --list` / `--print`             | TOC printed to stdout                            |
| `peek --manual -x viewer/themes.md`            | Chapter Markdown written to `themes.md`          |
| `peek --manual -x viewer/themes.md -o -`       | Chapter Markdown written to stdout               |
| `peek --manual -x viewer/themes.md -p`         | Rendered chapter printed to stdout               |
| `peek --manual -x viewer/themes.md -p --plain` | Chapter printed without colors or rendering      |
| `peek --manual --info`                         | Info section: chapter count, total size, version |

In the viewer, Enter on a TOC row opens the chapter as a normal Markdown session (rendered view, Tab
to source, search). Esc returns to the TOC. Opening the manual from inside another file's session is
a later step (see [Later](#later)).

The extract key is the chapter path relative to `manual/src/`, the same path that `SUMMARY.md` uses.
Completions for `-x` are out of scope.

`--manual` conflicts with a file path argument, and peek does not read stdin when it is set. Clap
enforces the conflict.

## Content rules for the manual source

These rules apply only to the in-app copy. mdbook ignores HTML comments, so the web manual does not
change.

1. **Images are dropped.** The image Markdown is removed, including the alt text. The manual's
   images are screenshots of peek itself, so they add little in a terminal.
2. **Links to chapters lose their target.** `[Themes](./viewer/themes.md)` renders as underlined
   `Themes` without the ` (./viewer/themes.md)` suffix that the renderer would otherwise add. This
   applies to any relative `.md` target, with or without a `#fragment`. External links keep the URL.
3. **`<!-- peek: … -->` adds in-app content.** The comment body is parsed as Markdown and inserted
   in place of the comment. It is typically placed next to a dropped image:

   ```markdown
   ![The Info view for a JPEG](../img/file-info.svg)

   <!-- peek:
   Press `i` on any file to open this view.
   -->
   ```

4. **`<!-- peek-skip -->` … `<!-- /peek-skip -->` hides web-only content.** For example, the "view
   this manual in peek" pointer on the web would be hidden inside peek.

Limits:

- A comment body can't contain `-->`.
- The markers must start a line (block-level HTML). Inline comments inside a paragraph are left
  alone. That keeps the rewrite simple, and the manual has no need for them.
- Prettier does not reflow text inside HTML comments, so keep comment bodies short and wrap them by
  hand.

Document these rules in a new `manual/README.md` for people who edit the manual.

## Design

### New crate: `crates/peek-manual`

The crate depends on `peek-foundation`, `peek-io` and `peek-detect`. The bin depends on it. Like the
other library crates, it is barred from the bin's session layer.

It does not depend on `peek-types`. A chapter is extracted as an `InputSource::Memory` whose name
ends in `.md`, so normal detection picks `FileType::Markdown` and the existing Markdown reader
renders it. The manual crate only provides the container: the TOC, Info and extract.

Contents:

- `build.rs`: reads `manual/src/`, preprocesses it and writes the generated chapter table to
  `OUT_DIR`. See [Build step](#build-step).
- `lib.rs`: the public entry points the bin calls: `source()`, `detected()`, `compose()`,
  `gather_extras()` and `extract()`.
- `toc.rs`: `ManualTocSource`, a `ListSource` implementation over the chapter table.
- `info.rs`: the derived Info section (chapter count, total bytes, manual version).

The crate reads only bytes that are compiled into the binary, so it adds no parsing surface for
hostile input.

### Build step

`build.rs` runs at compile time and does three things:

1. It parses `manual/src/SUMMARY.md` with pulldown-cmark into an ordered list of entries. An entry
   is a part heading (`# File Types`), or a chapter with a title, a path and a nesting depth.
2. It reads each chapter and applies the [content rules](#content-rules-for-the-manual-source) by
   walking pulldown-cmark events and slicing the source text by event offsets. Only the marked
   ranges change, and the rest of the bytes pass through unchanged. That way the source view in the
   viewer still looks like the authored Markdown.
3. It writes `manual.rs` to `OUT_DIR`. The file holds a `static CHAPTERS: &[Chapter]`, where each
   `Chapter` has a title, a path, a depth, a part and a body (`&'static str`), plus a
   `static SUMMARY: &str` for the container source.

`cargo:rerun-if-changed=../../manual/src` keeps incremental builds correct. The crate is never
published to crates.io, so reading files outside the crate directory is fine (see
[release.md](release.md)).

Failures are errors at build time. These include a `SUMMARY.md` entry with no file, a chapter that
no entry lists, an unclosed `peek-skip` marker and an empty `peek:` comment. A broken manual should
fail CI, not ship.

The 47 chapters total about 125 KB of text. Images are not embedded.

pulldown-cmark becomes a build dependency of the crate. It is already a workspace dependency.

### Container wiring

The manual follows the usual pattern for adding a file type (see
[architecture.md → Adding a new file type](architecture.md#adding-a-new-file-type)), with one
difference: detection never produces it.

1. **`FileType::Manual`** in `peek-detect`. No magic, extension or sniff ever produces it. Only
   `peek_manual::detected()` builds it. The `match` arms for MIME and the type name get a fixed
   entry (`text/markdown`, `peek manual`).
2. **Source.** `peek_manual::source()` returns
   `InputSource::Memory { bytes: SUMMARY, name: "peek manual" }`. Hex and the source view then show
   `SUMMARY.md`, and the breadcrumb in the viewer reads `peek manual > Themes`.
3. **Compose arm** in `src/compose.rs` calls `peek_manual::compose()`. It pushes one `ListingMode`
   over `ManualTocSource`. Hex, Info, About and Help are added automatically as usual.
4. **Gather arm** in `src/gather/` calls `peek_manual::gather_extras()`.
5. **Extract arm** in `src/extract/` calls `peek_manual::extract()`. The key is the chapter path.
   The result is `Extracted` wrapping `InputSource::Memory { bytes: chapter body, name: path }`,
   with a suggested file name of the path's last segment. An unknown key gives the same
   `ExtractError` that archives give.
6. **CLI.** `--manual` in `src/cli.rs`, with no short flag. In `src/input.rs`, `build_source`
   returns the manual source and the detected value when the flag is set, and skips path and stdin
   handling. After that, `main.rs` goes through the normal info, list, extract and interactive
   dispatch with no further changes. Recursion into chapters comes from the existing session stack.

Why the hubs rather than a separate code path: `--list`, `--print`, `--extract`, `--info`, recursion
and the breadcrumb all come for free. A separate path would duplicate the dispatch in `main.rs`. The
cost is a `FileType` variant that detection never produces. That's acceptable because the `match`
statements stay exhaustive and the variant documents itself.

### TOC listing: `ManualTocSource`

`TreeListSource` doesn't fit. It shows path segments rather than titles, it lists entries in archive
order, and it never extracts directory rows. In the manual, `file-types/index.md` ("Overview") is
both a page and the parent of the type chapters.

`ManualTocSource` implements `ListSource` directly:

- One row for each `SUMMARY.md` entry, in `SUMMARY.md` order.
- Part headings (`Getting Started`, `File Types`, …) are rows that can't be selected, painted as
  headings.
- Chapter rows show the title, indented by depth with the same tree connectors that `TreeListSource`
  uses. Use the shared helpers in `viewer/listing/row.rs` rather than copying them, and move the
  connector helper there if it is private today.
- Every chapter row is selectable and extractable, including ones with children. `parent()` points a
  nested chapter at its parent chapter, which gives the sticky breadcrumb.
- The size column shows the size of the preprocessed chapter. The listing has no permission or mtime
  columns.
- `flat_line()` (used by `--list`) prints `path<TAB>title`, so a script can pipe it to `cut -f1` and
  then to `-x`.
- `initial_selection()` is the first chapter (Introduction).

## Steps

Each step builds and passes `cargo test --workspace` and `cargo clippy --workspace` on its own.

1. **Crate skeleton and build step.** Add `crates/peek-manual` with `build.rs`, the chapter table
   and unit tests for the preprocessing. Tests cover image drop, link-target strip, `peek:` unwrap,
   `peek-skip` removal, untouched pass-through and each build-error case. Nothing calls the crate
   yet.
2. **Container type.** Add `FileType::Manual`, `ManualTocSource`, compose, gather and extract, the
   three hub arms, and `--manual` in the CLI and `build_source`. Add integration tests for
   `--manual --list`, `-x <path> -o -` (bytes equal the preprocessed body), `-x <path> -p --plain`
   and an unknown key.
3. **Content pass on the manual.** Add `peek:` text next to the images where the in-app copy needs
   it, and add a web-only `peek-skip` pointer to `--manual` in the introduction.
4. **Docs.** Add `--manual` to `manual/src/cli-reference.md`, a short section to `help-about.md` or
   a new chapter, the content rules in a new `manual/README.md`, the new crate in the CLAUDE.md file
   map and the crate structure in `docs/architecture.md`, and an entry in `docs/features.md`. Also
   mention `--manual` in the README usage section.

## Later

- **Following links.** A key that follows the link under the cursor in the rendered Markdown view,
  resolving relative `.md` paths and `#fragment` anchors against the chapter table. This needs link
  spans recorded in the rendered output and a jump hook in the session layer. When it lands, rule 2
  changes from "strip the target" to "keep it as link metadata".
- **Manual in the Tab cycle.** Every file gets a manual mode in its Tab cycle, next to Info and
  Help, that opens on the chapter for the current file type (a CSV file lands on "CSV / TSV"). This
  needs a `FileType` to chapter-path map. The manual crate already depends on `peek-detect`, so the
  map belongs there, and a test can check that every `FileType` maps to a chapter that exists.
- **Search across chapters** from the TOC.
