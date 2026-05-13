//! Per-type compose: build the mode stack for raster images.

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::types::image::{AnimationMode, ImageKind, ImageRenderMode};
use crate::viewer::ComposeCtx;
use crate::viewer::modes::Mode;

/// Push the image view modes onto `modes`. Animated GIF/WebP gets
/// [`AnimationMode`] (driven by the Mode trait's tick contract);
/// static raster goes through [`ImageRenderMode`].
pub fn compose(
    source: &InputSource,
    detected: &Detected,
    args: &Args,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let cfg = ctx.image_config(args);
    if let Some(frames) = crate::types::image::pipeline::animate::decode_anim_frames(
        source,
        detected.magic_mime.as_deref(),
    )? {
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
