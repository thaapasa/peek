# Spreadsheets

peek opens `.xlsx`, `.xlsm`, and `.ods` workbooks. A workbook is a set of named sheets, and each
sheet is a table. peek opens a workbook like a small database: you see a list of sheets and open the
one you want.

## Modes

To cycle through the modes, press `Tab`.

- **Sheets** is the default view. It lists the workbook's sheets.
  - To open a sheet as an aligned table, press `Enter`.
  - To save a sheet to a CSV file, press `e`.
- **Files** shows the raw entries inside the workbook. An `.xlsx` or `.ods` file is a zip archive,
  so you can browse and extract these entries like in any archive.
- **Info** shows the sheet count and names. If the file records them, the view also shows document
  properties such as the title, author, and dates.

## Sheet table

When you open a sheet, peek shows it in the same aligned table view as CSV and SQLite files:

- The header row stays in place while you scroll.
- If the table is wider than the screen, you can pan it with `Left` and `Right`.
- To search the cells of the whole sheet, press `/`.

Numeric columns align to the right. Text, dates, and Boolean values align to the left. If the first
row looks like a header, peek treats it as one. To toggle that, press `Shift+H`.

peek doesn't evaluate formulas. It shows the cached values stored in the file, which are the same
values a spreadsheet app shows when it opens the file.

## Notes

- peek loads a sheet into memory when you open it, because the spreadsheet libraries it uses don't
  stream sheet data. It keeps one sheet in memory at a time. When you open another sheet, peek
  releases the previous one.
- peek doesn't support the legacy binary `.xls` format. Use `.xlsx` or `.ods` instead.
