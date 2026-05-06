//! Binary info: friendly format label from a magic-byte MIME, or `None`
//! if the type is genuinely unknown. Render emits a single Format
//! section when a label is available.

use crate::info::{FileExtras, push_field, push_section_header};
use crate::theme::PeekTheme;

pub fn gather_extras(magic_mime: Option<&str>) -> FileExtras {
    FileExtras::Binary {
        format: magic_mime.map(format_label_for_mime),
    }
}

pub fn render_section(lines: &mut Vec<String>, format: Option<&str>, theme: &PeekTheme) {
    if let Some(fmt) = format {
        lines.push(String::new());
        push_section_header(lines, "Format", theme);
        push_field(lines, "Type", &theme.paint_value(fmt), theme);
    }
}

fn format_label_for_mime(mime: &str) -> String {
    match mime {
        "application/zip" => "ZIP archive".to_string(),
        "application/gzip" => "gzip".to_string(),
        "application/x-tar" => "tar archive".to_string(),
        "application/x-bzip2" => "bzip2".to_string(),
        "application/x-7z-compressed" => "7z archive".to_string(),
        "application/x-xz" => "xz".to_string(),
        "application/x-rar-compressed" => "RAR archive".to_string(),
        "application/x-executable" => "executable".to_string(),
        "application/x-mach-binary" => "Mach-O binary".to_string(),
        "application/x-msdownload" => "PE executable".to_string(),
        "application/vnd.sqlite3" | "application/x-sqlite3" => "SQLite database".to_string(),
        "application/pdf" => "PDF document".to_string(),
        m if m.starts_with("video/") => format!("video ({m})"),
        m if m.starts_with("audio/") => format!("audio ({m})"),
        m => m.to_string(),
    }
}
