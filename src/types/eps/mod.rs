//! EPS / PostScript support.
//!
//! Three views over a `.eps` / `.ps` file plus DSC metadata:
//!   * **Preview** — the raster preview baked into a binary DOS-EPS
//!     (`dos_eps`), rendered through the image pipeline. Instant, no
//!     interpreter; the default view when present.
//!   * **Render** — a Ghostscript rasterisation (`gs`), when an
//!     interpreter is found on PATH. True vector fidelity. Optional,
//!     never bundled (AGPL/GPL + large C dep), lazily spawned.
//!   * **Source** — the PostScript program text (sliced out of the
//!     binary container for a DOS-EPS).
//!
//! `info_gather` / `info_render` surface DSC fields (`dsc`), the
//! embedded-preview descriptor, and Ghostscript availability. Neither
//! image view exists without its source — a preview-less `.eps` with no
//! `gs` degrades to source + info.

pub mod compose;
pub mod detect;
pub mod dos_eps;
pub mod dsc;
pub mod format;
pub mod gs;
pub mod image_renderer;
pub mod info;
pub mod info_gather;
pub mod info_render;

pub use info::EpsInfo;

use dos_eps::DosEps;

/// The PostScript program text for DSC parsing / source view. For a
/// binary DOS-EPS that's the PostScript section; otherwise the whole
/// file. Decoded lossily — DSC headers are ASCII, and a stray non-UTF-8
/// byte in the body shouldn't lose the metadata.
fn postscript_text(bytes: &[u8], header: Option<&DosEps>) -> String {
    let slice = match header {
        Some(h) => &bytes[h.postscript.offset..h.postscript.offset + h.postscript.len],
        None => bytes,
    };
    String::from_utf8_lossy(slice).into_owned()
}
