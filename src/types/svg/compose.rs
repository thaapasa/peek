//! Per-type compose: build the mode stack for SVG (rasterized preview
//! / animation + XML source).

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::Detected;
use crate::types::image::{ImageKind, ImageRenderMode};
use crate::types::svg::SvgAnimationMode;
use crate::viewer::ComposeCtx;
use crate::viewer::modes::Mode;

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &Args,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
) -> Result<()> {
    let cfg = ctx.image_config(args);
    let anim = if args.no_svg_anim {
        None
    } else {
        crate::types::image::pipeline::svg_anim::try_parse(source)?
    };
    if let Some(model) = anim {
        modes.push(Box::new(SvgAnimationMode::new(model, cfg)));
    } else {
        modes.push(Box::new(ImageRenderMode::new(
            source.clone(),
            cfg,
            ImageKind::Svg,
        )));
    }
    // Pair the SVG view with its XML source.
    modes.push(ctx.text_content_mode(source, &crate::input::detect::FileType::Svg, args)?);
    Ok(())
}
