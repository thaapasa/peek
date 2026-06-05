//! Active-output state for [`ContentMode`](super::content::ContentMode).
//!
//! A `ContentMode` shows one of two outputs at a time — the raw source
//! stream or a pretty-printed form (structured / SVG XML). This module
//! owns the type that names which one is active and (when a pretty form
//! exists at all) the lazy-parse machinery that backs it.

use super::pretty_view::PrettyView;

/// Which output a [`ContentMode`](super::content::ContentMode) is
/// showing, plus (when a pretty form exists) the lazy-parse machinery
/// for it.
///
/// `RawOnly` is the trivial case — source code, plain text, anything
/// without a structured pretty form. `r` is inert.
///
/// `Either` carries the [`PrettyView`] (lazy whole-doc parse + rendered
/// cache) alongside a `showing` tag for which output the user is
/// looking at right now. `r` flips `showing`. A cap-exceeded or
/// parse-failed `PrettyView` locks the user back to `Showing::Raw`;
/// the variant stays `Either` so the status line can surface the
/// "Raw (forced)" label.
///
/// One field on `ContentMode` (`self.rendering`) replaces three:
/// the old `pretty: Option<PrettyView>`, `use_pretty: bool`, and
/// `allow_pretty_toggle: bool` — none of which encoded the
/// "pretty-form-exists-when-showing-pretty" invariant in the type.
pub(super) enum RenderingMode {
    RawOnly,
    Either {
        showing: Showing,
        pretty: PrettyView,
    },
}

/// Which side of an `Either` rendering is currently visible.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Showing {
    Raw,
    Pretty,
}

impl RenderingMode {
    /// `r` is live (a pretty form exists).
    pub(super) fn allow_toggle(&self) -> bool {
        matches!(self, Self::Either { .. })
    }

    /// `Showing::Pretty` and the pretty branch hasn't permanently
    /// failed. Drives the active-branch dispatch in `prepare_window`
    /// and the line-domain check in `tracks_position`.
    pub(super) fn showing_pretty(&self) -> bool {
        matches!(
            self,
            Self::Either {
                showing: Showing::Pretty,
                ..
            }
        )
    }

    pub(super) fn pretty(&self) -> Option<&PrettyView> {
        match self {
            Self::Either { pretty, .. } => Some(pretty),
            Self::RawOnly => None,
        }
    }

    pub(super) fn pretty_mut(&mut self) -> Option<&mut PrettyView> {
        match self {
            Self::Either { pretty, .. } => Some(pretty),
            Self::RawOnly => None,
        }
    }

    /// Borrow the pretty branch only when it is the *active* output —
    /// i.e. `Showing::Pretty`. Drives the prepare/render path so
    /// "showing pretty" and "have the pretty handle" become a single
    /// pattern match instead of two separate guards (`showing_pretty()`
    /// then `pretty_mut().expect(…)`).
    pub(super) fn active_pretty(&self) -> Option<&PrettyView> {
        match self {
            Self::Either {
                showing: Showing::Pretty,
                pretty,
            } => Some(pretty),
            _ => None,
        }
    }

    pub(super) fn active_pretty_mut(&mut self) -> Option<&mut PrettyView> {
        match self {
            Self::Either {
                showing: Showing::Pretty,
                pretty,
            } => Some(pretty),
            _ => None,
        }
    }

    /// `Showing::Pretty` *and* the pretty branch is materialised. Used
    /// to fork prepare_window between the pretty and raw paths in one
    /// check instead of `showing_pretty() && pretty().is_some_and(is_ready)`.
    pub(super) fn is_pretty_ready(&self) -> bool {
        self.active_pretty().is_some_and(PrettyView::is_ready)
    }

    /// Force-flip back to raw — used when the lazy pretty parse fails
    /// (size cap or parse error) and the user must be locked to raw.
    /// `Either` is retained so the status line can render
    /// "Raw (forced)".
    pub(super) fn force_raw(&mut self) {
        if let Self::Either { showing, .. } = self {
            *showing = Showing::Raw;
        }
    }

    /// Flip raw ↔ pretty if togglable and the pretty branch hasn't
    /// permanently failed. Returns `true` when the flip happened; the
    /// caller resets scroll / search on `true` because the line-index
    /// domain changes.
    pub(super) fn toggle(&mut self) -> bool {
        if let Self::Either { showing, pretty } = self
            && !pretty.failed()
        {
            *showing = match showing {
                Showing::Raw => Showing::Pretty,
                Showing::Pretty => Showing::Raw,
            };
            true
        } else {
            false
        }
    }
}
