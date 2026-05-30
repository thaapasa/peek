# Jupyter notebooks

`.ipynb` files are JSON documents of cells. peek renders the cells instead of dumping the raw JSON,
with a dual view like Markdown:

- **Rendered** (default) — the notebook is translated to a single Markdown document and rendered
  through the Markdown pipeline:
  - **Markdown cells** render as prose (headings, lists, emphasis, tables, …).
  - **Code cells** show an `In [n]:` label and the source as a fenced block, syntax-highlighted in
    the kernel language.
  - **Outputs** appear under each code cell: `stdout` / `stderr` streams and `text/plain` results
    as fenced text, `error` outputs as a bold `ename: evalue` line followed by the traceback (ANSI
    colour stripped), and image outputs (`image/png`, …) noted with their MIME type.
- **Source** — the raw notebook JSON, pretty-printed via the structured content mode. Reachable
  with Tab; `r` toggles the raw (unformatted) JSON. Becomes the entry view with `--raw`.

`--plain` drops the rendered view entirely.

Both nbformat 4 and the older nbformat-3 `worksheets` layout parse.

> Inline ASCII rendering of image outputs is a planned follow-up — for now they are noted, not
> drawn.

The Info view adds a Notebook section:

- nbformat version
- Kernel display name and language (+ version)
- Cell count, split into code / markdown / raw
- Output count, with image / error sub-counts
- Highest execution count
