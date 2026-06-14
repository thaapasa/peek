//! Machine-readable JSON encoding of [`FileInfo`] — the second output form
//! of the info layer, parallel to the themed terminal [`render`](super::render).
//!
//! Drives `peek --info --json`, designed for shell pipelines
//! (`peek x --info --json | jq .size_bytes`). The encoding is a single JSON
//! object: the core file metadata is fully typed (numbers stay numbers,
//! timestamps are ISO-8601 UTC strings, so `jq` can filter and compare them),
//! and each file type contributes a typed object nested under its own key
//! (`"pdf"`, `"archive"`, …) via [`InfoExtras::json_section`].
//!
//! A type that hasn't implemented `json_section` falls back to a `details`
//! array of the rendered section's plain-text lines — the honest "human
//! section, verbatim" form, not a structured payload. Every shipping type
//! now provides a typed encoder, so `details` is effectively a safety net
//! rather than a normal output.

use serde_json::{Map, Value, json};

use super::time::format_time;
use super::{CompressionInfo, FileInfo};
use crate::input::mime::{MimeCategory, MimeInfo};
use crate::theme::{PeekTheme, StyleMode};

/// Encode `info` as a single JSON object. `theme` is used only to render the
/// per-type `details` section; the encoder forces it to [`StyleMode::Plain`]
/// so no SGR escapes leak into the JSON strings.
pub fn to_json(info: &FileInfo, theme: &PeekTheme) -> Value {
    let mut obj = Map::new();
    obj.insert("file_name".into(), json!(info.file_name));
    obj.insert("path".into(), json!(info.path));
    obj.insert("size_bytes".into(), json!(info.size_bytes));
    obj.insert(
        "mimes".into(),
        Value::Array(info.mimes.iter().map(mime_json).collect()),
    );

    if let Some(modified) = info.modified {
        obj.insert("modified".into(), json!(format_time(modified, true)));
    }
    if let Some(created) = info.created {
        obj.insert("created".into(), json!(format_time(created, true)));
    }
    if let Some(ref perms) = info.permissions {
        obj.insert("permissions".into(), json!(perms));
    }
    if let Some(ref comp) = info.compression {
        obj.insert("compression".into(), compression_json(comp));
    }
    if !info.warnings.is_empty() {
        obj.insert("warnings".into(), json!(info.warnings));
    }

    // Converted types provide a typed object nested under their own key;
    // the rest fall back to the rendered section as a `details` text array.
    match info.extras.json_section() {
        Some((key, value)) => {
            obj.insert(key.into(), value);
        }
        None => {
            let details = extras_details(info, theme);
            if !details.is_empty() {
                obj.insert("details".into(), Value::Array(details));
            }
        }
    }

    Value::Object(obj)
}

fn mime_json(m: &MimeInfo) -> Value {
    json!({ "mime": m.mime, "category": category_label(m.category) })
}

/// Lowercase machine label for a MIME standardness category. Distinct from
/// [`MimeCategory::marker`], which yields the parenthesised display marker.
fn category_label(cat: MimeCategory) -> &'static str {
    match cat {
        MimeCategory::Registered => "registered",
        MimeCategory::Vendor => "vendor",
        MimeCategory::Convention => "convention",
        MimeCategory::Personal => "personal",
        MimeCategory::Experimental => "experimental",
    }
}

fn compression_json(comp: &CompressionInfo) -> Value {
    let mut obj = json!({
        "codec": comp.codec_label,
        "compressed_size": comp.compressed_size,
        "decompressed_size": comp.decompressed_size,
        // One decimal to match the print path's `{:.1}x`.
        "ratio": (comp.ratio() * 10.0).round() / 10.0,
        "outer_name": comp.outer_name,
    });
    if let Some(ref err) = comp.error {
        obj["error"] = json!(err);
    }
    obj
}

/// Render the per-type extras section with color suppressed and hand the
/// resulting lines back as JSON strings. Section-header rule lines are
/// reduced to their bare title; field lines are trimmed of the leading
/// indent. Blank spacer lines are dropped.
fn extras_details(info: &FileInfo, theme: &PeekTheme) -> Vec<Value> {
    let mut plain = theme.clone();
    plain.style_mode = StyleMode::Plain;

    let mut lines = Vec::new();
    info.extras.render_section(&mut lines, &plain);

    lines
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            // Section headers render as `── Title ───────`; keep just the
            // title so the array reads cleanly.
            let cleaned = trimmed.trim_matches(|c| c == '\u{2500}' || c == ' ');
            Some(json!(cleaned))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::info::{FileInfo, InfoExtras, push_field, push_section_header};
    use crate::theme::{PeekTheme, PeekThemeName, load_embedded_theme};

    fn theme() -> PeekTheme {
        PeekTheme::from_syntect(&load_embedded_theme(
            PeekThemeName::default().tmtheme_source(),
        ))
    }

    /// Extras stub mirroring a real per-type section: a header plus two
    /// fields, painted through the theme like production code.
    struct DemoExtras;
    impl InfoExtras for DemoExtras {
        fn render_section(&self, lines: &mut Vec<String>, theme: &PeekTheme) {
            lines.push(String::new());
            push_section_header(lines, "Demo", theme);
            push_field(lines, "Kind", &theme.paint_value("widget"), theme);
            push_field(lines, "Count", &theme.paint_value("3"), theme);
        }
    }

    fn base_info() -> FileInfo {
        FileInfo {
            file_name: "report.pdf".into(),
            path: "/tmp/report.pdf".into(),
            size_bytes: 1234,
            mimes: vec![MimeInfo::new("application/pdf")],
            warnings: Vec::new(),
            // 2025-01-15T14:30:00Z
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_736_951_400)),
            created: None,
            permissions: Some("-rw-r--r--".into()),
            compression: None,
            extras: Box::new(DemoExtras),
        }
    }

    #[test]
    fn core_fields_are_typed() {
        let v = to_json(&base_info(), &theme());
        assert_eq!(v["file_name"], json!("report.pdf"));
        assert_eq!(v["path"], json!("/tmp/report.pdf"));
        // Size stays a JSON number so `jq` can compare it.
        assert_eq!(v["size_bytes"], json!(1234));
        assert!(v["size_bytes"].is_number());
        // Timestamp is ISO-8601 UTC regardless of the local-time setting.
        assert_eq!(v["modified"], json!("2025-01-15T14:30:00Z"));
        assert_eq!(v["permissions"], json!("-rw-r--r--"));
        assert_eq!(v["mimes"][0]["mime"], json!("application/pdf"));
        assert_eq!(v["mimes"][0]["category"], json!("registered"));
    }

    #[test]
    fn absent_optionals_are_omitted() {
        let v = to_json(&base_info(), &theme());
        assert!(v.get("created").is_none());
        assert!(v.get("compression").is_none());
        assert!(v.get("warnings").is_none());
    }

    #[test]
    fn details_carry_plain_extras_lines_without_escapes() {
        let v = to_json(&base_info(), &theme());
        let details = v["details"].as_array().expect("details array");
        // Header reduced to its title, fields kept; no SGR escapes leak.
        assert_eq!(details[0], json!("Demo"));
        assert!(
            details
                .iter()
                .any(|d| d.as_str().unwrap().starts_with("Kind"))
        );
        assert!(
            details
                .iter()
                .all(|d| !d.as_str().unwrap().contains('\u{1b}')),
            "details must be plain text"
        );
    }
}
