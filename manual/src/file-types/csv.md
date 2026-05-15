# CSV / TSV

| Format | Extensions |
|--------|------------|
| CSV    | `.csv`     |
| TSV    | `.tsv`     |

peek renders CSV and TSV files as an **aligned table**: sticky header row at the top, body
rows scrolled underneath, columns padded to a common width per column.

## View modes

- **Table** (default) — aligned columns with sticky header.
- **Source** — raw bytes through the text viewer.
- **Info** — per-column type inference, record count, malformed counter.

Tab cycles between them. `x` opens the hex view; `h` / `?` show the help screen.

## Column widths

Widths seed from the first 1000 records on open. As the user scrolls, columns grow
monotonically — a wider cell scrolling into view bumps its column once and the new width
stays (the sticky header repaints with the new layout so column titles stay aligned with
the body). Columns never shrink on their own.

`Shift+R` reflows widths from the records currently visible in the viewport — the opt-in
shrink. One deliberate press beats constant automatic churn.

Cells wider than their column truncate with an ellipsis (`…`). The full value is visible in
the Source view.

## Header detection

Row 0 is treated as a header when every cell in row 0 looks like text (not int, float, bool,
or ISO date). A typed cell in row 0 turns the heuristic off — clear signal that row 0 is
data, not a label.

`Shift+H` toggles the header on / off, overriding the heuristic. When off, row 0 is body data
and the sticky region is empty.

## Horizontal pan

If the table is wider than the terminal, `Left` / `Right` step one column at a time. The
sticky header pans in lockstep with the body. The status bar shows `col N/total`.

## Delimiter detection

`.csv` defaults to comma, `.tsv` to tab. The first 64 KiB are sniffed for `,` / `\t` / `;` /
`|`; if a non-default candidate outscores the default by 3× outside quoted spans, it
overrides — covers misnamed files.

## Encoding

UTF-8 is native. UTF-16 LE and UTF-16 BE inputs are detected by BOM and transparently
transcoded to UTF-8 at the byte-source boundary.

## Malformed records

A record that exceeds 4 MiB or spans more than 10 000 physical lines is treated as
malformed (defends against unterminated quoted strings). The csv crate's per-record errors
(column-count mismatch, bad quoting, bad UTF-8) fall into the same bucket.

Malformed rows render as a single `<error>` cell painted in the theme's warning color, and
the status bar shows the running malformed count.

## Print mode

`peek --print foo.csv` (or piping into another tool) emits the table to stdout using the
seed widths only — no auto-widen. A cell wider than its seeded column prints in full and
pushes the rest of that row past the terminal edge (terminal clips); the next row realigns.
