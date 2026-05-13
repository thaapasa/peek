//! Per-type compose: audio. Metadata-only view (no playback) plus the
//! optional Cover / Lyrics / Embeds tabs when the file carries them.

use std::rc::Rc;

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{AudioFormat, Detected};
use crate::types::image::{ImageKind, ImageRenderMode};
use crate::viewer::ComposeCtx;
use crate::viewer::listing::{ListingMode, from_flat_paths};
use crate::viewer::modes::{ContentMode, InfoMode, Mode};

pub fn compose(
    source: &InputSource,
    _detected: &Detected,
    args: &Args,
    ctx: &ComposeCtx,
    modes: &mut Vec<Box<dyn Mode>>,
    fmt: AudioFormat,
) -> Result<()> {
    // Tab order: Info → Cover (when picture is embedded) → Lyrics
    // (when present) → Embeds listing (when either is present). The
    // universal Info push downstream dedupes by ModeId so the explicit
    // Info push here just controls position.
    modes.push(Box::new(InfoMode::new()));
    if let Ok(probed) = crate::types::audio::package::probe(source, fmt) {
        if let Some(visual) = crate::types::audio::package::primary_cover(&probed) {
            let name = crate::types::audio::package::visual_filename(visual);
            let cover_source = InputSource::memory(visual.data.clone(), name);
            modes.push(Box::new(ImageRenderMode::with_label(
                cover_source,
                ctx.image_config(args),
                ImageKind::Raster,
                "Cover",
            )));
        }
        if let Some(lyrics) = &probed.lyrics {
            let lyrics_source = InputSource::memory(lyrics.as_bytes().to_vec(), "lyrics.txt");
            if let Ok(line_source) = lyrics_source.open_line_source() {
                modes.push(Box::new(ContentMode::new(
                    lyrics_source,
                    line_source,
                    None,
                    None,
                    Rc::clone(&ctx.theme_manager),
                    ctx.theme_name,
                    false,
                    false,
                    args.line_numbers,
                    "Lyrics",
                )));
            }
        }
        let entries = crate::types::audio::package::build_listing(&probed);
        if !entries.is_empty() {
            let tree = from_flat_paths(entries);
            modes.push(Box::new(ListingMode::new(
                "Audio",
                "Embeds",
                tree,
                Vec::new(),
            )));
        }
    }
    Ok(())
}
