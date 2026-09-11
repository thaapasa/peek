---
name: release-highlight
description: >
  Generate a 1-3 sentence user-facing highlight blurb for the next release.
  Reads commits since the last tag, filters out internal refactors / docs /
  tests / CI / chores, summarises only what a peek user would notice. Output
  is text the user pastes into the release workflow input after review.
  Use when user says "release highlight", "highlight for next release",
  "what's new", "release notes blurb", "/release-highlight".
---

Build a short release-highlight blurb from commits since the last tag. The
user reviews and pastes it into the GitHub release workflow input manually
— do not push, tag, or trigger anything.

## Procedure

1. **Find last tag.**
   ```sh
   git describe --tags --abbrev=0
   ```
   If no tags exist, ask the user for a range instead of guessing.

2. **List commits since that tag.**
   ```sh
   git log <tag>..HEAD --no-merges --format='%h %s%n%b%n---'
   ```
   Include bodies — non-obvious "why" often hides there.

3. **Filter to user-visible changes.** A peek user is someone running the
   CLI: new file formats, new viewers, new keybindings, new flags, visible
   rendering changes, bug fixes they would notice, performance wins big
   enough to feel. Drop:
   - Pure refactors with no behaviour change (renames, module splits,
     trait → enum, helper extraction).
   - Documentation, CLAUDE.md, comments, READMEs.
   - Test additions / fixtures.
   - CI / workflows / release-tooling tweaks.
   - Internal cleanup, lints, formatting.
   - "Checkup" / "quick wins" series unless one ships visible behaviour.

   Conservative call: when unclear whether a change is user-visible, drop
   it. Highlights should over-report nothing.

4. **Group + summarise.** Cluster surviving commits by theme (new format
   support, viewer feature, fix, perf). Pick the 1-3 most prominent.
   Phrase as a release blurb, not a changelog dump.

5. **Output.** Plain prose, 1-3 sentences, ≤ ~300 chars total. Present-
   tense or perfect-tense both fine ("Adds CBZ comic viewing and ISO
   directory browsing." / "CBZ comics and ISO directory browsing now
   render inline."). Drop the version number — user fills that in.

## Style

- User-facing language. Say "comic archives" not "CBZ". Say "spreadsheets"
  only if CSV/TSV would confuse the reader; "CSV files" usually fine.
- Concrete over abstract. "Adds PDF viewing" beats "expands document
  support".
- No commit hashes, no PR refs, no internal module names.
- No "this release" / "we" / "now" filler — go straight to the change.
- If only one user-visible thing landed, one sentence is the answer. Don't
  pad to three.
- If nothing user-visible landed, say so plainly: "No user-visible changes
  since `<tag>` — release is internal cleanup only."

## Examples

✅ Multi-feature release:
> Adds inline viewing for PDF, EPUB, and CBZ comic archives, plus
> directory browsing inside ISO disk images. Embedded files in PDFs and
> audio cover art can now be extracted with `--extract`.

✅ Single-feature release:
> Adds CSV / TSV table view with column type inference, sticky header,
> and in-cell search.

✅ Fix-heavy release:
> Fixes UTF-16 CSV files rendering as garbage and large GIFs hanging on
> first frame. Image rendering now preserves source aspect on terminals
> with non-square cells.

✅ Internal-only:
> No user-visible changes since v0.1.5 — release is refactor / docs only.

❌ Too long / changelog-y:
> This release adds support for PDF files via Pdfium, EPUB ebooks with
> chapter navigation, CBZ comic archives, ISO disk image browsing, audio
> metadata for MP3/FLAC/Ogg, plus 14 bug fixes including ...

❌ Internal jargon:
> Refactors LineProvider trait into LineView enum and shares
> SearchState in ListingMode for cleaner viewer composition.

## Boundaries

Generates the highlight text only. Does not tag, does not push, does not
dispatch the release workflow, does not edit changelog files. Output the
blurb as a fenced block ready for the user to paste.
