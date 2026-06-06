//! Render the EPS / PostScript info section.

use crate::info::{format_size_human, push_field, push_section_header};
use crate::theme::PeekTheme;

use super::PostScriptFormat;
use super::dos_eps::PreviewKind;
use super::info::EpsInfo;

pub fn render_section(lines: &mut Vec<String>, info: &EpsInfo, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, info.format.label(), theme);

    let d = &info.dsc;
    if let Some(v) = &d.title {
        push_field(lines, "Title", &theme.paint_value(v), theme);
    }
    if let Some(v) = &d.creator {
        push_field(lines, "Creator", &theme.paint_value(v), theme);
    }
    if let Some(v) = &d.creation_date {
        push_field(lines, "Created", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &d.for_whom {
        push_field(lines, "For", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &d.bounding_box {
        push_field(lines, "BoundingBox", &theme.paint_value(v), theme);
    }
    if let Some(v) = &d.language_level {
        push_field(lines, "Language", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &d.pages {
        push_field(lines, "Pages", &theme.paint_muted(v), theme);
    }

    match &info.preview {
        Some(p) => {
            let size = format_size_human(p.bytes as u64);
            let desc = match (p.kind, p.dimensions) {
                (PreviewKind::Tiff, Some((w, h))) => format!("TIFF {w}×{h} ({size})"),
                // A TIFF that didn't decode (WMF, or an image-crate-
                // unsupported sub-format) can't drive the Preview tab.
                (PreviewKind::Tiff, None) => format!("TIFF ({size}, not rendered)"),
                (PreviewKind::Wmf, _) => format!("WMF ({size}, not rendered)"),
            };
            push_field(lines, "Preview", &theme.paint_value(&desc), theme);
        }
        None => push_field(lines, "Preview", &theme.paint_muted("none"), theme),
    }

    let render = if info.gs_available {
        theme.paint_value("Ghostscript")
    } else {
        theme.paint_muted("unavailable (install Ghostscript)")
    };
    push_field(lines, "Render", &render, theme);
}

/// Typed `--info --json` encoding of the EPS / PostScript section. The
/// embedded preview (when present) becomes a nested object with raw byte
/// and pixel-dimension numbers; `format` and the preview `kind` are
/// stable lowercase tokens.
pub fn json_section(info: &EpsInfo) -> (&'static str, serde_json::Value) {
    let mut obj = serde_json::json!({
        "format": format_token(info.format),
        "gs_available": info.gs_available,
    });

    let d = &info.dsc;
    if let Some(ref v) = d.title {
        obj["title"] = serde_json::json!(v);
    }
    if let Some(ref v) = d.creator {
        obj["creator"] = serde_json::json!(v);
    }
    if let Some(ref v) = d.creation_date {
        obj["creation_date"] = serde_json::json!(v);
    }
    if let Some(ref v) = d.for_whom {
        obj["for_whom"] = serde_json::json!(v);
    }
    if let Some(ref v) = d.bounding_box {
        obj["bounding_box"] = serde_json::json!(v);
    }
    if let Some(ref v) = d.language_level {
        obj["language_level"] = serde_json::json!(v);
    }
    if let Some(ref v) = d.pages {
        obj["pages"] = serde_json::json!(v);
    }

    if let Some(ref p) = info.preview {
        let mut prev = serde_json::json!({
            "kind": preview_kind_token(p.kind),
            "bytes": p.bytes,
        });
        if let Some((w, h)) = p.dimensions {
            prev["width"] = serde_json::json!(w);
            prev["height"] = serde_json::json!(h);
        }
        obj["preview"] = prev;
    }

    ("eps", obj)
}

fn format_token(format: PostScriptFormat) -> &'static str {
    match format {
        PostScriptFormat::Eps => "eps",
        PostScriptFormat::Ps => "ps",
    }
}

fn preview_kind_token(kind: PreviewKind) -> &'static str {
    match kind {
        PreviewKind::Tiff => "tiff",
        PreviewKind::Wmf => "wmf",
    }
}
