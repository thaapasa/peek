//! Presentation read reader: one slide at a time.
//!
//! Supplies per-slide rendering to the shared
//! [`PagedTextReadMode`](crate::viewer::paged::PagedTextReadMode) shell —
//! `n` / `p` stepping, per-slide search, the render cache, and the whole
//! `Mode` impl live there. Each slide renders through the shared document
//! prose renderer (`crate::types::document::render`); the slides are
//! parsed up front into `Vec<Doc>`, so rendering touches no I/O — it just
//! walks the cached AST. The cache key carries the theme name so a theme
//! cycle re-paints the visible slide.

use anyhow::Result;
use peek_theme::{PeekThemeName, StyleMode};

use crate::types::document::ast::Doc;
use crate::types::document::render;
use crate::viewer::modes::RenderCtx;
use crate::viewer::paged::{PagedText, PagedTextReadMode};
use crate::viewer::ui::{Action, HelpEntry};

const EXTRA_ACTIONS: &[HelpEntry] = &[
    (
        // With a search active these step matches instead of slides.
        &[Action::Next, Action::Prev],
        "Next / previous slide",
    ),
    (&[Action::OpenSearch], "Search"),
];

/// Per-slide render key. Theme name is on the key so a theme cycle drops
/// the cache and re-paints — same shape as the generic `RenderedTextMode`
/// cache key.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct SlideKey {
    width: usize,
    style_mode: StyleMode,
    theme_name: PeekThemeName,
}

pub(crate) struct PresentationReader {
    slides: Vec<Doc>,
}

impl PresentationReader {
    pub(crate) fn into_mode(slides: Vec<Doc>) -> PagedTextReadMode<Self> {
        PagedTextReadMode::new(Self { slides })
    }
}

impl PagedText for PresentationReader {
    type Key = SlideKey;

    fn pages_len(&self) -> usize {
        self.slides.len()
    }

    fn page_label(&self) -> &'static str {
        "slide"
    }

    fn nav_hint(&self) -> &'static str {
        "n/p:slide"
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        EXTRA_ACTIONS
    }

    fn cache_key(&self, ctx: &RenderCtx) -> SlideKey {
        SlideKey {
            width: ctx.term_cols,
            style_mode: ctx.peek_theme.style_mode,
            theme_name: ctx.theme_name,
        }
    }

    fn render_page(&mut self, idx: usize, ctx: &RenderCtx) -> Result<Vec<String>> {
        render::render(
            &self.slides[idx],
            ctx.term_cols,
            ctx.peek_theme,
            ctx.peek_theme.style_mode,
        )
    }
}
