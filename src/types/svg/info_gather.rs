//! SVG-specific extras (alongside source-text stats from `info::gather::text`).
//! Substring-based extraction — quick_xml would be stricter than necessary
//! for what amounts to "is the script tag here".

use crate::info::{FileExtras, SvgAnimationStats, TextStats};
use crate::types::image::pipeline::svg_anim::{self, ParseOutcome};

pub fn gather_extras(text: TextStats, bytes: &[u8]) -> FileExtras {
    let s = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            return FileExtras::Svg {
                text,
                view_box: None,
                declared_width: None,
                declared_height: None,
                path_count: 0,
                group_count: 0,
                rect_count: 0,
                circle_count: 0,
                text_count: 0,
                has_script: false,
                has_external_href: false,
                animation: None,
                animation_warning: None,
            };
        }
    };

    let view_box = root_attr(s, "viewBox");
    let declared_width = root_attr(s, "width");
    let declared_height = root_attr(s, "height");
    let path_count = count_open_tag(s, "path");
    let group_count = count_open_tag(s, "g");
    let rect_count = count_open_tag(s, "rect");
    let circle_count = count_open_tag(s, "circle");
    let text_count = count_open_tag(s, "text");
    let has_script = s.contains("<script");
    let has_external_href = has_external_href(s);
    let (animation, animation_warning) = match svg_anim::diagnose_bytes(bytes) {
        ParseOutcome::Animated(m) => (
            Some(SvgAnimationStats {
                frame_count: m.frames.len(),
                total_duration_ms: m.duration.as_millis() as u64,
                infinite: m.infinite,
            }),
            None,
        ),
        ParseOutcome::Unsupported(reason) => (None, Some(reason)),
        ParseOutcome::NotAnimated => (None, None),
    };

    FileExtras::Svg {
        text,
        view_box,
        declared_width,
        declared_height,
        path_count,
        group_count,
        rect_count,
        circle_count,
        text_count,
        has_script,
        has_external_href,
        animation,
        animation_warning,
    }
}

fn root_attr(svg: &str, attr: &str) -> Option<String> {
    let svg_open = svg.find("<svg")?;
    let after = &svg[svg_open..];
    let end = after.find('>')?;
    let header = &after[..end];
    extract_attr(header, attr)
}

fn extract_attr(header: &str, attr: &str) -> Option<String> {
    // Match ` attr=` to avoid e.g. `viewBox` matching `data-viewBox`.
    let needle = format!(" {attr}=");
    let pos = header.find(&needle)?;
    let after = &header[pos + needle.len()..];
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let body = &after[1..];
    let close = body.find(quote)?;
    Some(body[..close].to_string())
}

fn count_open_tag(svg: &str, tag: &str) -> usize {
    let mut count = 0usize;
    let needle = format!("<{tag}");
    let mut cursor = svg;
    while let Some(pos) = cursor.find(&needle) {
        let after = &cursor[pos + needle.len()..];
        // Must be followed by `>`, whitespace, `/`, or end — not by another
        // letter (so `<text` doesn't also match `<textPath`).
        match after.chars().next() {
            Some(c) if c.is_ascii_alphanumeric() || c == '-' || c == '_' => {}
            _ => count += 1,
        }
        cursor = after;
    }
    count
}

fn has_external_href(svg: &str) -> bool {
    for needle in ["xlink:href=", "href="] {
        let mut cursor = svg;
        while let Some(pos) = cursor.find(needle) {
            let after = &cursor[pos + needle.len()..];
            let value = after.trim_start_matches(['"', '\'']);
            if value.starts_with("http://")
                || value.starts_with("https://")
                || value.starts_with("//")
            {
                return true;
            }
            cursor = after;
        }
    }
    false
}
