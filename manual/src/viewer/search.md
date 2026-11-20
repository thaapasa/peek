# Search

To search, follow these steps:

1. Press `/`. A search prompt opens over the status line.
2. Type a query and press Enter.
3. To move to the next match, press `n`. To move to the previous match, press `p`.

Each match scrolls into view, and the cycle wraps around at the ends. While a search is active, the
status line shows `cur/total`. If nothing matches, it shows `no match`.

## Literal and regular expression search

By default, search matches an **exact substring**. To switch to **regular expression** matching,
press `Ctrl-R` inside the prompt. The regular expression engine runs in linear time, with no
catastrophic backtracking. The prompt title shows the active mode: `Search (literal)` or
`Search (regex)`.

If a regular expression is malformed, peek flashes the parse error and leaves the previous search as
it was. While a regular expression search is active, the match count has the prefix `regex`.

A regular expression matches within one logical line. This means that `^` and `$` anchor to the
start and end of a line, and a pattern can't span a line break.

## Smart case

Both modes use smart case. A query in all lowercase matches without regard to case. If the query has
any uppercase character, the whole query is case-sensitive.

## Match highlight

A match gets its own background **and** foreground color. peek drops the syntax color underneath, so
matched text looks the same whatever it was. The syntax color resumes after the match. Both match
colors come from the theme's accent hue. The current match, the one that `n` and `p` move to, is
vivid. The other matches are muted.

## Where search works

Search works in every view that renders text:

- Source, plain text, structured raw and pretty views, and SVG XML
- The rendered HTML view
- The EPUB **Read** view
- The DOCX, ODT, and RTF **Read** views
- The PDF **Text** view
- The CSV and TSV **Table** view, and the SQLite contents view that uses it
- Every listing table of contents

Two kinds of views search differently from a plain text scan:

- **Table views** for CSV, TSV, and SQLite search each cell separately. A query can't span a
  delimiter.
- **Listing tables of contents**, such as for archives, ISOs, directories, and PDF embeds, match
  only the **last path segment**. For example, `sub/` finds nothing.

## Clearing and limits

To clear the search, press Enter on an empty query. You can also press `Esc` while a search is
active. The first press clears the matches, and a second press goes back or quits as usual. Search
also clears when the set of scanned lines changes. This happens when you toggle between raw and
pretty views, step to another EPUB chapter, or resize the terminal.

The scan is a single pass over the active view, and it stops at 100,000 matches. The raw text scan
and the table cell scan also have a budget of 256 MB for each query. Past the budget, the counts
show as partial, such as `12/3400+`, and peek shows a warning.

> **Planned:** incremental search as you type, search in the file info and hex views, and a lazy,
> bounded "search from here" pass for multi-GB files. See the project's `docs/planned.md`.
