//! Shared primitives for paged-render modes (PDF, CBZ, EPUB).
//!
//! Each of those modes presents one entry at a time (page / chapter)
//! and caches its rendered output keyed by viewport size + image
//! config. The cache shape, navigation step logic, and image-config
//! cycle handlers are identical across the three; this module is the
//! single source for those pieces.
//!
//! Per-format render bodies and any mode-specific extras (e.g. EPUB
//! search) stay in each mode's own file — only the truly shared
//! mechanism lives here.

use anyhow::Result;

use crate::theme::StyleMode;
use crate::types::image::pipeline::{Background, FitMode, ImageConfig, ImageMode};
use crate::viewer::modes::Handled;
use crate::viewer::ui::Action;

/// Inputs that affect a single page's rendered output. Stored
/// alongside the cached lines so the cache invalidates automatically
/// when the user cycles color (`c`), background (`b`), image mode
/// (`m`), or fit (`f`) — or when the terminal resizes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageCacheKey {
    pub width: usize,
    pub rows: usize,
    pub style_mode: StyleMode,
    pub image_mode: ImageMode,
    pub background: Background,
    pub fit: FitMode,
}

impl PageCacheKey {
    pub fn build(cfg: &ImageConfig, width: usize, rows: usize, style_mode: StyleMode) -> Self {
        Self {
            width,
            rows,
            style_mode,
            image_mode: cfg.mode,
            background: cfg.background,
            fit: cfg.fit,
        }
    }
}

/// Cached per-entry render: the key that produced it plus the lines.
pub(crate) struct CachedRender {
    pub key: PageCacheKey,
    pub lines: Vec<String>,
}

/// Cap on inline image height in pipe / `--print` mode where
/// `term_rows` is unbounded; otherwise a single page would dominate
/// the output. Shared across paged viewers.
pub(crate) const PIPE_IMAGE_MAX_ROWS: u32 = 30;

/// Translate a `term_rows` value (possibly `usize::MAX` for pipe mode)
/// into a `u32` row count for the image pipeline. Pipe mode is capped
/// at [`PIPE_IMAGE_MAX_ROWS`] so a tall image doesn't dominate output.
pub(crate) fn pipe_rows(rows: usize) -> u32 {
    if rows == usize::MAX {
        PIPE_IMAGE_MAX_ROWS
    } else {
        rows.min(u32::MAX as usize) as u32
    }
}

/// Look up `cache[idx]` and, on miss or key mismatch, render via `f`
/// and store. Returns the cached rendered lines.
///
/// Disjoint-borrows pattern: the caller must split `&mut self.cache`
/// off `self` separately from any fields the closure captures, so the
/// closure doesn't collide with the cache borrow held here. Per-mode
/// render bodies therefore become free helpers taking the fields they
/// need by explicit ref rather than `&mut self`.
pub(crate) fn render_cached<F>(
    cache: &mut [Option<CachedRender>],
    idx: usize,
    key: PageCacheKey,
    f: F,
) -> Result<&[String]>
where
    F: FnOnce(&PageCacheKey) -> Result<Vec<String>>,
{
    let stale = cache
        .get(idx)
        .and_then(|c| c.as_ref())
        .is_none_or(|c| c.key != key);
    if stale {
        let lines = f(&key)?;
        cache[idx] = Some(CachedRender { key, lines });
    }
    Ok(&cache[idx].as_ref().expect("cache populated").lines)
}

/// Move `current` by `delta` clamped to `[0, count)`.
///
/// Returns `Handled::No` for an empty list, `Handled::Yes` for a no-op
/// step (already at the bound), `Handled::YesResetScroll` after a real
/// move.
pub(crate) fn step_paged(current: &mut usize, count: usize, delta: i32) -> Handled {
    if count == 0 {
        return Handled::No;
    }
    let max = count - 1;
    let next = if delta >= 0 {
        (*current).saturating_add(delta as usize).min(max)
    } else {
        (*current).saturating_sub(delta.unsigned_abs() as usize)
    };
    if next == *current {
        return Handled::Yes;
    }
    *current = next;
    Handled::YesResetScroll
}

/// Handle the five image-config cycle keys. Returns `Some(Handled::Yes)`
/// when the action matches one of them; `None` when it doesn't (caller
/// continues its own `match`).
pub(crate) fn cycle_image_config(action: Action, cfg: &mut ImageConfig) -> Option<Handled> {
    match action {
        Action::CycleBackground => {
            cfg.background = cfg.background.next();
            Some(Handled::Yes)
        }
        Action::CycleBackgroundBack => {
            cfg.background = cfg.background.prev();
            Some(Handled::Yes)
        }
        Action::CycleImageMode => {
            cfg.mode = cfg.mode.next();
            Some(Handled::Yes)
        }
        Action::CycleImageModeBack => {
            cfg.mode = cfg.mode.prev();
            Some(Handled::Yes)
        }
        Action::CycleFitMode => {
            cfg.fit = cfg.fit.next();
            Some(Handled::Yes)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_paged_advances_and_clamps() {
        let mut cur = 0;
        assert_eq!(step_paged(&mut cur, 3, 1), Handled::YesResetScroll);
        assert_eq!(cur, 1);
        assert_eq!(step_paged(&mut cur, 3, 1), Handled::YesResetScroll);
        assert_eq!(cur, 2);
        // Already at end: no-op.
        assert_eq!(step_paged(&mut cur, 3, 1), Handled::Yes);
        assert_eq!(cur, 2);
        assert_eq!(step_paged(&mut cur, 3, -1), Handled::YesResetScroll);
        assert_eq!(cur, 1);
        // Backward past zero: clamps to 0.
        assert_eq!(step_paged(&mut cur, 3, -5), Handled::YesResetScroll);
        assert_eq!(cur, 0);
        assert_eq!(step_paged(&mut cur, 3, -1), Handled::Yes);
        assert_eq!(cur, 0);
    }

    #[test]
    fn step_paged_empty_list() {
        let mut cur = 0;
        assert_eq!(step_paged(&mut cur, 0, 1), Handled::No);
    }

    #[test]
    fn pipe_rows_caps_unbounded() {
        assert_eq!(pipe_rows(usize::MAX), PIPE_IMAGE_MAX_ROWS);
        assert_eq!(pipe_rows(42), 42);
    }
}
