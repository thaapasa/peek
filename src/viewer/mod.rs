use std::rc::Rc;

use anyhow::Result;
use syntect::easy::HighlightLines;
use syntect::highlighting::Style;

use crate::Args;
use crate::input::InputSource;
use crate::input::detect::{Detected, FileType, StructuredFormat};
use crate::theme::{ColorMode, PeekTheme, PeekThemeName, ThemeManager};
use crate::viewer::modes::{
    AboutMode, AnimationMode, ContentMode, HelpMode, HexMode, ImageKind, ImageRenderMode, InfoMode,
    Mode,
};
use crate::viewer::ui::{Action, GLOBAL_ACTIONS};

pub mod hex;
pub mod image;
pub mod interactive;
pub(crate) mod modes;
pub mod structured;
pub(crate) mod ui;

/// Highlight text content as colored terminal lines.
pub fn highlight_lines(
    content: &str,
    syntax_token: &str,
    tm: &ThemeManager,
    theme_name: PeekThemeName,
    color_mode: ColorMode,
) -> Result<Vec<String>> {
    let syntax = tm
        .syntax_set
        .find_syntax_by_token(syntax_token)
        .or_else(|| tm.syntax_set.find_syntax_by_name(syntax_token))
        .or_else(|| {
            fallback_syntax_token(syntax_token).and_then(|t| tm.syntax_set.find_syntax_by_name(t))
        })
        .unwrap_or_else(|| tm.syntax_set.find_syntax_plain_text());
    let theme = tm.theme_for(theme_name);
    let mut hl = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();
    for line in content.lines() {
        let ranges = hl.highlight_line(line, &tm.syntax_set)?;
        lines.push(ranges_to_escaped(&ranges, color_mode));
    }
    Ok(lines)
}

/// Walk syntect's `(Style, &str)` ranges and emit one line of text with
/// colors encoded according to `color_mode`. Replaces syntect's
/// `as_24_bit_terminal_escaped`, which is hardcoded to 24-bit output.
pub(crate) fn ranges_to_escaped(ranges: &[(Style, &str)], color_mode: ColorMode) -> String {
    let mut out = String::new();
    for (style, text) in ranges {
        out.push_str(&color_mode.fg_seq(style.foreground));
        out.push_str(text);
    }
    out.push_str(color_mode.reset());
    out
}

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
    pub fn compose_modes(
        &self,
        source: &InputSource,
        detected: &Detected,
        args: &Args,
    ) -> Result<Vec<Box<dyn Mode>>> {
        let file_type = &detected.file_type;
        let mut modes: Vec<Box<dyn Mode>> = Vec::new();

        if self.plain_mode {
            // Binary in --plain still goes to Hex (the universal tail);
            // ContentMode requires UTF-8 input.
            if !matches!(file_type, FileType::Binary) {
                modes.push(self.text_content_mode(source, file_type, args)?);
            }
        } else {
            match file_type {
                FileType::SourceCode { .. } | FileType::Structured(_) => {
                    modes.push(self.text_content_mode(source, file_type, args)?);
                }
                FileType::Image => {
                    let cfg = self.image_config(args);
                    // Animated GIF/WebP: AnimationMode owns the frame stack
                    // and drives ticks via the Mode trait. Static image:
                    // ImageRenderMode renders on demand.
                    if let Some(frames) =
                        image::animate::decode_anim_frames(source, detected.magic_mime.as_deref())?
                    {
                        modes.push(Box::new(AnimationMode::new(frames, cfg)));
                    } else {
                        modes.push(Box::new(ImageRenderMode::new(
                            source.clone(),
                            cfg,
                            ImageKind::Raster,
                        )));
                    }
                }
                FileType::Svg => {
                    let cfg = self.image_config(args);
                    modes.push(Box::new(ImageRenderMode::new(
                        source.clone(),
                        cfg,
                        ImageKind::Svg,
                    )));
                    // Pair the rasterized SVG with its XML source view.
                    modes.push(self.text_content_mode(source, file_type, args)?);
                }
                FileType::Binary => {
                    // Default view for binary IS hex; HexMode is appended
                    // below in the always-present block.
                }
            }
        }

        // Hex/Info/Help/About are universal — every file gets these views.
        modes.push(Box::new(HexMode::new(source, 0)?));
        modes.push(Box::new(InfoMode::new()));
        modes.push(Box::new(AboutMode::new()));

        // Help action union: globals + every preceding mode's extras,
        // deduped. Help itself contributes nothing new.
        let mut help_actions: Vec<(Action, &'static str)> = GLOBAL_ACTIONS.to_vec();
        for m in &modes {
            for (a, label) in m.extra_actions() {
                if !help_actions.iter().any(|(b, _)| b == a) {
                    help_actions.push((*a, *label));
                }
            }
        }
        modes.push(Box::new(HelpMode::new(help_actions)));

        Ok(modes)
    }

    /// Build a `ContentMode` for text-based file types: source code,
    /// structured (lazy pretty-print), plain text, or SVG XML.
    ///
    /// Raw text is loaded eagerly here; pretty-print is deferred to the
    /// first time pretty view is rendered (see `ContentMode::ensure_pretty`).
    fn text_content_mode(
        &self,
        source: &InputSource,
        file_type: &FileType,
        args: &Args,
    ) -> Result<Box<dyn Mode>> {
        let raw = source.read_text()?;

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

        // Pretty-print is the default whenever it's available; --raw flips
        // structured/SVG views back to the raw source.
        let initial_use_pretty = pretty_target.is_some() && !args.raw;

        // Structured files expose `r` as a pretty/raw toggle. SVG XML
        // doesn't — there `r` should fall through to cycle_primary so the
        // user can flip rasterized ↔ XML. Source/text have no pretty form.
        let allow_pretty_toggle = matches!(file_type, FileType::Structured(_));

        let label: &'static str = match file_type {
            FileType::SourceCode { .. } => "Source",
            FileType::Svg => "Source",
            _ => "Content",
        };

        Ok(Box::new(ContentMode::new(
            raw,
            pretty_target,
            syntax_token,
            Rc::clone(&self.theme_manager),
            initial_use_pretty,
            allow_pretty_toggle,
            label,
        )))
    }

    fn image_config(&self, args: &Args) -> image::ImageConfig {
        image::ImageConfig {
            mode: image::ImageMode::from_str(&args.image_mode),
            width: args.width,
            background: image::Background::from_str(&args.background),
            margin: args.margin,
            color_mode: args.color,
        }
    }
}

/// Resolve a syntect syntax token for a file. Priority: explicit
/// `--language` override, then the detected `FileType` syntax hint
/// (extension), then the bare filename (catches `Makefile`, `Dockerfile`
/// — syntect matches these by name). Structured/SVG always map to a
/// fixed syntax token.
pub(crate) fn syntax_token_for(
    forced_language: Option<&str>,
    source: &InputSource,
    file_type: &FileType,
) -> Option<String> {
    match file_type {
        FileType::SourceCode { syntax } => forced_language
            .map(String::from)
            .or_else(|| syntax.clone())
            .or_else(|| {
                source
                    .path()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .map(String::from)
            }),
        FileType::Structured(fmt) => Some(
            match fmt {
                StructuredFormat::Json => "JSON",
                StructuredFormat::Yaml => "YAML",
                StructuredFormat::Toml => "TOML",
                StructuredFormat::Xml => "XML",
            }
            .to_string(),
        ),
        FileType::Svg => Some("XML".to_string()),
        _ => None,
    }
}

/// Map file extensions that syntect doesn't natively support to the closest
/// available syntax name.
fn fallback_syntax_token(ext: &str) -> Option<&'static str> {
    match ext {
        "ts" | "tsx" | "mts" | "cts" => Some("JavaScript"),
        "jsx" | "mjs" | "cjs" => Some("JavaScript"),
        "jsonc" | "json5" => Some("JSON"),
        "zsh" | "bash" | "fish" => Some("Bourne Again Shell (bash)"),
        "h" => Some("C++"),
        "hpp" | "hxx" => Some("C++"),
        "cxx" | "cc" => Some("C++"),
        _ => None,
    }
}
