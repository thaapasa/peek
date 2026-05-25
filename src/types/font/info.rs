//! Font info shape. One [`FontInfo`] per file plus a list of per-face
//! summaries — a `.ttc` collection carries many faces, plain `.ttf` /
//! `.otf` carry exactly one.

use crate::types::font::format::FontFormat;

pub struct FontInfo {
    pub format: FontFormat,
    /// Number of faces in the file. 1 for plain TrueType / OpenType,
    /// ≥ 1 for collections.
    pub face_count: u32,
    /// One entry per face. Phase 1 surfaces face 0 only — Phase 3
    /// (per-face listing recursion) populates the rest.
    pub faces: Vec<FaceInfo>,
    /// Best-effort parse errors. One per failed face — rendered as a
    /// Warning row so a malformed face doesn't suppress the rest.
    pub parse_errors: Vec<String>,
}

pub struct FaceInfo {
    /// Position of the face in the file. 0 for plain non-collection
    /// fonts and the first face of a collection.
    pub index: u32,
    /// Font family name (from `name` table ID 1).
    pub family: String,
    /// Subfamily — `Regular` / `Bold` / `Italic` / etc. (`name` ID 2).
    pub subfamily: String,
    /// Full name (`name` ID 4). Often `<family> <subfamily>`.
    pub full_name: String,
    /// Postscript name (`name` ID 6). The identifier most renderers
    /// use to address the face.
    pub postscript_name: String,
    /// Version string (`name` ID 5).
    pub version: String,
    pub copyright: String,
    pub designer: String,
    pub vendor: String,
    pub license_url: String,
    /// Units per em from the `head` table — the design grid resolution.
    pub units_per_em: u16,
    pub glyph_count: u16,
    /// True when the font declares itself as monospaced (post.isFixedPitch).
    pub monospaced: bool,
    /// OS/2 weight class (100..900, 400 = regular).
    pub weight: u16,
    /// OS/2 width class (1..9, 5 = normal). Out-of-range values pass
    /// through; the renderer paints them as-is.
    pub width: u16,
    pub italic: bool,
    /// Whether `head.flags` indicates hinting is present (bit 0 of the
    /// `flags` field — set when the font requires baseline-to-pixel
    /// rounding).
    pub hinting_present: bool,
    /// Approximate Unicode codepoint coverage from the cmap table.
    /// Counted by walking every code-point range in every cmap subtable
    /// the font exposes.
    pub codepoint_count: u32,
    /// Scripts the font carries glyphs for, inferred from cmap
    /// coverage and OS/2 unicode-range bits. Names match Unicode block
    /// labels (`Latin`, `Cyrillic`, `Greek`, `CJK`, …).
    pub scripts: Vec<String>,
}
