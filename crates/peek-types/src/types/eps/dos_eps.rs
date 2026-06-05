//! Binary "DOS EPS" (EPSF) container parser.
//!
//! A binary EPS file opens with a 30-byte header (magic `C5 D0 D3 C6`)
//! holding little-endian offset/length pairs pointing at three
//! sections: the ASCII PostScript program, an optional Windows
//! Metafile (WMF) preview, and an optional TIFF preview. Designers bake
//! the preview so non-PostScript apps can show *something* without an
//! interpreter — peek renders the TIFF one through the image pipeline.
//!
//! Plain `%!PS` EPS files have no such header; the whole file is the
//! PostScript program and any preview (rare EPSI hex-ASCII) is inline.

/// Magic bytes that mark a binary DOS-EPS container.
pub const MAGIC: [u8; 4] = [0xC5, 0xD0, 0xD3, 0xC6];

/// Which preview image format the header points at. Only TIFF is
/// renderable through the image pipeline; WMF is recorded for the Info
/// view but not rendered (no pure-Rust WMF rasteriser).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Tiff,
    Wmf,
}

/// One section's location within the file.
#[derive(Debug, Clone, Copy)]
pub struct Section {
    pub offset: usize,
    pub len: usize,
}

/// Parsed DOS-EPS header: where the PostScript lives plus an optional
/// preview. TIFF wins over WMF when both are present (renderable).
#[derive(Debug, Clone)]
pub struct DosEps {
    pub postscript: Section,
    pub preview_kind: Option<PreviewKind>,
    pub preview: Option<Section>,
}

fn le_u32(b: &[u8]) -> usize {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize
}

/// Parse the 30-byte header if `data` is a binary DOS-EPS container.
/// Returns `None` for plain `%!PS` text EPS or anything too short.
/// Sections are clamped to the actual file length so a corrupt
/// offset/length can't drive an out-of-bounds slice downstream.
pub fn parse(data: &[u8]) -> Option<DosEps> {
    if data.len() < 30 || data[..4] != MAGIC {
        return None;
    }
    let total = data.len();
    let clamp = |off: usize, len: usize| -> Option<Section> {
        if len == 0 || off >= total {
            return None;
        }
        let end = off.saturating_add(len).min(total);
        Some(Section {
            offset: off,
            len: end - off,
        })
    };

    let ps = clamp(le_u32(&data[4..8]), le_u32(&data[8..12]))?;
    let wmf = clamp(le_u32(&data[12..16]), le_u32(&data[16..20]));
    let tiff = clamp(le_u32(&data[20..24]), le_u32(&data[24..28]));

    let (preview_kind, preview) = match (tiff, wmf) {
        (Some(t), _) => (Some(PreviewKind::Tiff), Some(t)),
        (None, Some(w)) => (Some(PreviewKind::Wmf), Some(w)),
        (None, None) => (None, None),
    };

    Some(DosEps {
        postscript: ps,
        preview_kind,
        preview,
    })
}
