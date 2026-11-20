# Manual for the current file type

From any file, the user can open the manual chapter for that file type. A CSV file opens "CSV and
TSV," and a PDF opens "PDF and Illustrator." The item comes from
[planned.md → Built-in manual](planned.md#built-in-manual-).

## Open question: mode in the frame, or descend into the manual

planned.md asks for "a manual mode next to Info and Help" in the Tab cycle. These two don't fit
together. Info is in the Tab cycle, but Help is an aux mode: a key toggles it and Tab skips it. So
there are three options:

1. **Manual mode in the Tab cycle.** The compose hub in the bin appends a mode that shows the
   rendered chapter. Only the bin can build it, because the Markdown renderer lives in `peek-types`
   and `peek-manual` doesn't depend on it.
   - Every Tab cycle gets one more stop. For a file with one content view, Tab goes content → Info →
     Manual instead of content → Info.
   - The mode would take the rendered view out of the Markdown mode stack and drop the source view.
     The chapter search would have to work inside this one mode.
   - The user gets one chapter and can't reach the TOC.
   - Following links, which is the next item in planned.md, would have to switch chapters inside the
     mode. That would work separately from the way it works in `peek --manual`.
2. **Aux manual mode with a key, like Help.** Same as option 1, except Tab skips it. The rendering
   and link problems stay.
3. **A key that opens the manual on the session stack (recommended).** The key pushes the manual TOC
   frame and then the chapter frame on top of it. These are the same frames that `peek --manual` and
   Enter on a TOC row build. Esc goes back to the TOC, and Esc again goes back to the file. The
   breadcrumb reads `data.csv > peek manual > file-types/csv.md`.
   - No new mode and no new rendering path. The chapter is a normal Markdown session with the
     rendered and source views, search, and extract.
   - peek builds nothing until the user presses the key, so other files pay nothing.
   - Link following, once it lands, works the same in both places.
   - It doesn't put the manual in the Tab cycle, so planned.md changes to match.

The rest of this plan assumes option 3.

## Smaller choices

1. **Key.** `m` and `M` cycle the image mode. Candidates are `F1`, or a free letter such as `v`.
   Help could also list the key next to its own entry.
2. **One frame or two.** Pushing only the chapter frame means a single Esc returns to the file, but
   the user can't reach the TOC from there. Pushing both frames costs a second Esc.
3. **Inside the manual.** If the current frame is already the manual or one of its chapters, the key
   does nothing, or it jumps to the chapter for the manual itself. That's the "Manual" section of
   `viewer/help-about.md`. The stack depth cap of 16 keeps repeated presses bounded in any case.

## Design

### Chapter map in `peek-manual`

`peek_manual::chapter_for(&FileType) -> &'static str` returns the chapter path. It's an exhaustive
`match` with no wildcard arm, so a new `FileType` variant fails to compile until it has a chapter.
These mappings aren't one-to-one:

- `SourceCode`, including SQL, → `file-types/source-code.md`.
- `Structured(_)` → `file-types/structured-data.md`.
- `Compressed(_)` → `file-types/archives.md`, which covers the single-stream codecs.
- `Manual` → `viewer/help-about.md`.

A unit test calls `chapter_for` with one value per variant and checks that each path is in the
chapter table. The `match` makes sure every variant has an arm, but nothing makes sure that a new
variant gets a value in the test. Unless the test can list the variants some other way, the test
carries a comment next to the list.

### Session wiring

1. Add a new `Action` to the global actions in `viewer/ui/keys.rs`, so it shows in Help and works in
   every mode. Name it to match the key.
2. `ViewerState` handles it in `viewer_session/`. It builds the manual frame from
   `peek_manual::source()` and `detected()`, then calls `peek_manual::extract(chapter_for(..))` and
   pushes the result the same way a TOC descend does. The TOC selection moves to that chapter, so
   Esc lands on its row.
3. `ManualTocSource` gets a way to start on a given chapter path instead of the Introduction.

Print mode and `--info` aren't affected.

## Tests

- The chapter map test in [Chapter map in `peek-manual`](#chapter-map-in-peek-manual).
- A `ViewerState` test: open a CSV source, press the key, and check that the stack is file → manual
  → `file-types/csv.md`. Then press Esc twice to get back to the file.

## Docs

- `keyboard-shortcuts.md` and the "Manual" section of `viewer/help-about.md` in the manual.
- `docs/features.md`, and remove the item from `docs/planned.md`.
- The `//!` headers of the files that change.
