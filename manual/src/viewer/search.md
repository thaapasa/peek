# Search

Press `/` to open a search prompt over the status line. Type a query, press Enter to run it.
`n` / `p` cycle forward / backward through matches (wrapping at the ends), scrolling each into
view. The status line shows `cur/total` while a search is active, or `no match` when nothing
hits.

## Literal and regex

Matching defaults to **exact substring**. `Ctrl-R` inside the prompt toggles **regex** — a
linear-time engine with no catastrophic backtracking; the prompt title shows the active mode
(`Search (literal)` / `Search (regex)`). A malformed regex flashes the parse reason and leaves
the previous search untouched. While a regex search is active the count is prefixed with `regex`.

Regex matches per logical line, so `^` / `$` anchor to line bounds and a pattern can't span a
line break.

## Smart case

Both modes honour smart-case: an all-lowercase query matches case-insensitively; any uppercase
character makes the whole query case-sensitive.

## Match highlight

A match gets an explicit background **and** foreground pair — the syntax colour underneath is
dropped so matched text looks uniform regardless of what it was, then resumes after the span.
Both colours derive from the theme's accent hue: the current `n` / `p` match is vivid, the rest
muted.

## Where it works

Search reaches every text-rendering view — source / plain text / structured raw-pretty / SVG
XML, the rendered HTML view, the EPUB **Read** view, the DOCX / ODT / RTF **Read** views, the
PDF **Text** view, the CSV / TSV **Table** view and the SQLite contents view that shares it, and
every listing TOC. Two scopes differ from a plain text scan:

- **Table views** (CSV / TSV / SQLite) search per cell — a query can't span a delimiter.
- **Listing TOCs** (archives, ISO, directories, PDF embeds, …) match the **last path segment**
  only, so `sub/` finds nothing.

## Clearing and limits

An empty-query Enter clears the search; so does `Esc` while a search is active (it clears matches
first, then falls through to normal back / quit on a second press). Search also clears when the
scanned line set changes underneath it — the raw/pretty toggle, an EPUB chapter step, or a
terminal resize.

The scan is a single pass over the active view, capped at 100,000 matches. The raw text scan and
the table-cell scan are additionally budgeted at 256 MB per query — past it the counts show as
partial (`12/3400+`) and a warning surfaces.

> **Planned:** incremental search-as-you-type, reach into the file-info and hex views, and a
> lazy / bounded "search from here" pass for multi-GB files. See the project's `docs/planned.md`.
