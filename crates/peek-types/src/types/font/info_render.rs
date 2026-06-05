//! Themed render for a `FontInfo`. Family / subfamily / weight /
//! glyph count / script coverage. Face 0 always emits its full block;
//! collection faces beyond 0 surface as a separate header in Phase 3.

use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;
use crate::types::font::info::{FaceInfo, FontInfo};

pub fn render_section(lines: &mut Vec<String>, info: &FontInfo, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Font", theme);
    push_field(
        lines,
        "Format",
        &theme.paint_value(info.format.label()),
        theme,
    );
    if info.face_count > 1 {
        push_field(
            lines,
            "Faces",
            &paint_count(info.face_count as usize, theme),
            theme,
        );
    }

    for face in &info.faces {
        lines.push(String::new());
        let title = face_title(face, info.face_count);
        lines.push(format!(
            "{} {}",
            theme.paint_muted("\u{2500}\u{2500}"),
            theme.paint_heading(&title),
        ));
        render_face(lines, face, theme);
    }

    for err in &info.parse_errors {
        lines.push(String::new());
        push_field(
            lines,
            "Parse error",
            &theme.paint(err, theme.warning),
            theme,
        );
    }
}

fn face_title(face: &FaceInfo, face_count: u32) -> String {
    let suffix = if face_count > 1 {
        format!(" (face {})", face.index)
    } else {
        String::new()
    };
    let base = if !face.full_name.is_empty() {
        face.full_name.clone()
    } else if !face.family.is_empty() {
        if face.subfamily.is_empty() {
            face.family.clone()
        } else {
            format!("{} {}", face.family, face.subfamily)
        }
    } else if !face.postscript_name.is_empty() {
        face.postscript_name.clone()
    } else {
        "Face".to_string()
    };
    format!("{base}{suffix}")
}

fn render_face(lines: &mut Vec<String>, face: &FaceInfo, theme: &PeekTheme) {
    push_named(lines, "Family", &face.family, theme);
    push_named(lines, "Subfamily", &face.subfamily, theme);
    push_named(lines, "Postscript", &face.postscript_name, theme);
    push_named(lines, "Version", &face.version, theme);
    push_field(lines, "Weight", &paint_weight(face.weight, theme), theme);
    push_field(
        lines,
        "Width",
        &theme.paint_value(width_label(face.width)),
        theme,
    );
    if face.italic {
        push_field(lines, "Style", &theme.paint_value("Italic"), theme);
    }
    if face.monospaced {
        push_field(lines, "Pitch", &theme.paint_value("Monospaced"), theme);
    }
    push_field(
        lines,
        "Glyphs",
        &paint_count(face.glyph_count as usize, theme),
        theme,
    );
    if face.units_per_em > 0 {
        push_field(
            lines,
            "Units / em",
            &paint_count(face.units_per_em as usize, theme),
            theme,
        );
    }
    if face.codepoint_count > 0 {
        push_field(
            lines,
            "Codepoints",
            &paint_count(face.codepoint_count as usize, theme),
            theme,
        );
    }
    if !face.scripts.is_empty() {
        push_field(
            lines,
            "Scripts",
            &theme.paint_value(&face.scripts.join(", ")),
            theme,
        );
    }
    if face.hinting_present {
        push_field(lines, "Hinting", &theme.paint_value("present"), theme);
    }
    push_named(lines, "Designer", &face.designer, theme);
    push_named(lines, "Vendor", &face.vendor, theme);
    push_named(lines, "Copyright", &face.copyright, theme);
    push_named(lines, "License", &face.license_url, theme);
}

fn push_named(lines: &mut Vec<String>, label: &str, value: &str, theme: &PeekTheme) {
    if value.is_empty() {
        return;
    }
    push_field(lines, label, &theme.paint_value(value), theme);
}

/// Format a numeric weight as `<class> (<name>)` when it matches a
/// canonical OS/2 class, or bare number otherwise.
fn paint_weight(weight: u16, theme: &PeekTheme) -> String {
    let name = match weight {
        100 => Some("Thin"),
        200 => Some("ExtraLight"),
        300 => Some("Light"),
        400 => Some("Regular"),
        500 => Some("Medium"),
        600 => Some("SemiBold"),
        700 => Some("Bold"),
        800 => Some("ExtraBold"),
        900 => Some("Black"),
        _ => None,
    };
    let text = match name {
        Some(label) => format!("{weight} ({label})"),
        None => weight.to_string(),
    };
    theme.paint_value(&text)
}

fn width_label(width: u16) -> &'static str {
    match width {
        1 => "Ultra-condensed",
        2 => "Extra-condensed",
        3 => "Condensed",
        4 => "Semi-condensed",
        5 => "Normal",
        6 => "Semi-expanded",
        7 => "Expanded",
        8 => "Extra-expanded",
        9 => "Ultra-expanded",
        _ => "—",
    }
}
