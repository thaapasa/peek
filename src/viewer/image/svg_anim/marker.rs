//! Per-target marker injection + per-frame substitution. The marked
//! string is built once at parse time; [`render_frame`] does the cheap
//! per-frame substitution.
//!
//! Layout per animated element:
//! - Outer wrapper `<g __PEEK_ANIM_TR_<i>__>...</g>` carries the
//!   per-frame transform (if any). Empty placeholder = no transform.
//! - The element's original opening tag is rewritten in place: each
//!   animated CSS property's attribute is excised and one
//!   `__PEEK_ANIM_S_<i>_<j>__` slot placeholder is inserted before
//!   the closing `>` (or `/>`). [`render_frame`] substitutes the slot
//!   with ` name="value"` (or empty for "no attribute").
//!
//! Direct attribute substitution (rather than CSS-rule injection) is
//! required because usvg 0.47 does not honor `<style>` rules for SVG
//! geometric properties like `r`, `cx`, `cy`. See
//! `property_animation_resolves_and_renders` for the regression check.

use super::{AnimatedSvg, ResolvedTarget};

/// Build the marked SVG. Replaces each target's original opening tag
/// with a slotted version and wraps the whole element in an outer
/// `<g>` carrying the transform placeholder.
///
/// Edits can collide at byte boundaries — for adjacent self-closing
/// elements without whitespace between them (`<circle/><circle/>`),
/// the previous element's `close_at` and the next's `open_at` are the
/// same byte. Iterating `replace_range` in any order ends up
/// clobbering one of the edits because indices into the *current*
/// buffer drift after each mutation. Instead we sort all edits
/// ascending and build the output in a single forward pass: the
/// natural order of zero-length inserts at the same byte (close of N,
/// then open of N+1, then patched-open of N+1) falls out for free
/// from the stable push order below.
pub(super) fn build_marked_svg(text: &str, targets: &[ResolvedTarget]) -> String {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (i, t) in targets.iter().enumerate() {
        let original_open = &text[t.open_at..t.open_tag_end];
        let patched_open = patch_opening_tag(original_open, i, &t.animated_props);

        // Push order at same start byte must be: wrap-open (insert) →
        // patched-open (replace) so the final output reads
        // `<g …><circle …/></g>` not `<circle …/><g …></g>`.
        edits.push((t.open_at, t.open_at, format!("<g __PEEK_ANIM_TR_{i}__>")));
        edits.push((t.open_at, t.open_tag_end, patched_open));
        edits.push((t.close_at, t.close_at, "</g>".to_string()));
    }
    edits.sort_by_key(|(s, _, _)| *s);

    let mut out = String::with_capacity(text.len() + edits.len() * 32);
    let mut cursor = 0;
    for (start, end, replacement) in edits {
        // Copy any source between the previous edit and this one.
        if cursor < start {
            out.push_str(&text[cursor..start]);
        }
        out.push_str(&replacement);
        if end > cursor {
            cursor = end;
        }
    }
    if cursor < text.len() {
        out.push_str(&text[cursor..]);
    }
    out
}

/// Excise each animated attribute from `original_open` and append slot
/// placeholders just before the closing `>`/`/>`. Whitespace gymnastics
/// keep the resulting tag well-formed.
fn patch_opening_tag(original: &str, target_idx: usize, animated_props: &[String]) -> String {
    let mut out = original.to_string();
    // Remove each animated attribute from the tag (descending so byte
    // positions remain valid).
    let mut removals: Vec<(usize, usize)> = animated_props
        .iter()
        .filter_map(|name| find_attr_span(&out, name))
        .collect();
    removals.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    for (s, e) in removals {
        out.replace_range(s..e, "");
    }

    // Insert slot placeholders just before the closing `>` / `/>`.
    let close_pos = closing_bracket(&out);
    let mut slots = String::new();
    for (j, _) in animated_props.iter().enumerate() {
        slots.push_str(&format!("__PEEK_ANIM_S_{target_idx}_{j}__"));
    }
    out.insert_str(close_pos, &slots);
    out
}

/// Find the byte position of the `>` (or the `/` of a `/>`) that
/// closes the opening tag. Returns the index *of* that byte (so
/// inserting before it places content right before the close).
fn closing_bracket(tag: &str) -> usize {
    let bytes = tag.as_bytes();
    // Walk from the end skipping whitespace, then back up over `/>` or `>`.
    let mut i = bytes.len();
    while i > 0 && (bytes[i - 1] as char).is_whitespace() {
        i -= 1;
    }
    if i >= 1 && bytes[i - 1] == b'>' {
        if i >= 2 && bytes[i - 2] == b'/' {
            return i - 2;
        }
        return i - 1;
    }
    bytes.len()
}

/// Locate `name="value"` (or `name='value'`) inside an opening tag.
/// Returns the span covering leading whitespace + name + `=` + quoted
/// value, suitable for full removal.
fn find_attr_span(tag: &str, name: &str) -> Option<(usize, usize)> {
    let bytes = tag.as_bytes();
    let mut i = 0;
    if !bytes.is_empty() && bytes[0] == b'<' {
        i = 1;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if c.is_whitespace() || c == '/' || c == '>' {
                break;
            }
            i += 1;
        }
    }
    while i < bytes.len() {
        let ws_start = i;
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let c = bytes[i] as char;
        if c == '/' || c == '>' {
            return None;
        }
        let name_start = i;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if c == '=' || c.is_whitespace() {
                break;
            }
            i += 1;
        }
        let attr_name = &tag[name_start..i];
        // Skip ws and the `=`.
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            return None;
        }
        i += 1;
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let q = bytes[i];
        if q != b'"' && q != b'\'' {
            return None;
        }
        i += 1;
        let val_start = i;
        while i < bytes.len() && bytes[i] != q {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let val_end = i;
        i += 1;
        if attr_name.eq_ignore_ascii_case(name) {
            return Some((ws_start, i));
        }
        let _ = val_start;
        let _ = val_end;
    }
    None
}

/// Public probe used at parse time to grab the original attribute value
/// before the marker phase rewrites the opening tag.
pub(super) fn find_attr_value(tag: &str, name: &str) -> Option<String> {
    let span = find_attr_span(tag, name)?;
    // Re-parse to lift just the value. Cheap; the tag is short.
    let bytes = tag.as_bytes();
    let mut i = span.0;
    while i < span.1 && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    while i < span.1 && bytes[i] != b'=' {
        i += 1;
    }
    if i >= span.1 {
        return None;
    }
    i += 1;
    while i < span.1 && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    if i >= span.1 {
        return None;
    }
    let q = bytes[i];
    if q != b'"' && q != b'\'' {
        return None;
    }
    i += 1;
    let val_start = i;
    while i < span.1 && bytes[i] != q {
        i += 1;
    }
    Some(tag[val_start..i].to_string())
}

/// Substitute every per-frame placeholder. Result is a complete SVG
/// document ready to feed to resvg.
pub fn render_frame(model: &AnimatedSvg, frame_idx: usize) -> String {
    let frame = &model.frames[frame_idx % model.frames.len()];
    let mut out = model.marked.clone();
    for (i, target) in frame.targets.iter().enumerate() {
        let tr_placeholder = format!("__PEEK_ANIM_TR_{i}__");
        let tr_replacement = if target.transform.is_empty() {
            String::new()
        } else {
            format!(" transform=\"{}\"", target.transform)
        };
        out = out.replace(&tr_placeholder, &tr_replacement);

        for (j, slot) in target.slots.iter().enumerate() {
            let placeholder = format!("__PEEK_ANIM_S_{i}_{j}__");
            out = out.replace(&placeholder, slot);
        }
    }
    out
}
