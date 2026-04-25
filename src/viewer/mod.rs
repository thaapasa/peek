use std::rc::Rc;

use anyhow::Result;
use syntect::easy::HighlightLines;
use syntect::util::as_24_bit_terminal_escaped;

use crate::Args;
use crate::input::detect::{FileType, StructuredFormat};
use crate::input::InputSource;
use crate::output::Output;
use crate::theme::{ANSI_RESET, PeekTheme, PeekThemeName, ThemeManager};

pub mod hex;
pub mod image;
pub mod interactive;
pub mod structured;
mod syntax;
mod text;
pub(crate) mod ui;

/// Closure type for theme-aware content rendering.
/// The `bool` parameter is `pretty`: true = pretty-print structured data, false = raw.
pub type ContentRenderer = Box<dyn Fn(PeekThemeName, bool) -> Result<Vec<String>>>;

/// Trait for all file viewers.
pub trait Viewer {
    fn render(
        &self,
        source: &InputSource,
        file_type: &FileType,
        output: &mut Output,
    ) -> Result<()>;
}

/// Highlight text content as colored terminal lines.
pub fn highlight_lines(
    content: &str,
    syntax_token: &str,
    tm: &ThemeManager,
    theme_name: PeekThemeName,
) -> Result<Vec<String>> {
    let syntax = tm
        .syntax_set
        .find_syntax_by_token(syntax_token)
        .or_else(|| tm.syntax_set.find_syntax_by_name(syntax_token))
        .or_else(|| {
            fallback_syntax_token(syntax_token)
                .and_then(|t| tm.syntax_set.find_syntax_by_name(t))
        })
        .unwrap_or_else(|| tm.syntax_set.find_syntax_plain_text());
    let theme = tm.theme_for(theme_name);
    let mut hl = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();
    for line in content.lines() {
        let ranges = hl.highlight_line(line, &tm.syntax_set)?;
        let escaped = as_24_bit_terminal_escaped(&ranges, false);
        lines.push(format!("{escaped}{ANSI_RESET}"));
    }
    Ok(lines)
}

/// Registry of viewers, dispatches by file type.
pub struct Registry {
    syntax_viewer: syntax::SyntaxViewer,
    structured_viewer: structured::StructuredViewer,
    image_viewer: image::ImageViewer,
    svg_viewer: image::SvgViewer,
    text_viewer: text::TextViewer,
    hex_viewer: hex::HexViewer,
    theme_manager: Rc<ThemeManager>,
    forced_language: Option<String>,
    plain_mode: bool,
    theme_name: PeekThemeName,
    peek_theme: PeekTheme,
}

impl Registry {
    pub fn new(args: &Args) -> Result<Self> {
        let theme = Rc::new(ThemeManager::new(args.theme));
        let peek_theme = theme.peek_theme().clone();
        let img_config = image::ImageConfig {
            mode: image::ImageMode::from_str(&args.image_mode),
            width: args.width,
            background: image::Background::from_str(&args.background),
            margin: args.margin,
        };
        Ok(Self {
            syntax_viewer: syntax::SyntaxViewer::new(Rc::clone(&theme), args.language.clone()),
            structured_viewer: structured::StructuredViewer::new(Rc::clone(&theme), args.raw),
            image_viewer: image::ImageViewer::new(img_config, args.theme),
            svg_viewer: image::SvgViewer::new(img_config, args.theme, Rc::clone(&theme), args.raw),
            text_viewer: text::TextViewer,
            hex_viewer: hex::HexViewer::new(args.theme),
            theme_manager: theme,
            forced_language: args.language.clone(),
            plain_mode: args.plain,
            theme_name: args.theme,
            peek_theme,
        })
    }

    pub fn image_viewer(&self) -> &image::ImageViewer {
        &self.image_viewer
    }

    pub fn svg_viewer(&self) -> &image::SvgViewer {
        &self.svg_viewer
    }

    pub fn theme_name(&self) -> PeekThemeName {
        self.theme_name
    }

    pub fn peek_theme(&self) -> &PeekTheme {
        &self.peek_theme
    }

    pub fn viewer_for(&self, file_type: &FileType) -> &dyn Viewer {
        if self.plain_mode {
            // --plain: use plain text for non-binary; hex still beats failing on
            // non-UTF-8 bytes for binary.
            return match file_type {
                FileType::Binary => &self.hex_viewer,
                _ => &self.text_viewer,
            };
        }

        match file_type {
            FileType::SourceCode { .. } => &self.syntax_viewer,
            FileType::Structured(_) => &self.structured_viewer,
            FileType::Image => &self.image_viewer,
            FileType::Svg => &self.svg_viewer,
            FileType::Binary => &self.hex_viewer,
        }
    }

    pub fn hex_viewer(&self) -> &hex::HexViewer {
        &self.hex_viewer
    }

    /// Build a closure that renders file content to lines for any given theme.
    /// Used by the interactive viewer for theme-aware re-rendering.
    /// The `pretty` parameter controls whether structured files are pretty-printed.
    pub fn content_renderer(
        &self,
        source: &InputSource,
        file_type: &FileType,
    ) -> Result<ContentRenderer> {
        let raw_content = source.read_text()?;

        // Pre-compute pretty-printed version for structured files
        let pretty_content = if !self.plain_mode {
            if let FileType::Structured(fmt) = file_type {
                Some(structured::pretty_print(&raw_content, *fmt)?)
            } else {
                None
            }
        } else {
            None
        };

        // Determine syntax token for highlighting
        let syntax_token = if self.plain_mode {
            None
        } else {
            self.syntax_token_for(source, file_type)
        };

        let tm = Rc::clone(&self.theme_manager);

        Ok(Box::new(move |theme_name, pretty| {
            let content = if pretty {
                pretty_content.as_deref().unwrap_or(&raw_content)
            } else {
                &raw_content
            };
            if let Some(ref token) = syntax_token {
                highlight_lines(content, token, &tm, theme_name)
            } else {
                Ok(content.lines().map(String::from).collect())
            }
        }))
    }

    fn syntax_token_for(&self, source: &InputSource, file_type: &FileType) -> Option<String> {
        match file_type {
            FileType::SourceCode { syntax } => self
                .forced_language
                .clone()
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
        "dockerfile" | "Dockerfile" => Some("Dockerfile"),
        _ => None,
    }
}
