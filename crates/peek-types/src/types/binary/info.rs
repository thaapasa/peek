//! Binary info: friendly format label from a magic-byte MIME, or `None`
//! if the type is genuinely unknown. The Format section vanishes when no
//! label is available (its only field is absent).

use crate::info::Extras;

/// Format section view — drives both `--info` print and `--info --json`.
#[derive(serde::Serialize, crate::info::InfoView)]
#[info(title = "Format")]
pub struct BinaryInfo {
    #[info(label = "Type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

pub fn gather_extras(magic_mime: Option<&str>) -> Extras {
    Box::new(BinaryInfo {
        format: magic_mime.map(format_label_for_mime),
    })
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
