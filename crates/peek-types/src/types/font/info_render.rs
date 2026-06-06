//! Renders a [`FontInfo`] via a single [`FontView`] driving both the themed
//! terminal output ([`InfoView`](crate::info::InfoView)) and the
//! `--info --json` form (`serde::Serialize`).
//!
//! The per-face headers aren't standard section rules (`── Title` with no
//! trailing dashes), so faces emit as a blank `Line` + a custom header `Line`
//! followed by top-level field rows rather than `Block`s.

use serde::{Serialize, Serializer};

use crate::info::{InfoNode, paint_count, render_info};
use crate::theme::PeekTheme;
use crate::types::font::FontFormat;
use crate::types::font::info::{FaceInfo, FontInfo};

/// Themed terminal Font section.
pub fn render_section(lines: &mut Vec<String>, info: &FontInfo, theme: &PeekTheme) {
    render_info(lines, &FontView(info), theme);
}

/// Typed `--info --json` view of the Font section, nested under `"font"`.
pub fn json_section(info: &FontInfo) -> (&'static str, serde_json::Value) {
    (
        "font",
        serde_json::to_value(FontView(info)).expect("font info view serializes"),
    )
}

struct FontView<'a>(&'a FontInfo);

impl crate::info::InfoView for FontView<'_> {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        let info = self.0;
        let mut nodes = Vec::new();

        let mut head = vec![InfoNode::Row {
            label: "Format".into(),
            value: theme.paint_value(info.format.label()),
        }];
        if info.face_count > 1 {
            head.push(InfoNode::Row {
                label: "Faces".into(),
                value: paint_count(info.face_count as usize, theme),
            });
        }
        nodes.push(InfoNode::Block {
            title: "Font".to_string(),
            body: head,
        });

        for face in &info.faces {
            nodes.push(InfoNode::Line(String::new()));
            let title = face_title(face, info.face_count);
            nodes.push(InfoNode::Line(format!(
                "{} {}",
                theme.paint_muted("\u{2500}\u{2500}"),
                theme.paint_heading(&title),
            )));
            nodes.extend(face_rows(face, theme));
        }

        for err in &info.parse_errors {
            nodes.push(InfoNode::Line(String::new()));
            nodes.push(InfoNode::Row {
                label: "Parse error".into(),
                value: theme.paint(err, theme.warning),
            });
        }
        nodes
    }
}

impl Serialize for FontView<'_> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let info = self.0;
        let mut obj = serde_json::json!({
            "format": format_token(info.format),
            "face_count": info.face_count,
        });
        let faces: Vec<serde_json::Value> = info.faces.iter().map(face_json).collect();
        obj["faces"] = serde_json::json!(faces);
        if !info.parse_errors.is_empty() {
            obj["parse_errors"] = serde_json::json!(info.parse_errors);
        }
        obj.serialize(ser)
    }
}

/// Push a `label  value` row, skipping empty values.
fn named(rows: &mut Vec<InfoNode>, label: &'static str, value: &str, theme: &PeekTheme) {
    if !value.is_empty() {
        rows.push(InfoNode::Row {
            label: label.into(),
            value: theme.paint_value(value),
        });
    }
}

fn face_rows(face: &FaceInfo, theme: &PeekTheme) -> Vec<InfoNode> {
    let mut rows = Vec::new();
    named(&mut rows, "Family", &face.family, theme);
    named(&mut rows, "Subfamily", &face.subfamily, theme);
    named(&mut rows, "Postscript", &face.postscript_name, theme);
    named(&mut rows, "Version", &face.version, theme);
    rows.push(InfoNode::Row {
        label: "Weight".into(),
        value: paint_weight(face.weight, theme),
    });
    rows.push(InfoNode::Row {
        label: "Width".into(),
        value: theme.paint_value(width_label(face.width)),
    });
    if face.italic {
        rows.push(InfoNode::Row {
            label: "Style".into(),
            value: theme.paint_value("Italic"),
        });
    }
    if face.monospaced {
        rows.push(InfoNode::Row {
            label: "Pitch".into(),
            value: theme.paint_value("Monospaced"),
        });
    }
    rows.push(InfoNode::Row {
        label: "Glyphs".into(),
        value: paint_count(face.glyph_count as usize, theme),
    });
    if face.units_per_em > 0 {
        rows.push(InfoNode::Row {
            label: "Units / em".into(),
            value: paint_count(face.units_per_em as usize, theme),
        });
    }
    if face.codepoint_count > 0 {
        rows.push(InfoNode::Row {
            label: "Codepoints".into(),
            value: paint_count(face.codepoint_count as usize, theme),
        });
    }
    if !face.scripts.is_empty() {
        rows.push(InfoNode::Row {
            label: "Scripts".into(),
            value: theme.paint_value(&face.scripts.join(", ")),
        });
    }
    if face.hinting_present {
        rows.push(InfoNode::Row {
            label: "Hinting".into(),
            value: theme.paint_value("present"),
        });
    }
    named(&mut rows, "Designer", &face.designer, theme);
    named(&mut rows, "Vendor", &face.vendor, theme);
    named(&mut rows, "Copyright", &face.copyright, theme);
    named(&mut rows, "License", &face.license_url, theme);
    rows
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

/// Format a numeric weight as `<class> (<name>)` when it matches a canonical
/// OS/2 class, or bare number otherwise.
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

fn format_token(format: FontFormat) -> &'static str {
    match format {
        FontFormat::TrueType => "truetype",
        FontFormat::OpenType => "opentype",
        FontFormat::Collection => "collection",
        FontFormat::Woff => "woff",
        FontFormat::Woff2 => "woff2",
    }
}

fn face_json(face: &FaceInfo) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "index": face.index,
        "weight": face.weight,
        "width": face.width,
        "italic": face.italic,
        "monospaced": face.monospaced,
        "hinting_present": face.hinting_present,
        "glyph_count": face.glyph_count,
    });
    let mut put = |key: &str, value: &str| {
        if !value.is_empty() {
            obj[key] = serde_json::json!(value);
        }
    };
    put("family", &face.family);
    put("subfamily", &face.subfamily);
    put("full_name", &face.full_name);
    put("postscript_name", &face.postscript_name);
    put("version", &face.version);
    put("copyright", &face.copyright);
    put("designer", &face.designer);
    put("vendor", &face.vendor);
    put("license_url", &face.license_url);
    if face.units_per_em > 0 {
        obj["units_per_em"] = serde_json::json!(face.units_per_em);
    }
    if face.codepoint_count > 0 {
        obj["codepoint_count"] = serde_json::json!(face.codepoint_count);
    }
    if !face.scripts.is_empty() {
        obj["scripts"] = serde_json::json!(face.scripts);
    }
    obj
}
