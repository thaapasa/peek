//! The `FileType → types::<x>::compose` dispatch hub.
//!
//! `Registry::compose_modes` is session-orchestration glue: it maps a
//! detected file type onto the per-type mode-stack builders that live in
//! `types::<x>::compose`. The dispatch match is the single-file overview
//! of which file type yields which view stack. It lives in the bin (not
//! the reader/compose layer) because it is the seam where session config
//! meets the parser modules — the per-type `compose` functions stay pure
//! in `peek-types`; only this match knows the full `FileType` set.

use std::rc::Rc;

use anyhow::Result;
use peek_detect::{ComicFormat, Detected, EbookFormat, FileType};
use peek_foundation::viewer::modes::Mode;
use peek_foundation::viewer::{ComposeCtx, ComposeOpts, append_universal_modes};
use peek_io::InputSource;
use peek_theme::{PeekTheme, PeekThemeName, ThemeManager};
use peek_types::types;

/// File-type-aware mode-stack builder. Holds the shared `ThemeManager`
/// plus the CLI-driven options every mode in the stack needs to consume
/// (plain mode, current theme, image config). Used by both the
/// interactive event loop and the print-mode `render_to_pipe` path —
/// `compose_modes` is the single dispatcher across both.
pub struct Registry {
    theme_manager: Rc<ThemeManager>,
    theme_name: PeekThemeName,
    peek_theme: PeekTheme,
    /// CLI-derived compose options, read by `compose_modes` and the
    /// per-type compose fns. Held here so neither the dispatcher nor the
    /// builder closures have to thread `&Args` (clap) through.
    opts: ComposeOpts,
}

impl Registry {
    pub fn new(opts: &ComposeOpts) -> Result<Self> {
        let theme = Rc::new(ThemeManager::new(opts.theme, opts.color));
        let peek_theme = theme.peek_theme().clone();
        Ok(Self {
            theme_manager: theme,
            theme_name: opts.theme,
            peek_theme,
            opts: opts.clone(),
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
    ) -> Result<Vec<Box<dyn Mode>>> {
        let file_type = &detected.file_type;
        let args = &self.opts;
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
                modes.push(ctx.text_content_mode(
                    source,
                    file_type,
                    args,
                    types::structured::pretty_view_for(file_type, args.plain),
                )?);
            }
            FileType::Html => {
                types::html::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Markdown => {
                types::markdown::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Notebook => {
                types::notebook::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Image => {
                types::image::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Svg => {
                types::svg::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Email(fmt) => {
                types::email::compose::compose(source, detected, args, &ctx, &mut modes, *fmt)?;
            }
            FileType::Ebook(EbookFormat::Epub) => {
                types::ebook::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Document(fmt) => {
                types::document::compose::compose(source, detected, args, &ctx, &mut modes, *fmt)?;
            }
            FileType::Pdf(_) => {
                types::pdf::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::PostScript(_) => {
                types::eps::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Spreadsheet(fmt) => {
                types::spreadsheet::compose::compose(
                    source, detected, args, &ctx, &mut modes, *fmt,
                )?;
            }
            FileType::Presentation(fmt) => {
                types::presentation::compose::compose(
                    source, detected, args, &ctx, &mut modes, *fmt,
                )?;
            }
            FileType::Comic(ComicFormat::Cbz) => {
                types::comic::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Archive(fmt) => {
                types::archive::compose::compose(source, detected, args, &ctx, &mut modes, *fmt)?;
            }
            FileType::DiskImage(fmt) => {
                types::disk_image::compose::compose(
                    source, detected, args, &ctx, &mut modes, *fmt,
                )?;
            }
            FileType::ObjectFile => {
                types::objfile::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Classfile => {
                types::classfile::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::DsStore => {
                types::ds_store::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Audio(fmt) => {
                types::audio::compose::compose(source, detected, args, &ctx, &mut modes, *fmt)?;
            }
            FileType::Csv(fmt) => {
                types::csv::compose::compose(source, detected, args, &ctx, &mut modes, *fmt)?;
            }
            FileType::Sqlite(fmt) => {
                types::sqlite::compose::compose(source, detected, args, &ctx, &mut modes, *fmt)?;
            }
            FileType::Cert(fmt) => {
                types::cert::compose::compose(source, detected, args, &ctx, &mut modes, *fmt)?;
            }
            FileType::Font(_) => {
                types::font::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::VObject(fmt) => {
                types::vobject::compose::compose(source, detected, args, &ctx, &mut modes, *fmt)?;
            }
            FileType::Directory => {
                types::directory::compose::compose(source, detected, args, &ctx, &mut modes)?;
            }
            FileType::Compressed(_) => {
                // Bare-codec streams normally resolve to their inner
                // content upstream via `resolve_transparent`, so reaching
                // this arm means decompression either failed or was
                // *deferred* (a big file in a Default session, awaiting
                // the load prompt). Push nothing file-type-specific — the
                // universal Hex + Info tail below renders the raw
                // compressed bytes, and on failure the FileInfo warning
                // row surfaces the error.
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
        }
    }
}
