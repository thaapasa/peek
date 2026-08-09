//! The Font section. Each face is an irregular block — `── Title` headers with
//! no trailing dashes, and rows whose print and JSON forms diverge (a
//! `Weight` label vs the raw OS/2 number, a `Style`/`Pitch`/`Hinting` row shown
//! only when set vs an always-present bool). The `#[derive(InfoView)]` can't
//! express that, so each face builds one [`InfoRow`] list that drives *both*
//! outputs — [`push_rows`] for the themed lines, [`rows_to_json`] for the
//! object. The section frame (format / face_count / faces array) stays manual.

use peek_theme::PeekTheme;
use serde_json::json;

use crate::info::{
    InfoNode, InfoRow, Role, Value, paint_count, push_entry, push_parse_errors, render_info,
    rows_to_json,
};
use crate::types::font::FontFormat;
use crate::types::font::info::{FaceInfo, FontInfo};

/// Themed terminal Font section.
pub fn render_section(lines: &mut Vec<String>, info: &FontInfo, theme: &PeekTheme) {
    render_info(lines, &FontView(info), theme);
}

/// Typed `--info --json` view of the Font section, nested under `"font"`.
pub fn json_section(info: &FontInfo) -> (&'static str, serde_json::Value) {
    let faces: Vec<serde_json::Value> = info
        .faces
        .iter()
        .map(|f| serde_json::Value::Object(rows_to_json(&face_rows(f))))
        .collect();
    let mut obj = serde_json::Map::new();
    obj.insert("format".into(), json!(format_token(info.format)));
    obj.insert("face_count".into(), json!(info.face_count));
    obj.insert("faces".into(), serde_json::Value::Array(faces));
    if !info.parse_errors.is_empty() {
        obj.insert("parse_errors".into(), json!(info.parse_errors));
    }
    ("font", serde_json::Value::Object(obj))
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
            // One row list drives the print body here and the JSON in
            // `json_section`.
            let title = face_title(face, info.face_count);
            push_entry(&mut nodes, theme, &title, &face_rows(face));
        }

        push_parse_errors(&mut nodes, theme, &info.parse_errors);
        nodes
    }
}

/// The one row list per face — feeding both outputs.
fn face_rows(face: &FaceInfo) -> Vec<InfoRow> {
    let mut r = Vec::new();
    // `index` and `full_name` appear in the title / JSON only, not as print rows.
    r.push(InfoRow::json_int("index", face.index as i64));
    named(&mut r, "Family", "family", &face.family);
    json_text(&mut r, "full_name", &face.full_name);
    named(&mut r, "Subfamily", "subfamily", &face.subfamily);
    named(
        &mut r,
        "Postscript",
        "postscript_name",
        &face.postscript_name,
    );
    named(&mut r, "Version", "version", &face.version);
    // Print the labelled weight/width; JSON keeps the raw OS/2 numbers.
    r.push(InfoRow::new(
        "Weight",
        "weight",
        Value::split(weight_text(face.weight), Role::Value, json!(face.weight)),
    ));
    r.push(InfoRow::new(
        "Width",
        "width",
        Value::split(width_label(face.width), Role::Value, json!(face.width)),
    ));
    // JSON keeps the bool always; print shows the row only when set.
    r.push(InfoRow::json_bool("italic", face.italic));
    if face.italic {
        r.push(InfoRow::print_only("Style", Value::text("Italic")));
    }
    r.push(InfoRow::json_bool("monospaced", face.monospaced));
    if face.monospaced {
        r.push(InfoRow::print_only("Pitch", Value::text("Monospaced")));
    }
    r.push(InfoRow::count(
        "Glyphs",
        "glyph_count",
        face.glyph_count as u64,
    ));
    if face.units_per_em > 0 {
        r.push(InfoRow::count(
            "Units / em",
            "units_per_em",
            face.units_per_em as u64,
        ));
    }
    if face.codepoint_count > 0 {
        r.push(InfoRow::count(
            "Codepoints",
            "codepoint_count",
            face.codepoint_count as u64,
        ));
    }
    if !face.scripts.is_empty() {
        r.push(InfoRow::new(
            "Scripts",
            "scripts",
            Value::split(face.scripts.join(", "), Role::Value, json!(face.scripts)),
        ));
    }
    r.push(InfoRow::json_bool("hinting_present", face.hinting_present));
    if face.hinting_present {
        r.push(InfoRow::print_only("Hinting", Value::text("present")));
    }
    named(&mut r, "Designer", "designer", &face.designer);
    named(&mut r, "Vendor", "vendor", &face.vendor);
    named(&mut r, "Copyright", "copyright", &face.copyright);
    named(&mut r, "License", "license_url", &face.license_url);
    r
}

/// A text field present in both outputs, skipped from both when empty.
fn named(rows: &mut Vec<InfoRow>, label: &'static str, key: &'static str, value: &str) {
    if !value.is_empty() {
        rows.push(InfoRow::new(label, key, Value::text(value)));
    }
}

/// A JSON-only text field, omitted when empty.
fn json_text(rows: &mut Vec<InfoRow>, key: &'static str, value: &str) {
    if !value.is_empty() {
        rows.push(InfoRow::json_only(key, Value::text(value)));
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

/// A numeric weight as `<class> (<name>)` when it matches a canonical OS/2
/// class, or the bare number otherwise.
fn weight_text(weight: u16) -> String {
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
    match name {
        Some(label) => format!("{weight} ({label})"),
        None => weight.to_string(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_face() -> FaceInfo {
        FaceInfo {
            index: 0,
            family: "Inter".to_string(),
            subfamily: "Regular".to_string(),
            full_name: "Inter Regular".to_string(),
            postscript_name: "Inter-Regular".to_string(),
            version: "1.0".to_string(),
            copyright: String::new(),
            designer: String::new(),
            vendor: String::new(),
            license_url: String::new(),
            units_per_em: 2048,
            glyph_count: 1234,
            monospaced: false,
            weight: 400,
            width: 5,
            italic: true,
            hinting_present: true,
            codepoint_count: 2500,
            scripts: vec!["Latin".to_string(), "Greek".to_string()],
        }
    }

    /// One face's JSON object: the labelled weight/width serialize as the raw
    /// OS/2 numbers, the always-present bools carry their JSON keys, and the
    /// human-only print rows (`Style`, `Pitch`, `Hinting`) stay out of JSON.
    #[test]
    fn face_json_shape() {
        let obj = serde_json::Value::Object(rows_to_json(&face_rows(&sample_face())));

        assert_eq!(obj["index"], json!(0));
        assert_eq!(obj["family"], json!("Inter"));
        assert_eq!(obj["full_name"], json!("Inter Regular"));
        // Weight/width serialize as the raw numbers, not `400 (Regular)`.
        assert_eq!(obj["weight"], json!(400));
        assert_eq!(obj["width"], json!(5));
        // Bools always keyed; their print rows are JSON-keyless.
        assert_eq!(obj["italic"], json!(true));
        assert_eq!(obj["monospaced"], json!(false));
        assert_eq!(obj["hinting_present"], json!(true));
        assert!(obj.get("Style").is_none(), "print label leaked: {obj}");
        assert!(obj.get("Pitch").is_none(), "print label leaked: {obj}");
        assert!(obj.get("Hinting").is_none(), "print label leaked: {obj}");
        assert_eq!(obj["glyph_count"], json!(1234));
        assert_eq!(obj["scripts"], json!(["Latin", "Greek"]));
    }

    /// The section frame: `font` key, `format` token, `face_count`, the faces
    /// array, and `parse_errors` only when non-empty.
    #[test]
    fn section_frame() {
        let info = FontInfo {
            format: FontFormat::OpenType,
            face_count: 1,
            faces: vec![sample_face()],
            parse_errors: Vec::new(),
        };
        let (key, value) = json_section(&info);
        assert_eq!(key, "font");
        assert_eq!(value["format"], json!("opentype"));
        assert_eq!(value["face_count"], json!(1));
        assert_eq!(value["faces"].as_array().unwrap().len(), 1);
        assert!(value.get("parse_errors").is_none());
    }
}
