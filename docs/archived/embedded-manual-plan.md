# Embedded manual plan

> **Status: Completed 2026-11-11.** Archived for reference. Follow-ups live in
> [planned.md → Built-in manual](../planned.md#built-in-manual-).

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
2. **Relative links lose their target.** `[Themes](./viewer/themes.md)` renders as underlined
   `Themes` without the ` (./viewer/themes.md)` suffix that the renderer would otherwise add. This
   applies to any target without a scheme: chapters, `#anchors` and paths. Links with a scheme
   (`https://`, `mailto:`) keep the URL.
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
3. It writes `manual.rs` to `OUT_DIR`. The file holds a `static ENTRIES: &[TocEntry]` in
   `SUMMARY.md` order, where an entry is a part heading or a `Chapter` with a title, a path, a depth
   and a body (`&'static str`), plus a `static SUMMARY: &str` for the container source.

The preprocessing lives in `src/preprocess.rs`. `build.rs` includes it with `#[path]`, and the
crate's unit tests compile it under `cfg(test)`.

`cargo:rerun-if-changed=../../manual/src` keeps incremental builds correct. The crate is never
published to crates.io, so reading files outside the crate directory is fine (see
[release.md](../release.md)).

Failures are errors at build time. These include a `SUMMARY.md` entry with no file, a chapter that
no entry lists, an unclosed `peek-skip` marker and an empty `peek:` comment. A broken manual should
fail CI, not ship.

The 47 chapters total about 125 KB of text. Images are not embedded.

pulldown-cmark becomes a build dependency of the crate. It is already a workspace dependency.

### Container wiring

The manual follows the usual pattern for adding a file type (see
[architecture.md → Adding a new file type](../architecture.md#adding-a-new-file-type)), with one
difference: detection never produces it.

1. **`FileType::Manual`** in `peek-detect`. No magic, extension or sniff ever produces it. Only
   `peek_manual::detected()` builds it. The registered-MIME `match` maps it to `text/markdown`.
2. **Source.** `peek_manual::source()` returns
   `InputSource::Memory { bytes: SUMMARY, name: "peek manual" }`. Hex and the source view then show
   `SUMMARY.md`. A chapter frame is named by its path, so the breadcrumb in the viewer reads
   `peek manual > viewer/themes.md`.
3. **Compose arm** in `src/compose.rs` calls `peek_manual::compose()`. It pushes one `ListingMode`
   over `ManualTocSource`. Hex, Info, About and Help are added automatically as usual.
4. **Gather arm** in `src/gather/` calls `peek_manual::gather_extras()`.
5. **Extract arm** in `src/extract/` calls `peek_manual::extract()`. The key is the chapter path.
   The result is `Extracted` wrapping `InputSource::Memory { bytes: chapter body, name: path }`,
   with a suggested file name of the path's last segment. An unknown key gives the same
   `ExtractError` that archives give.
6. **CLI.** `--manual` in `src/cli.rs`, with no short flag. In `src/input.rs`, `build_input` returns
   the source and the detected value: the manual pair when the flag is set, without reading a path
   or stdin, and otherwise the usual source plus `detect`. After that, `main.rs` goes through the
   normal info, list, extract and interactive dispatch with no further changes. Recursion into
   chapters comes from the existing session stack.

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
- Part headings (`Getting Started`, `File Types`, …) are top-level group rows. They are selectable,
  as directory rows are in `TreeListSource`, so moving to the parent level works, but they don't
  extract. They paint with the new `NameStyle::Heading`: accent colour, without the trailing `/` of
  a directory. `NameStyle` replaces the `is_dir` flag on `NameCell`.
- Chapter rows show the title, indented by depth with the same tree connectors that `TreeListSource`
  uses. The connector helpers moved to `viewer/listing/row.rs` so both sources share them.
- Every chapter row is selectable and extractable, including ones with children. `parent()` points a
  nested chapter at its parent chapter, which gives the sticky breadcrumb.
- The size column shows the size of the preprocessed chapter. The listing has no permission or mtime
  columns.
- `flat_line()` (used by `--list`) prints `path<TAB>title`, so a script can pipe it to `cut -f1` and
  then to `-x`.
- `initial_selection()` is the first chapter (Introduction).

## What shipped

All four steps landed:

1. **Crate and build step.** `crates/peek-manual` with `build.rs` and `src/preprocess.rs`. Unit
   tests cover image drop, link-target strip, `peek:` unwrap, `peek-skip` removal, pass-through of
   untouched text, `SUMMARY.md` parsing and each marker error. Missing or unlisted chapters fail the
   build.
2. **Container type.** `FileType::Manual`, `ManualTocSource`, compose, gather and extract, the three
   hub arms, and `--manual`. The bin has no process-level CLI tests, so the tests go through the
   hubs instead: extract and re-detect a chapter, gather the Info section, and descend from the TOC
   into a chapter in a `ViewerState`. The CLI paths were checked by hand against the debug build.
3. **Content pass.** Of the 23 images, 7 had no caption and needed nothing. 11 image-and-caption
   pairs are wrapped in `peek-skip`, because the caption repeated nearby text or only made sense
   next to the picture. Two captions stand alone and stay. Three sites got a `peek:` replacement:
   the introduction, the Info screen and the Markdown chapter. The introduction has a web-only
   pointer to `--manual`. The pass also fixed the SQLite chapter, which claimed that empty cells are
   NULLs; the table view shows a muted `NULL`.
4. **Docs.** `--manual` in the CLI reference, a "Manual" section in `help-about.md`, the authoring
   rules in `manual/README.md`, the crate in the CLAUDE.md file map and `docs/architecture.md`, and
   entries in `docs/features.md` and the README.

## Known limitations

- **Links can't be followed.** Relative links render as underlined text only. Following them is a
  planned follow-up (see [Later](#later)).
- **`--plain` shows the rewritten source.** `--plain` turns off the rendered Markdown view, so a
  chapter prints as its preprocessed Markdown, where stripped links read `[Themes]()`.
- **Pipes.** Print mode reports an error when the reader closes the pipe early (`| head`), and piped
  output stays coloured without `--color plain`. Both affect every file type and are tracked in
  [checkup-findings.md](../checkup-findings.md).

The chapter-path breadcrumb and the missing `-x` completions are covered under
[Container wiring](#container-wiring) and [User-facing behaviour](#user-facing-behaviour).

## Later

Moved to [planned.md → Built-in manual](../planned.md#built-in-manual-).
