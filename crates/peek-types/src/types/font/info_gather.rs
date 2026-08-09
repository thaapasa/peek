//! Decode font metadata into a [`FontInfo`]. Pure ttf-parser walk —
//! no rasterization, no glyph loading. Phase 1 surfaces face 0 of a
//! collection only; per-face listing recursion is the Phase 3 path.

use peek_io::InputSource;
use ttf_parser::{Face, fonts_in_collection};

use crate::info::Extras;
use crate::types::font::FontFormat;
use crate::types::font::info::{FaceInfo, FontInfo};

/// Cap on bytes read for font parsing. The largest fonts in the wild —
/// Noto CJK supersets, Apple's San Francisco collection — sit around
/// 30–50 MB; 256 MB leaves comfortable headroom for the worst case
/// without putting an absurd buffer at the mercy of a hostile input.
const FONT_BYTE_LIMIT: u64 = 256 * 1024 * 1024;

/// Collect the font Info sidecar: unwrap a WOFF wrapper to its inner
/// sfnt (bare sfnt borrows through), then parse every face. Capped at
/// [`FONT_BYTE_LIMIT`]; an over-cap or malformed source falls back to
/// the generic binary view.
pub fn gather_extras(source: &InputSource, fmt: FontFormat, magic_mime: Option<&str>) -> Extras {
    if let Ok(bs) = source.open_byte_source()
        && bs.len() > FONT_BYTE_LIMIT
    {
        return crate::types::binary::info::gather_extras(magic_mime);
    }
    let Ok(bytes) = source.read_bytes(peek_io::limits::Budget::Unbounded(
        "gated by FONT_BYTE_LIMIT above",
    )) else {
        return crate::types::binary::info::gather_extras(magic_mime);
    };
    let Ok(sfnt) = super::sfnt::decode(&bytes, fmt) else {
        return crate::types::binary::info::gather_extras(magic_mime);
    };
    Box::new(gather(&sfnt, fmt))
}

/// Parse `bytes` as a font of the given container `format` and produce
/// a [`FontInfo`]. Every face in a collection is gathered so the Info
/// view can list them all; a malformed face surfaces as a `parse_errors`
/// entry without suppressing the rest.
pub fn gather(bytes: &[u8], format: FontFormat) -> FontInfo {
    let face_count = fonts_in_collection(bytes).unwrap_or(1);

    let mut faces = Vec::new();
    let mut parse_errors = Vec::new();

    for index in 0..face_count {
        match Face::parse(bytes, index) {
            Ok(face) => faces.push(gather_face(index, &face)),
            Err(e) => parse_errors.push(format!("face {index}: {e}")),
        }
    }

    FontInfo {
        format,
        face_count,
        faces,
        parse_errors,
    }
}

/// Returns the embedded face count for a font collection, falling
/// back to 1 for plain single-face containers. Used by the compose
/// path so the specimen mode knows how many faces it can cycle through
/// without re-parsing every face's metadata.
pub fn face_count(bytes: &[u8]) -> u32 {
    fonts_in_collection(bytes).unwrap_or(1)
}

fn gather_face(index: u32, face: &Face<'_>) -> FaceInfo {
    let names = read_name_table(face);
    let glyph_count = face.number_of_glyphs();
    let units_per_em = face.units_per_em();
    let monospaced = face.is_monospaced();
    let italic = face.is_italic();
    let weight = face.weight().to_number();
    let width = face.width().to_number();
    let hinting_present = head_flags(face)
        .map(|flags| flags & 0x0001 != 0)
        .unwrap_or(false);
    let (codepoint_count, scripts) = scan_cmap(face);

    FaceInfo {
        index,
        family: names.family,
        subfamily: names.subfamily,
        full_name: names.full_name,
        postscript_name: names.postscript_name,
        version: names.version,
        copyright: names.copyright,
        designer: names.designer,
        vendor: names.vendor,
        license_url: names.license_url,
        units_per_em,
        glyph_count,
        monospaced,
        weight,
        width,
        italic,
        hinting_present,
        codepoint_count,
        scripts,
    }
}

#[derive(Default)]
struct NameTable {
    family: String,
    subfamily: String,
    full_name: String,
    postscript_name: String,
    version: String,
    copyright: String,
    designer: String,
    vendor: String,
    license_url: String,
}

/// Read the canonical entries from the OpenType `name` table. Each
/// field falls back to the first parseable string when the preferred
/// (Windows Unicode BMP, English-US) record isn't present — fonts in
/// the wild are inconsistent and a missing English entry shouldn't
/// blank the section.
fn read_name_table(face: &Face<'_>) -> NameTable {
    use ttf_parser::PlatformId;

    let mut out = NameTable::default();

    for record in face.names() {
        let value = match decode_name(&record) {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };

        // Score the record: prefer Windows Unicode-BMP English over
        // anything else, fall back to Macintosh Roman, then to whatever
        // we already have.
        let preferred = matches!(record.platform_id, PlatformId::Windows)
            && record.encoding_id == 1
            && record.language_id == 0x0409;

        let slot: &mut String = match record.name_id {
            0 => &mut out.copyright,
            1 => &mut out.family,
            2 => &mut out.subfamily,
            4 => &mut out.full_name,
            5 => &mut out.version,
            6 => &mut out.postscript_name,
            9 => &mut out.designer,
            14 => &mut out.license_url,
            // Vendor URL (ID 11) is more useful than the abbreviated
            // vendor code in OS/2; prefer the named-table form.
            11 => &mut out.vendor,
            _ => continue,
        };

        if slot.is_empty() || preferred {
            *slot = value;
        }
    }

    out
}

/// Decode a `name` record. Windows / Unicode platforms use UTF-16BE;
/// Macintosh platform records use Mac-Roman (most Apple system fonts
/// — Menlo, San Francisco — carry their canonical name records here
/// rather than in the Windows table, so this path can't be skipped).
/// Returns `None` for unsupported platforms or malformed buffers; the
/// caller keeps any earlier-recorded value.
fn decode_name(record: &ttf_parser::name::Name<'_>) -> Option<String> {
    use ttf_parser::PlatformId;

    let bytes = record.name;
    match record.platform_id {
        PlatformId::Unicode | PlatformId::Windows => {
            // UTF-16BE big-endian, even-length payload.
            if !bytes.len().is_multiple_of(2) {
                return None;
            }
            let words: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16(&words).ok()
        }
        PlatformId::Macintosh => Some(
            bytes
                .iter()
                .map(|&b| {
                    if b < 0x80 {
                        b as char
                    } else {
                        MAC_ROMAN_HIGH[(b - 0x80) as usize]
                    }
                })
                .collect(),
        ),
        _ => None,
    }
}

/// Mac Roman upper-half (0x80..=0xFF) → Unicode, per Apple's
/// reference table (`MacRoman.TXT` from the Unicode consortium).
/// Used by [`decode_name`] for the Macintosh platform records that
/// Apple system fonts still ship as their primary metadata.
const MAC_ROMAN_HIGH: [char; 128] = [
    'Ä', 'Å', 'Ç', 'É', 'Ñ', 'Ö', 'Ü', 'á', 'à', 'â', 'ä', 'ã', 'å', 'ç', 'é', 'è', // 0x80
    'ê', 'ë', 'í', 'ì', 'î', 'ï', 'ñ', 'ó', 'ò', 'ô', 'ö', 'õ', 'ú', 'ù', 'û', 'ü', // 0x90
    '†', '°', '¢', '£', '§', '•', '¶', 'ß', '®', '©', '™', '´', '¨', '≠', 'Æ', 'Ø', // 0xA0
    '∞', '±', '≤', '≥', '¥', 'µ', '∂', '∑', '∏', 'π', '∫', 'ª', 'º', 'Ω', 'æ', 'ø', // 0xB0
    '¿', '¡', '¬', '√', 'ƒ', '≈', '∆', '«', '»', '…', '\u{00A0}', 'À', 'Ã', 'Õ', 'Œ',
    'œ', // 0xC0
    '–', '—', '“', '”', '‘', '’', '÷', '◊', 'ÿ', 'Ÿ', '⁄', '€', '‹', '›', 'ﬁ', 'ﬂ', // 0xD0
    '‡', '·', '‚', '„', '‰', 'Â', 'Ê', 'Á', 'Ë', 'È', 'Í', 'Î', 'Ï', 'Ì', 'Ó', 'Ô', // 0xE0
    '\u{F8FF}', 'Ò', 'Ú', 'Û', 'Ù', 'ı', 'ˆ', '˜', '¯', '˘', '˙', '˚', '¸', '˝', '˛',
    'ˇ', // 0xF0
];

/// Read the `head` table's flags field. ttf-parser doesn't expose
/// `head.flags` directly — pull it from the raw face bytes. Returns
/// `None` if the table isn't present (older fonts may lack it,
/// though that's vanishingly rare).
fn head_flags(face: &Face<'_>) -> Option<u16> {
    let raw = face.raw_face();
    let tag = ttf_parser::Tag::from_bytes(b"head");
    let table = raw.table(tag)?;
    // head layout: u32 version, u32 fontRevision, u32 checkSumAdjustment,
    // u32 magicNumber (offset 12), u16 flags (offset 16).
    if table.len() < 18 {
        return None;
    }
    Some(u16::from_be_bytes([table[16], table[17]]))
}

/// Walk every cmap subtable and count unique codepoints + bucket each
/// into a script. Counts overlap (one codepoint may appear in several
/// subtables) so the total is an upper bound — that's fine for an info
/// summary; a precise count would require a HashSet over u32 which
/// costs memory we don't need for a one-shot render.
fn scan_cmap(face: &Face<'_>) -> (u32, Vec<String>) {
    let Some(cmap) = face.tables().cmap else {
        return (0, Vec::new());
    };

    let mut total: u32 = 0;
    let mut scripts = [false; SCRIPT_COUNT];

    for subtable in cmap.subtables {
        if !subtable.is_unicode() {
            continue;
        }
        subtable.codepoints(|cp| {
            total = total.saturating_add(1);
            if let Some(idx) = script_bucket(cp) {
                scripts[idx] = true;
            }
        });
    }

    let names: Vec<String> = SCRIPT_NAMES
        .iter()
        .enumerate()
        .filter(|&(i, _)| scripts[i])
        .map(|(_, name)| (*name).to_string())
        .collect();

    (total, names)
}

const SCRIPT_NAMES: &[&str] = &[
    "Latin",
    "Latin Extended",
    "Greek",
    "Cyrillic",
    "Hebrew",
    "Arabic",
    "Devanagari",
    "Thai",
    "Hangul",
    "CJK",
    "Emoji",
    "Symbols",
];
const SCRIPT_COUNT: usize = 12;

/// Bucket a Unicode codepoint into a script slot. Coverage is broad —
/// the buckets exist to give the user a sense of what the font can
/// render, not to enumerate every Unicode block. Codepoints outside
/// every bucket return `None` and don't affect the script list.
fn script_bucket(cp: u32) -> Option<usize> {
    match cp {
        // Basic Latin (ASCII) + Latin-1 Supplement
        0x0020..=0x024F => Some(0),
        // Latin Extended Additional / Phonetic
        0x1D00..=0x1FFF | 0x2C60..=0x2C7F => Some(1),
        // Greek and Coptic
        0x0370..=0x03FF => Some(2),
        // Cyrillic + Supplement
        0x0400..=0x052F => Some(3),
        // Hebrew
        0x0590..=0x05FF => Some(4),
        // Arabic + Supplement + Extended-A
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF => Some(5),
        // Devanagari
        0x0900..=0x097F => Some(6),
        // Thai
        0x0E00..=0x0E7F => Some(7),
        // Hangul Jamo + Syllables
        0x1100..=0x11FF | 0xAC00..=0xD7AF => Some(8),
        // CJK Unified Ideographs (BMP block + a few common ranges)
        0x2E80..=0x2FFF | 0x3000..=0x303F | 0x3400..=0x4DBF | 0x4E00..=0x9FFF => Some(9),
        // Emoji + Misc Symbols + Pictographs (subset)
        0x1F300..=0x1FAFF | 0x2600..=0x27BF => Some(10),
        // General Punctuation / Symbols / Arrows / Math
        0x2000..=0x22FF | 0x2300..=0x25FF | 0x2B00..=0x2BFF => Some(11),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bundled OFL specimen: walks the full gather pipeline against a
    /// real TTF and asserts that the high-signal fields decode as
    /// expected. Catches regressions in the `name` table walker, OS/2
    /// weight readout, and cmap script bucketing.
    #[test]
    fn cabin_gathers_real_metadata() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../test-data/fonts/cabin/Cabin[wdth,wght].ttf"
        ))
        .expect("fixture present");
        let info = gather(&bytes, FontFormat::TrueType);

        assert_eq!(info.face_count, 1);
        assert!(info.parse_errors.is_empty(), "{:?}", info.parse_errors);
        let face = &info.faces[0];
        assert_eq!(face.family, "Cabin");
        assert_eq!(face.subfamily, "Regular");
        assert_eq!(face.postscript_name, "Cabin-Regular");
        assert_eq!(face.weight, 400);
        assert!(face.glyph_count > 100);
        assert!(face.codepoint_count > 100);
        assert!(face.scripts.contains(&"Latin".to_string()));
    }

    /// Script bucketing for a script-heavy face — Sacramento ships
    /// Latin / Latin Extended / Greek / Symbols, so cmap walk should
    /// surface all four. Regression guard for the `script_bucket`
    /// ranges.
    #[test]
    fn sacramento_surfaces_multiple_scripts() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../test-data/fonts/sacramento/Sacramento-Regular.ttf"
        ))
        .expect("fixture present");
        let info = gather(&bytes, FontFormat::TrueType);
        let face = &info.faces[0];
        assert_eq!(face.family, "Sacramento");
        for script in ["Latin", "Latin Extended", "Greek", "Symbols"] {
            assert!(
                face.scripts.contains(&script.to_string()),
                "missing {script} in {:?}",
                face.scripts
            );
        }
    }
}
