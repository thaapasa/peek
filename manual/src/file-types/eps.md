# EPS and PostScript

peek opens `.eps` (Encapsulated PostScript) and `.ps` (PostScript) files. PostScript is a program,
not a plain image. peek shows whatever it can extract or render, in up to four views.

## Modes

To cycle through the modes, press `Tab`.

- **Preview** is the default view when the file has a preview. Many EPS files are binary DOS EPS
  containers, with a raster preview that the design tool embedded. The preview is usually a TIFF.
  peek renders the preview the same way as other images. It shows up instantly and needs no
  interpreter, which is why it's the default when available. To zoom and pan, use the standard keys.
- **Render** shows a true [Ghostscript](https://www.ghostscript.com/) rasterization of the
  PostScript. This view appears only when peek finds a `gs` interpreter on your `PATH`. It's more
  faithful than an embedded preview, because Ghostscript interprets the actual vector program. The
  view renders lazily. The `gs` subprocess runs only when you switch to this view, so it never holds
  up opening a file.
- **Source** shows the PostScript program text. For a binary DOS EPS file, this view shows the
  PostScript section instead of the raw binary wrapper.
- **Info** shows the metadata from the Document Structuring Conventions (DSC) header: title,
  creator, the recipient from the `%%For` comment, creation date, bounding box, language level, and
  page count. The view also shows the embedded preview's format and dimensions, and whether
  Ghostscript is available.

If a file has no embedded preview and `gs` isn't on your `PATH`, it has only the **Source** and Info
views. It opens straight to **Source**.

## Ghostscript

peek never bundles Ghostscript, because it's a large C dependency under the GPL and AGPL licenses.
To turn on the **Render** view, do the following:

1. Install Ghostscript:

   ```sh
   # macOS
   brew install ghostscript
   # Debian / Ubuntu
   sudo apt install ghostscript
   ```

2. Make sure that `gs` is on your `PATH`.

The "Render" line in the Info view shows whether peek found Ghostscript.

## Limitations

- For a `.ps` document with several pages, peek renders only the first page.
- peek detects Windows Metafile (WMF) previews but doesn't render them, because no pure-Rust library
  can rasterize them. To view these files, install `gs`.
- Legacy Illustrator files from before CS2 are pure PostScript. They open in this viewer, but peek
  doesn't label them as Illustrator files.
