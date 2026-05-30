//! Render the EPS / PostScript info section.

use crate::info::{format_size_human, push_field, push_section_header};
use crate::theme::PeekTheme;

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
