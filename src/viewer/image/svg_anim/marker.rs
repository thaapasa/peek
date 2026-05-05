//! Per-target marker injection + per-frame substitution. The marked
//! string is built once at parse time; [`render_frame`] does the cheap
//! per-frame substitution.

use super::{AnimatedSvg, ResolvedTarget};

pub(super) fn build_marked_svg(text: &str, targets: &[ResolvedTarget]) -> String {
    // Inject `__PEEK_ANIM_OPEN_<i>__` and `__PEEK_ANIM_CLOSE_<i>__`
    // markers around each animated element's children. Sort all
    // injection points descending so byte offsets remain valid as we
    // mutate.
    let mut out = text.to_string();
    let mut points: Vec<(usize, String)> = Vec::with_capacity(targets.len() * 2);
    for (i, t) in targets.iter().enumerate() {
        points.push((t.open_at, format!("__PEEK_ANIM_OPEN_{i}__")));
        points.push((t.close_at, format!("__PEEK_ANIM_CLOSE_{i}__")));
    }
    points.sort_by_key(|(pos, _)| std::cmp::Reverse(*pos));
    for (pos, marker) in points {
        out.insert_str(pos, &marker);
    }
    out
}

/// Substitute the per-target placeholders in `marked` with the transform
/// values for `frame`. Result is a complete SVG document ready to feed to
/// resvg.
pub fn render_frame(model: &AnimatedSvg, frame_idx: usize) -> String {
    let frame = &model.frames[frame_idx % model.frames.len()];
    let mut out = model.marked.clone();
    for (i, transform) in frame.transforms.iter().enumerate() {
        let open = format!("__PEEK_ANIM_OPEN_{i}__");
        let close = format!("__PEEK_ANIM_CLOSE_{i}__");
        let (open_repl, close_repl) = if transform.is_empty() {
            (String::new(), String::new())
        } else {
            (format!("<g transform=\"{transform}\">"), "</g>".to_string())
        };
        out = out.replace(&open, &open_repl);
        out = out.replace(&close, &close_repl);
    }
    out
}
