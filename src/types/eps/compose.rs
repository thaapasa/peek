//! Per-type compose for EPS / PostScript.
//!
//! Mode order (first = default view):
//!   1. **Preview** — embedded DOS-EPS raster, when present. Instant, no
//!      subprocess; the default so opening a file never blocks on
//!      Ghostscript.
//!   2. **Render** — Ghostscript rasterisation, when `gs` is on PATH.
//!      Lazy: only spawns when the tab is actually viewed.
//!   3. **Source** — the PostScript program text (sliced out of the
//!      binary container for a DOS-EPS).
//!   4. **Info** — appended universally by `compose_modes`.
//!
//! `--plain` drops both image views, leaving Source — consistent with
//! every other dual-nature type.

use anyhow::Result;

use crate::input::InputSource;
use crate::input::detect::{Detected, FileType, PostScriptFormat};
use crate::viewer::ComposeOpts;
use crate::viewer::modes::Mode;
use crate::viewer::paged::PagedImageMode;
use crate::viewer::{ComposeCtx, image_config};

use super::dos_eps;
use super::gs;
use super::image_renderer::{EpsImageRenderer, EpsImageSource};

pub fn compose(
    source: &InputSource,
    detected: &Detected,
    args: &ComposeOpts,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let format = match detected.file_type {
        FileType::PostScript(fmt) => fmt,
        _ => PostScriptFormat::Eps,
    };

    let bytes = source.read_bytes()?;
    let header = dos_eps::parse(&bytes);

    if !ctx.plain_mode {
        // 1. Embedded preview. Decode it here rather than lazily so a
        // preview the image crate can't handle (WMF, or an exotic TIFF
        // sub-format like RGBPalette) never becomes a dead
        // "[render unavailable]" tab — it simply isn't offered, and the
        // view falls through to the Ghostscript render.
        if let Some(h) = &header
            && h.preview_kind == Some(dos_eps::PreviewKind::Tiff)
            && let Some(section) = h.preview
            && let Ok(img) =
                image::load_from_memory(&bytes[section.offset..section.offset + section.len])
        {
            modes.push(Box::new(PagedImageMode::with_label(
                EpsImageRenderer::new(EpsImageSource::Preview(std::sync::Arc::new(img))),
                image_config(args),
                "Preview",
            )));
        }

        // 2. Ghostscript render — lazy; only added when an interpreter
        // exists so there's no dead "Render" tab.
        if let Some(exe) = gs::find() {
            let postscript = match &header {
                Some(h) => bytes.slice(h.postscript.offset..h.postscript.offset + h.postscript.len),
                None => bytes.clone(),
            };
            modes.push(Box::new(PagedImageMode::with_label(
                EpsImageRenderer::new(EpsImageSource::Ghostscript {
                    exe,
                    postscript,
                    crop_to_bbox: format.crop_to_bbox(),
                }),
                image_config(args),
                "Render",
            )));
        }
    }

    // 3. Source — the PostScript program. For a binary DOS-EPS the
    // program is a slice of the file; showing the binary container raw
    // would be noise, so source a memory view of just the PS section.
    let source_view = match &header {
        Some(h) => InputSource::memory(
            bytes.slice(h.postscript.offset..h.postscript.offset + h.postscript.len),
            source.name().to_string(),
        ),
        None => source.clone(),
    };
    modes.push(ctx.text_content_mode(&source_view, &FileType::PostScript(format), args)?);
    Ok(())
}
