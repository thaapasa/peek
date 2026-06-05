//! PostScript-family format: Encapsulated PostScript (`.eps`) vs plain
//! PostScript (`.ps`). The two share the whole viewer (embedded preview,
//! Ghostscript render, source, DSC info); the format only labels the
//! Info section and decides whether Ghostscript renders with `-dEPSCrop`
//! (crop to the EPS BoundingBox) or the full default page.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostScriptFormat {
    /// Encapsulated PostScript — single illustration with a
    /// `%%BoundingBox`, optionally a binary preview header.
    Eps,
    /// Plain PostScript program / document.
    Ps,
}

impl PostScriptFormat {
    /// Human label for the Info section header.
    pub fn label(self) -> &'static str {
        match self {
            PostScriptFormat::Eps => "EPS",
            PostScriptFormat::Ps => "PostScript",
        }
    }

    /// Whether Ghostscript should crop to the EPS BoundingBox.
    pub fn crop_to_bbox(self) -> bool {
        matches!(self, PostScriptFormat::Eps)
    }
}

/// Binary DOS-EPS container magic (`C5 D0 D3 C6`). Detection only needs
/// the prefix check; the full header parse (preview extraction) lives in
/// the eps reader's `dos_eps` module.
const DOS_EPS_MAGIC: [u8; 4] = [0xC5, 0xD0, 0xD3, 0xC6];

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
    head.len() >= DOS_EPS_MAGIC.len() && head[..DOS_EPS_MAGIC.len()] == DOS_EPS_MAGIC
}
