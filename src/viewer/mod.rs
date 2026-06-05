use std::rc::Rc;

use anyhow::Result;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{ComicFormat, Detected, EbookFormat, FileType, StructuredFormat};
use crate::theme::{PeekTheme, PeekThemeName, ThemeManager};
use crate::viewer::modes::{
    AboutMode, ContentMode, ContentModeConfig, HelpMode, HexMode, InfoMode, Mode, PrettyView,
};
use crate::viewer::ui::help::HelpSection;
use crate::viewer::ui::{GLOBAL_ACTIONS, HelpEntry};

pub mod cell_size;
pub mod hex;
pub mod highlight;
pub(crate) mod image_render;
pub mod interactive;
pub(crate) mod listing;
pub(crate) mod modes;
pub(crate) mod paged;
pub(crate) mod search;
pub(crate) mod table;
pub(crate) mod ui;
pub(crate) mod wrap_scroll;

pub use highlight::highlight_lines;
pub(crate) use highlight::{LineStreamHighlighter, syntax_token_for};

/// File-type-aware mode-stack builder. Holds the shared `ThemeManager`
/// plus the CLI-driven options every mode in the stack needs to consume
/// (plain mode, current theme, image config). Used by both the
/// interactive event loop and the print-mode `render_to_pipe` path —
/// `compose_modes` is the single dispatcher across both.
pub struct Registry {
    theme_manager: Rc<ThemeManager>,
    plain_mode: bool,
    theme_name: PeekThemeName,
    peek_theme: PeekTheme,
}

impl Registry {
    pub fn new(args: &Args) -> Result<Self> {
        let theme = Rc::new(ThemeManager::new(args.theme, args.color));
        let peek_theme = theme.peek_theme().clone();
        Ok(Self {
            theme_manager: theme,
            plain_mode: args.plain,
            theme_name: args.theme,
            peek_theme,
        })
    }

    pub fn theme_name(&self) -> PeekThemeName {
        self.theme_name
    }

    pub fn peek_theme(&self) -> &PeekTheme {
        &self.peek_theme
    }

    /// Compose the view-mode list for a given file type. Always appends
    /// Hex, Info, About, and Help so every file gets those views; other
    /// modes are file-type specific. The interactive event loop and the
    /// print-mode pipe path both consume this stack — pipe mode picks
    /// the first non-aux mode (or the first mode if all are aux, e.g.
    /// binary files).
    ///
    /// Per-type arms below delegate to `types::<x>::compose::compose`
    /// so each type owns its mode-building logic; this match is the
    /// single-file overview of the dispatch table.
    pub fn compose_modes(
        &self,
        source: &InputSource,
        detected: &Detected,
        args: &Args,
    ) -> Result<Vec<Box<dyn Mode>>> {
        let file_type = &detected.file_type;
        let mut modes: Vec<Box<dyn Mode>> = Vec::new();
        let ctx = self.compose_ctx();

        // `--plain` is not a separate dispatch: every type composes its
        // normal mode stack. The flag only suppresses syntax highlight
        // / pretty-print inside `ContentMode` (via `ComposeCtx`), and a
        // couple of dual-view types (HTML, SVG) drop their rendered view
        // in favour of raw source. Non-text views (image, PDF page) keep
        // composing — they degrade through `StyleMode::Plain` on their own.
        match file_type {
            FileType::SourceCode { .. } | FileType::Structured(_) => {
                modes.push(ctx.text_content_mode(source, file_type, args)?);
            }
            FileType::Html => {
                crate::types::html::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Markdown => {
                crate::types::markdown::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Notebook => {
                crate::types::notebook::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Image => {
                crate::types::image::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Svg => {
                crate::types::svg::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Email(fmt) => {
                crate::types::email::compose::compose(
                    source, detected, args, &ctx, &mut modes, *fmt,
                )?;
            }
            FileType::Ebook(EbookFormat::Epub) => {
                crate::types::ebook::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Document(fmt) => {
                crate::types::document::compose::compose(
                    source, detected, args, &ctx, &mut modes, *fmt,
                )?;
            }
            FileType::Pdf(_) => {
                crate::types::pdf::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::PostScript(_) => {
                crate::types::eps::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Spreadsheet(fmt) => {
                crate::types::spreadsheet::compose::compose(
                    source, detected, args, &ctx, &mut modes, *fmt,
                )?;
            }
            FileType::Comic(ComicFormat::Cbz) => {
                crate::types::comic::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Archive(fmt) => {
                crate::types::archive::compose::compose(
                    source, detected, args, &ctx, &mut modes, *fmt,
                )?;
            }
            FileType::DiskImage(fmt) => {
                crate::types::disk_image::compose::compose(
                    source, detected, args, &ctx, &mut modes, *fmt,
                )?;
            }
            FileType::ObjectFile => {
                crate::types::objfile::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Classfile => {
                crate::types::classfile::compose::compose(
                    source, detected, args, &ctx, &mut modes,
                )?;
            }
            FileType::Audio(fmt) => {
                crate::types::audio::compose::compose(
                    source, detected, args, &ctx, &mut modes, *fmt,
                )?;
            }
            FileType::Csv(fmt) => {
                crate::types::csv::compose::compose(
                    source, detected, args, &ctx, &mut modes, *fmt,
                )?;
            }
            FileType::Sqlite(fmt) => {
                crate::types::sqlite::compose::compose(
                    source, detected, args, &ctx, &mut modes, *fmt,
                )?;
            }
            FileType::Cert(fmt) => {
                crate::types::cert::compose::compose(
                    source, detected, args, &ctx, &mut modes, *fmt,
                )?;
            }
            FileType::Font(_) => {
                crate::types::font::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::VObject(fmt) => {
                crate::types::vobject::compose::compose(
                    source, detected, args, &ctx, &mut modes, *fmt,
                )?;
            }
            FileType::Directory => {
                crate::types::directory::compose::compose(
                    source, detected, args, &ctx, &mut modes,
                )?;
            }
            FileType::Compressed(_) => {
                // Bare-codec streams resolve to their inner content
                // upstream via `compression::resolve_transparent`,
                // so reaching this arm means decompression failed.
                // Push nothing file-type-specific — the universal
                // Hex + Info tail below renders the raw compressed
                // bytes, and the FileInfo warning row surfaces the
                // decompression error.
            }
            FileType::Binary => {
                // Default view for binary IS hex; HexMode is appended
                // below in the always-present block.
            }
        }

        // Directories opt out of Hex: no byte stream to dump. (The Unix
        // path tolerated `File::open` on a directory as a silent 0-byte
        // file; Windows rejects directory handles outright.)
        let hex_source = (!matches!(file_type, FileType::Directory)).then_some(source);
        append_universal_modes(&mut modes, hex_source)?;
        Ok(modes)
    }

    fn compose_ctx(&self) -> ComposeCtx {
        ComposeCtx {
            theme_manager: Rc::clone(&self.theme_manager),
            theme_name: self.theme_name,
            plain_mode: self.plain_mode,
        }
    }
}

/// Shared services threaded through each `types::<x>::compose::compose`
/// call. Holds the theme/style state plus the two helpers (image config,
/// generic text content mode) that per-type compose bodies need to build
/// their mode stacks.
pub struct ComposeCtx {
    pub theme_manager: Rc<ThemeManager>,
    pub theme_name: PeekThemeName,
    pub plain_mode: bool,
}

impl ComposeCtx {
    /// Build a `ContentMode` for text-based file types: source code,
    /// structured (lazy pretty-print), plain text, or SVG XML.
    ///
    /// Constructs a `LineSource` over the input — one streaming pass to
    /// count lines and capture sparse anchors — instead of reading the
    /// whole file into memory. Pretty-print is deferred to the first
    /// time pretty view is rendered, capped at `PRETTY_MAX_BYTES`.
    pub fn text_content_mode(
        &self,
        source: &InputSource,
        file_type: &FileType,
        args: &Args,
    ) -> Result<Box<dyn Mode>> {
        let line_source = source.open_line_source()?;

        let pretty_target = if !self.plain_mode {
            match file_type {
                FileType::Structured(fmt) => Some(*fmt),
                FileType::Svg => Some(StructuredFormat::Xml),
                _ => None,
            }
        } else {
            None
        };

        let syntax_token = if self.plain_mode {
            None
        } else {
            syntax_token_for(args.language.as_deref(), source, file_type)
        };

        // Pretty-print is the default whenever it's available *and* the
        // round-trip is lossless. `--raw` always flips structured/SVG views
        // back to the raw source. JSONC and JSON5 have lossy pretty paths
        // (comments dropped, JSON5 syntax collapsed) so they default to raw —
        // `r` still toggles for users who want the strict-JSON view.
        let start_pretty =
            pretty_target.is_some() && !args.raw && !pretty_target.is_some_and(is_lossy_pretty);

        // Build the pretty branch here, in the compose hub that holds the
        // structured knowledge, and inject it — `ContentMode` / `PrettyView`
        // stay type-agnostic (no reach into `types::structured`).
        let pretty = pretty_target.map(|fmt| {
            PrettyView::new(
                move |raw: &str| crate::types::structured::pretty::pretty_print(raw, fmt),
                crate::types::structured::info::format_name(fmt),
            )
        });

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
            self.theme_name,
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
pub fn image_config(args: &Args) -> crate::types::image::ImageConfig {
    use crate::types::image::{Background, FitMode, ImageConfig, ImageMode};
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
pub(crate) fn append_universal_modes(
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

/// True when pretty-printing the format drops information from the source
/// (comments / JSON5 features / etc.), so raw should be the default view.
fn is_lossy_pretty(fmt: StructuredFormat) -> bool {
    matches!(fmt, StructuredFormat::Jsonc | StructuredFormat::Json5)
}
