//! EPS / PostScript detection: extension, MIME, and content sniff.

use super::dos_eps;
use super::format::PostScriptFormat;

/// Map a lowercase extension to a PostScript format.
pub fn format_from_ext(ext: &str) -> Option<PostScriptFormat> {
    match ext {
        "eps" | "epsf" | "epsi" => Some(PostScriptFormat::Eps),
        "ps" => Some(PostScriptFormat::Ps),
        _ => None,
    }
}

/// Map a magic-byte / IANA MIME to a PostScript format. Binary DOS-EPS
/// is always EPS; the generic `application/postscript` defaults to EPS
/// too (the common magic-only case is an illustration).
pub fn format_from_mime(mime: &str) -> Option<PostScriptFormat> {
    match mime {
        "image/x-eps" | "application/eps" => Some(PostScriptFormat::Eps),
        "application/postscript" => Some(PostScriptFormat::Eps),
        _ => None,
    }
}

/// Sniff a text head for a PostScript program. `%!PS-Adobe-…EPSF-…`
/// marks Encapsulated PostScript; a bare `%!` / `%!PS` is a plain
/// PostScript program.
pub fn sniff_text(text: &str) -> Option<PostScriptFormat> {
    let head = text.trim_start();
    if !head.starts_with("%!") {
        return None;
    }
    // The DSC version line is the first line; EPS announces itself with
    // an `EPSF` conformance token there.
    let first_line = head.lines().next().unwrap_or(head);
    if first_line.contains("EPSF") {
        Some(PostScriptFormat::Eps)
    } else {
        Some(PostScriptFormat::Ps)
    }
}

/// Whether a file head is a binary DOS-EPS container (always EPS).
pub fn is_dos_eps(head: &[u8]) -> bool {
    head.len() >= dos_eps::MAGIC.len() && head[..dos_eps::MAGIC.len()] == dos_eps::MAGIC
}
