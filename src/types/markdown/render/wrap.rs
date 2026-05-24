//! Wrap helper: prepend a static prefix (list indent / blockquote rail
//! / etc.) to every wrapped row, and route the body through the shared
//! `wrap_styled` so inline SGR survives line breaks.

use crate::theme::{Sgr, scan};
use crate::viewer::ui::wrap_styled;

/// Wrap an SGR-styled `body` at `width` columns, prepending `prefix` to
/// each row. The prefix counts toward the width budget.
pub fn wrap_with_prefix(prefix: &str, body: &str, width: usize) -> Vec<String> {
    let prefix_width = display_width(prefix);
    let body_budget = width.saturating_sub(prefix_width).max(1);
    let chunks = wrap_styled(body, body_budget);
    chunks
        .into_iter()
        .map(|chunk| format!("{prefix}{chunk}"))
        .collect()
}

/// Display width of a string with possible embedded SGR escapes — only
/// text tokens contribute. Matches the wrap-time width math.
pub fn display_width(s: &str) -> usize {
    let mut w = 0;
    for tok in scan(s) {
        if let Sgr::Text(text) = tok {
            w += unicode_width::UnicodeWidthStr::width(text);
        }
    }
    w
}
