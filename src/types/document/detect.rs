//! Extension- and MIME-based word-processing document format detection.

use super::format::DocumentFormat;

/// Map a single file extension to a word-processing document format.
pub fn format_from_ext(ext: &str) -> Option<DocumentFormat> {
    match ext {
        "docx" => Some(DocumentFormat::Docx),
        "odt" => Some(DocumentFormat::Odt),
        "rtf" => Some(DocumentFormat::Rtf),
        _ => None,
    }
}

/// Map a magic-byte MIME to a document format. Only RTF has a
/// magic-byte identifier in our setup (`application/rtf`, set from
/// the `{\rtf1` head probe). DOCX/ODT come in as `application/zip`
/// and are routed through extension-based detection or container
/// peeking elsewhere.
pub fn format_from_mime(mime: &str) -> Option<DocumentFormat> {
    match mime {
        "application/rtf" => Some(DocumentFormat::Rtf),
        _ => None,
    }
}
