//! EPUB read mode: one chapter at a time.
//!
//! Renders the spine entry at `current` through the shared HTML
//! pipeline (`types::html::render`). `n` / `N` step forward / back
//! through the spine, resetting the scroll offset for the new
//! chapter. The render cache is keyed by `(chapter, width)` so a
//! resize re-renders only the visible chapter and a chapter step
//! reuses prior renders when stepping back.

use anyhow::Result;
use syntect::highlighting::Color;

use crate::input::InputSource;
use crate::output::PrintOutput;
use crate::theme::{PeekTheme, StyleMode};
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window, slice_window};
use crate::viewer::ui::Action;

use super::package::{self, Chapter, Package};

const EXTRA_ACTIONS: &[(Action, &str)] = &[
    (Action::NextChapter, "Next chapter"),
    (Action::PrevChapter, "Previous chapter"),
];

pub(crate) struct EpubReadMode {
    source: InputSource,
    style_mode: StyleMode,
    chapters: Vec<Chapter>,
    current: usize,
    /// Per-chapter rendered cache. Each entry holds the width it was
    /// rendered at; a width change drops the entry on next access.
    cache: Vec<Option<CachedChapter>>,
    warnings: Vec<String>,
}

struct CachedChapter {
    width: usize,
    lines: Vec<String>,
}

impl EpubReadMode {
    pub(crate) fn new(source: InputSource, style_mode: StyleMode, package: Package) -> Self {
        let n = package.chapters.len();
        let mut cache = Vec::with_capacity(n);
        cache.resize_with(n, || None);
        Self {
            source,
            style_mode,
            chapters: package.chapters,
            current: 0,
            cache,
            warnings: Vec::new(),
        }
    }

    fn ensure_rendered(&mut self, width: usize) -> Result<&[String]> {
        if self.chapters.is_empty() {
            return Ok(&[]);
        }
        let idx = self.current;
        let needs = self
            .cache
            .get(idx)
            .and_then(|c| c.as_ref())
            .map(|c| c.width != width)
            .unwrap_or(true);
        if needs {
            let chapter = &self.chapters[idx];
            let mut zip = package::open_zip(&self.source)?;
            let bytes = match package::read_entry(&mut zip, &chapter.full_path) {
                Ok(b) => b,
                Err(e) => {
                    self.warnings.push(format!("chapter {}: {e:#}", idx + 1));
                    Vec::new()
                }
            };
            let lines = if bytes.is_empty() {
                vec![format!("[chapter {} unavailable]", idx + 1)]
            } else {
                crate::types::html::render::render(&bytes, width.max(20), self.style_mode)?
            };
            self.cache[idx] = Some(CachedChapter { width, lines });
        }
        Ok(&self
            .cache
            .get(idx)
            .and_then(|c| c.as_ref())
            .expect("cache populated")
            .lines)
    }
}

impl Mode for EpubReadMode {
    fn id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn label(&self) -> &str {
        "Read"
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let lines = self.ensure_rendered(ctx.term_cols)?;
        let total = lines.len();
        let win = slice_window(lines, scroll, rows);
        Ok(Window { lines: win, total })
    }

    /// Print mode walks every chapter in spine order, separating
    /// each with a blank line. Honors the cache so chapters already
    /// rendered (after Tab + scrolling in interactive mode) reuse
    /// their output. The interactive view stays single-chapter; only
    /// the pipe path materializes the whole book.
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let total = self.chapters.len();
        let saved = self.current;
        for i in 0..total {
            self.current = i;
            let lines = self.ensure_rendered(ctx.term_cols)?;
            for line in lines {
                out.write_line(line)?;
            }
            if i + 1 < total {
                out.write_line("")?;
            }
        }
        self.current = saved;
        Ok(())
    }

    fn total_lines(&self) -> Option<usize> {
        self.cache
            .get(self.current)
            .and_then(|c| c.as_ref())
            .map(|c| c.lines.len())
    }

    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        EXTRA_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        match action {
            Action::NextChapter => {
                if self.chapters.is_empty() {
                    return Handled::No;
                }
                let next = (self.current + 1).min(self.chapters.len() - 1);
                if next == self.current {
                    return Handled::Yes;
                }
                self.current = next;
                Handled::YesResetScroll
            }
            Action::PrevChapter => {
                if self.current == 0 {
                    return Handled::Yes;
                }
                self.current = self.current.saturating_sub(1);
                Handled::YesResetScroll
            }
            _ => Handled::No,
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        if self.chapters.is_empty() {
            return Vec::new();
        }
        vec![(
            format!("ch {}/{}", self.current + 1, self.chapters.len()),
            theme.muted,
        )]
    }

    fn status_hints(&self, _has_return_target: bool) -> Vec<&'static str> {
        if self.chapters.len() <= 1 {
            return Vec::new();
        }
        vec!["n/N:chapter"]
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}
