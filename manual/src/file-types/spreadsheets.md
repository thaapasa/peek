# Spreadsheets

`.xlsx`, `.xlsm`, and `.ods` workbooks. A workbook is a set of named sheets, each a table, so
peek opens it like a small database: a list of sheets you drill into.

## Modes

Cycled with Tab:

- **Sheets** (default) — the workbook's sheets. `Enter` opens a sheet as an aligned table;
  `e` saves a sheet to a CSV file.
- **Files** — the workbook's raw internal entries (an `.xlsx` / `.ods` is a zip archive),
  browsable and extractable like any archive.
- **Info** — sheet count and names, plus document properties (title, author, dates) when the
  file records them.

## Sheet table

Opening a sheet shows the same aligned table view as CSV and SQLite: a sticky header row,
horizontal panning with `Left` / `Right` when the table is wide, and `/` cell search across
the whole sheet. Numeric columns right-align; text, dates, and booleans left-align. The first
row is treated as a header when it looks like one; `Shift+H` toggles that.

Formulas are not evaluated — peek shows the cached values stored in the file (the same values
a spreadsheet app shows on open).

## Notes

- A sheet is loaded into memory when you open it (the spreadsheet libraries peek uses don't
  stream sheet data). One sheet at a time — opening another releases the previous one.
- Legacy binary `.xls` is not supported; use `.xlsx` / `.ods`.
