use std::rc::Rc;

use anyhow::Result;

use crate::input::InputSource;
use crate::input::detect::FileType;
use crate::theme::{PeekThemeName, StyleMode, ThemeManager};
use crate::viewer::modes::{
    AboutMode, ContentMode, ContentModeConfig, HelpMode, HexMode, InfoMode, Mode, PrettyView,
};
use crate::viewer::ui::help::HelpSection;
use crate::viewer::ui::{GLOBAL_ACTIONS, HelpEntry};

pub mod cell_size;
pub mod hex;
pub mod highlight;
pub mod image_render;
pub mod listing;
pub mod modes;
pub mod paged;
pub mod search;
pub mod table;
pub mod ui;
pub mod wrap_scroll;

pub use highlight::highlight_lines;
pub use highlight::{LineStreamHighlighter, syntax_token_for};

/// Shared services threaded through each `types::<x>::compose::compose`
/// call. Holds the `ThemeManager` plus the two helpers (image config,
/// generic text content mode) that per-type compose bodies need to build
/// their mode stacks. The active theme name is `theme_manager.theme_name`;
/// plain mode is `ComposeOpts::plain` (every compose receives `args`) —
/// neither is mirrored here.
pub struct ComposeCtx {
    pub theme_manager: Rc<ThemeManager>,
}

/// CLI-derived configuration the compose path reads — the subset of
/// `cli::Args` that mode construction needs, as plain values. The bin
/// builds it (`Args::compose_opts`) and threads `&ComposeOpts` through
/// `Registry::new` / `compose_modes` / every `types::<x>::compose`, so
/// the reader/compose layer never depends on clap. (clap stays in the
/// bin; this is what keeps it out of `peek-types`, where the per-type
/// `compose` functions live.)
#[derive(Clone)]
pub struct ComposeOpts {
    pub theme: PeekThemeName,
    pub color: StyleMode,
    pub plain: bool,
    pub raw: bool,
    pub line_numbers: bool,
    pub no_svg_anim: bool,
    pub language: Option<String>,
    pub width: u32,
    pub margin: u32,
    pub image_mode: String,
    pub background: String,
    pub edge_density: f32,
}

impl ComposeCtx {
    /// Build a `ContentMode` for text-based file types: source code,
    /// structured (lazy pretty-print), plain text, or SVG XML.
    ///
    /// Constructs a `LineSource` over the input — one streaming pass to
    /// count lines and capture sparse anchors — instead of reading the
    /// whole file into memory. Pretty-print is deferred to the first
    /// time pretty view is rendered, capped at `PRETTY_MAX_BYTES`.
    /// `pretty` is the pre-built pretty-print branch (or `None`), supplied
    /// by the caller via `types::structured::pretty_view_for`. That branch
    /// carries its own default-view intent (`starts_default`), so this
    /// method does no format reasoning — it names no `types::*` reader
    /// module and never matches on which formats pretty-print.
    pub fn text_content_mode(
        &self,
        source: &InputSource,
        file_type: &FileType,
        args: &ComposeOpts,
        pretty: Option<PrettyView>,
    ) -> Result<Box<dyn Mode>> {
        let line_source = source.open_line_source()?;

        let syntax_token = if args.plain {
            None
        } else {
            syntax_token_for(args.language.as_deref(), source, file_type)
        };

        // Pretty is the default view whenever the branch opts into it
        // (available + lossless round-trip — the reader decides) and the
        // user hasn't forced `--raw`. `r` still toggles either way.
        let start_pretty = pretty.as_ref().is_some_and(PrettyView::starts_default) && !args.raw;

        let label: &'static str = match file_type {
            FileType::SourceCode { .. } => "Source",
            FileType::Svg | FileType::Html | FileType::Markdown => "Source",
            FileType::PostScript(_) | FileType::Email(_) | FileType::VObject(_) => "Source",
            _ => "Content",
        };

        Ok(Box::new(ContentMode::new(
            source.clone(),
            line_source,
            Rc::clone(&self.theme_manager),
            self.theme_manager.theme_name,
            ContentModeConfig {
                label,
                syntax_token,
                pretty,
                start_pretty,
                line_numbers: args.line_numbers,
            },
        )))
    }
}

/// Build the image-render configuration from CLI args. A free function,
/// not a `ComposeCtx` method — it reads only `args`, nothing the
/// `ComposeCtx` bundle carries.
pub fn image_config(args: &ComposeOpts) -> crate::viewer::image_render::ImageConfig {
    use crate::viewer::image_render::{Background, FitMode, ImageConfig, ImageMode};
    ImageConfig {
        mode: ImageMode::from_str(&args.image_mode),
        width: args.width,
        background: Background::from_str(&args.background),
        margin: args.margin,
        style_mode: args.color,
        edge_density: args.edge_density,
        fit: FitMode::Contain,
    }
}

/// Push `mode` onto `modes` only if no entry with the same `ModeId` is
/// already present. Used in `compose_modes` so the universal Hex/Info/About
/// tail can run unconditionally without doubling up on a mode a file-type
/// arm has already pushed.
fn push_unique_mode(modes: &mut Vec<Box<dyn Mode>>, mode: Box<dyn Mode>) {
    let id = mode.id();
    if modes.iter().any(|m| m.id() == id) {
        return;
    }
    modes.push(mode);
}

/// Append the universal view tail every frame gets: Hex (when a hexable
/// byte stream is given), Info, About, then a Help screen sectioned per
/// mode. Called at the end of `compose_modes` for top-level frames, and
/// by descend builders so synthetic frames (mbox message, spreadsheet
/// sheet, SQLite table) don't drift from real ones. Dedupes by `ModeId`,
/// so a caller that pre-pushed Info/About doesn't double up.
///
/// `hex_source` is `None` when the frame has no byte stream worth dumping:
/// a directory, or a synthetic frame whose `source` is the parent
/// container (a sheet / table reuses the whole-workbook / whole-db
/// source, so hexing it would dump the container, not the view). Pass
/// `Some(src)` only when `src`'s bytes are exactly what the frame shows.
pub fn append_universal_modes(
    modes: &mut Vec<Box<dyn Mode>>,
    hex_source: Option<&InputSource>,
) -> Result<()> {
    if let Some(source) = hex_source {
        push_unique_mode(modes, Box::new(HexMode::new(source, 0)?));
    }
    push_unique_mode(modes, Box::new(InfoMode::new()));
    push_unique_mode(modes, Box::new(AboutMode::new()));

    // Help screen: a "Global" section, then one section per mode that has
    // extras (its label as the heading). A mode's entry is dropped from
    // its section when it's already a global, so the global keys aren't
    // repeated. The screen lists every mode the frame has.
    let mut help_sections: Vec<HelpSection> = vec![HelpSection {
        title: "Global".to_string(),
        entries: GLOBAL_ACTIONS.to_vec(),
    }];
    for m in modes.iter() {
        let entries: Vec<HelpEntry> = m
            .extra_actions()
            .iter()
            .filter(|e| !GLOBAL_ACTIONS.contains(e))
            .copied()
            .collect();
        if !entries.is_empty() {
            help_sections.push(HelpSection {
                title: m.label().to_string(),
                entries,
            });
        }
    }
    modes.push(Box::new(HelpMode::new(help_sections)));
    Ok(())
}
