//! Per-type compose: build the mode stack for raster images.

use anyhow::Result;
use peek_detect::Detected;
use peek_io::InputSource;

use crate::types::image::{AnimationMode, ImageKind, ImageRenderMode};
use crate::viewer::ComposeOpts;
use crate::viewer::modes::Mode;
use crate::viewer::{ComposeCtx, image_config};

/// Push the image view modes onto `modes`. Animated GIF/WebP gets
/// [`AnimationMode`] (driven by the Mode trait's tick contract);
/// static raster goes through [`ImageRenderMode`].
pub fn compose(
    source: &InputSource,
    detected: &Detected,
    args: &ComposeOpts,
    _ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let cfg = image_config(args);
    // A decode failure here (corrupt/truncated animation) is not fatal:
    // fall back to the static raster mode, which attempts its own decode
    // and — if that also fails — degrades to Hex at render time via the
    // viewer's universal fallback. Never abort compose over bad pixels.
    let frames = crate::types::image::pipeline::animate::decode_anim_frames(
        source,
        detected.magic_mime.as_deref(),
    )
    .unwrap_or(None);
    if let Some(frames) = frames {
        modes.push(Box::new(AnimationMode::new(frames, cfg)));
    } else {
        modes.push(Box::new(ImageRenderMode::new(
            source.clone(),
            cfg,
            ImageKind::Raster,
        )));
    }
    Ok(())
}
