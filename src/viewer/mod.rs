use std::path::Path;

use anyhow::Result;

use crate::detect::FileType;
use crate::pager::Output;
use crate::theme::{PeekTheme, ThemeManager};
use crate::Args;

pub mod image;
pub mod interactive;
mod structured;
mod syntax;
mod text;

/// Trait for all file viewers.
pub trait Viewer {
    fn render(&self, path: &Path, file_type: &FileType, output: &mut Output) -> Result<()>;
}

/// Registry of viewers, dispatches by file type.
pub struct Registry {
    syntax_viewer: syntax::SyntaxViewer,
    structured_viewer: structured::StructuredViewer,
    image_viewer: image::ImageViewer,
    text_viewer: text::TextViewer,
    plain_mode: bool,
    peek_theme: PeekTheme,
}

impl Registry {
    pub fn new(args: &Args) -> Result<Self> {
        let theme = ThemeManager::new(args.theme);
        let peek_theme = theme.peek_theme().clone();
        let image_mode = image::ImageMode::from_str(&args.image_mode);
        Ok(Self {
            syntax_viewer: syntax::SyntaxViewer::new(theme, args.language.clone()),
            structured_viewer: structured::StructuredViewer::new(),
            image_viewer: image::ImageViewer::new(args.width, image_mode, peek_theme.clone()),
            text_viewer: text::TextViewer,
            plain_mode: args.plain,
            peek_theme,
        })
    }

    pub fn image_viewer(&self) -> &image::ImageViewer {
        &self.image_viewer
    }

    pub fn peek_theme(&self) -> &PeekTheme {
        &self.peek_theme
    }

    pub fn viewer_for(&self, file_type: &FileType) -> &dyn Viewer {
        if self.plain_mode {
            return &self.text_viewer;
        }

        match file_type {
            FileType::SourceCode { .. } => &self.syntax_viewer,
            FileType::Structured(_) => &self.structured_viewer,
            FileType::Image => &self.image_viewer,
            FileType::Binary => &self.text_viewer,
        }
    }
}
