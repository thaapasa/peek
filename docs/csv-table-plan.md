# CSV / TSV Table View — Implementation Plan

Status: planned. Adds `.csv` / `.tsv` as a first-class file type with an aligned table view,
replacing today's fallback (CSV renders as plain highlighted text — unaligned, hard to read).

Slots into the Structured Data table in [features.md](features.md) and the pending entry in
[planned.md](planned.md#structured-data-additions-).

## Goals

- Aligned column table view as the default render.
- Streaming-safe: multi-GB CSV must open instantly and scroll without a full-file load — same
  bar as archive TOC and `LineSource`.
- Stable layout while scrolling. No per-keypress column reflow.

## Mode stack

Three modes, Tab cycles (mirrors SVG's rendered + source + info shape):

| Mode       | Default | Backing                                                              |
|------------|---------|----------------------------------------------------------------------|
| **Table**  | yes     | New `CsvTableMode` — aligned columns, sticky header                  |
| **Source** | no      | `ContentMode` over the raw CSV bytes, CSV/generic syntax highlight   |
| **Info**   | no      | `InfoMode` — per-column stats (see below)                            |

`x` hex and `h`/`?` help work as everywhere. Print mode (`--print` / non-TTY) renders the table to
stdout, `Contain`-style (unbounded rows, width clamped to terminal — over-wide tables truncate
per-column rather than wrapping).

## Column width strategy: seed + monotonic auto-widen + manual reflow

The width problem: exact alignment wants a full-file pass; streaming forbids it. Resolution:

1. **Seed.** On open, scan the first N records (cap: rows *or* bytes, e.g. 1000 rows / 1 MiB
   whichever first) → initial per-column widths.
2. **Auto-widen (monotonic).** As wider cells scroll into view, a column's width only ever
   *grows*, never shrinks. A wide cell scrolling in bumps its column once and it stays. No
   shrink-reflow, so layout never jumps backwards under the user.
3. **Manual reflow (`R`).** Recomputes every column width from the records currently in the
   viewport. This is the *opt-in* shrink: after scrolling past a block of wide entries the user
   can press `R` to reclaim the space. One deliberate reflow beats constant automatic churn.

Over-wide single cell: truncate with ellipsis in the table; full value visible in the Source view
(or a future cell-detail peek).

## Sticky header

Header row(s) pinned to the viewport top while the body scrolls. Precedent: the listing mode's
sticky parent breadcrumb (`viewer/listing/mode.rs`). Simpler here — fixed top row(s), no ancestry
walk. Painted with the theme's separator/heading styling so it reads as distinct from the body.

### Header detection

- **Heuristic** sets the *initial* state only: first record all-text while later sampled records
  carry typed (numeric/date) cells → treat row 1 as header. Ambiguous → header on by default.
- **`H` toggles** header on/off, overriding the heuristic. When off, row 1 is body data and the
  sticky region is empty.

## Keybindings

| Key | Action             | Notes                                                        |
|-----|--------------------|--------------------------------------------------------------|
| `R` | Reflow widths      | Recompute column widths from current viewport (opt-in shrink)|
| `H` | Toggle header      | Force header on/off; heuristic only seeds the initial state  |

New `Action` variants in `viewer/ui/keys.rs`, table-mode-only. Both surface in the help screen
under the **Table** section (the help screen already sections per mode).

## Horizontal overflow

Total table width > terminal: pan horizontally. `Left` / `Right` step **one column at a time**
(snaps column boundaries to the left edge — cleaner than the 8-column character pan ContentMode
uses for prose). Sticky header pans in lockstep with the body. `Home` returns to column 0.

## Streaming architecture

`LineSource` is line-indexed; CSV records can span physical lines (quoted newlines), so CSV needs
its own record reader — it cannot reuse `LineSource`.

- **`types/csv/parse.rs`** — record reader over `InputSource::open_byte_source()`, using the `csv`
  crate's `Reader` for quoting / escaping / delimiter handling. Builds a **lazy record-offset
  index**: byte offset of each record start, grown on demand as the user scrolls (same anchor-index
  pattern as `LineSource`). Random access = seek to the indexed offset, parse forward.
- **Seed scan** reads the first N records once: feeds initial widths, header heuristic, and the
  type-inference sample.
- Delimiter: `.tsv` → tab, `.csv` → comma; content sniff (`,` vs `\t` vs `;` frequency in the seed
  rows) can override for misnamed files. Stdin: sniff only.

## Info section

Per-column type inference (typed scan over the seed sample):

- **Per column** — header name, inferred type (`int` / `float` / `bool` / `date` / `string`),
  null/empty count, longest cell width.
- **Whole file** — record count (from the offset index once fully built, or "≥ N" while partial),
  column count, delimiter, whether a header was detected.

Type inference is sample-based (seed rows) — flagged as such if the file is larger than the
sample.

## Module layout

```
src/types/csv/
  mod.rs           — module wiring; re-exports CsvTableMode
  format.rs        — CsvFormat (delimiter) + label
  detect.rs        — format_from_ext (.csv/.tsv) + delimiter sniff
  parse.rs         — streaming record reader + lazy record-offset index + seed scan
  compose.rs       — compose(): CsvTableMode + paired Source ContentMode + Info
  table_mode.rs    — CsvTableMode: aligned render, sticky header, auto-widen, R/H, h-scroll
  info.rs          — CsvStats { columns: Vec<ColumnStats>, record_count, delimiter, has_header }
  info_gather.rs   — gather_extras via parse seed scan
  info_render.rs   — render_section (CSV info section)
```

Central wiring: one arm in `viewer/mod.rs::compose_modes`, one entry in `input/detect.rs`
detectors, one arm in the info render match. Matches the existing per-type colocation pattern.

## Dependencies

- `csv` crate — pure Rust, mature, handles quoting / escaping / embedded newlines / delimiter
  config. No spreadsheet engine, no heavy transitive deps.

## Out of scope (v1)

- Cell-detail peek (full value of a truncated cell). Source view covers the need for now.
- Column sort / filter — peek is a viewer, not a query tool.
- Row selection cursor / extract — CSV rows aren't extractable artifacts.
- Frozen first column (row-label pinning) — revisit if wide tables make it painful.

## Resolved decisions

- **Multi-row headers — out for v1.** Single-row header only; `H` stays a binary toggle. A
  count-based header control is a possible follow-up if real multi-header CSVs show up.
- **Terminal resize does not reflow widths.** Resize keeps the monotonic column widths untouched;
  only `R` shrinks. Consistent with the "no automatic shrink" rule — resize changes the viewport,
  not the layout.
