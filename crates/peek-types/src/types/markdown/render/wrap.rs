//! Wrap helper: prepend a static prefix (list indent / blockquote rail
//! / etc.) to every wrapped row, and route the body through the shared
//! word-aware wrapper so inline SGR survives line breaks and rows cut
//! at word boundaries rather than mid-word.

use crate::viewer::ui::wrap_styled_words;
use peek_theme::display_width;

/// Wrap an SGR-styled `body` at `width` columns, prepending `prefix` to
/// each row. The prefix counts toward the width budget. Cuts at space
/// boundaries; a single word wider than the budget falls back to a hard
/// char-count split.
pub fn wrap_with_prefix(prefix: &str, body: &str, width: usize) -> Vec<String> {
    let prefix_width = display_width(prefix);
    let body_budget = width.saturating_sub(prefix_width).max(1);
    let chunks = wrap_styled_words(body, body_budget);
    chunks
        .into_iter()
        .map(|chunk| format!("{prefix}{chunk}"))
        .collect()
}
