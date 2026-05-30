# EPS & PostScript

`.eps` (Encapsulated PostScript) and `.ps` (PostScript) files. PostScript is a program rather
than a plain image, so peek shows whatever it can extract or render, in up to four views.

## Modes

Cycled with Tab:

- **Preview** (default when present) — many EPS files are "binary DOS-EPS" containers with a
  raster preview (usually TIFF) baked in by the design tool. peek renders that preview through
  the image pipeline. It's instant and needs no interpreter, so it's the default view when
  available. Zoom / pan via the standard keys.
- **Render** — a true [Ghostscript](https://www.ghostscript.com/) rasterisation of the
  PostScript, shown only when a `gs` interpreter is found on your `PATH`. Higher fidelity than a
  baked preview (it interprets the actual vector program). It renders lazily — the `gs`
  subprocess only runs when you switch to this tab, so opening a file is never blocked on it.
- **Source** — the PostScript program text. For a binary DOS-EPS, the PostScript section is
  shown rather than the raw binary wrapper.
- **Info** — DSC header metadata (title, creator, creation date, bounding box, language level,
  page count), the embedded preview's format and dimensions, and whether Ghostscript is
  available.

A file with no embedded preview and no `gs` on your `PATH` opens straight to **Source** + Info.

## Ghostscript

peek never bundles Ghostscript (it's a large GPL/AGPL C dependency). To enable the **Render**
view, install it yourself and make sure `gs` is on your `PATH`:

```sh
# macOS
brew install ghostscript
# Debian / Ubuntu
sudo apt install ghostscript
```

The Info view's "Render" line tells you whether peek found it.

## Limitations

- Multi-page `.ps` documents render only the first page.
- WMF previews are detected but not rendered (no pure-Rust rasteriser) — install `gs` for those.
- Legacy pre-CS2 Illustrator files (pure PostScript) open through this viewer but aren't
  specifically labelled as Illustrator.
