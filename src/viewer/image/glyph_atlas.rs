use super::ImageMode;

/// Cell resolution for glyph matching.
pub const CELL_W: u32 = 8;
pub const CELL_H: u32 = 16;

/// Category of a glyph, used for mode-based filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphCategory {
    /// Unicode block/quadrant elements — geometrically precise
    Block,
    /// Curated ASCII subset with distinct spatial patterns
    Curated,
    /// Extended characters (full ASCII, Latin-1, box drawing, geometric shapes)
    Extended,
}

/// A glyph with its pre-computed bitmap at CELL_W x CELL_H resolution.
#[derive(Debug, Clone, Copy)]
pub struct GlyphBitmap {
    /// The character to render
    pub ch: char,
    /// Bitmap: bit i = 1 means "ink" (foreground), 0 means "empty" (background).
    /// Row-major: bit 0 = (row 0, col 0), bit 1 = (row 0, col 1), ...,
    /// bit 7 = (row 0, col 7), bit 8 = (row 1, col 0), etc.
    pub bits: u128,
    /// Category for mode-based filtering
    pub category: GlyphCategory,
}

/// Result of glyph matching.
pub struct GlyphMatch {
    /// The best-matching character
    pub ch: char,
    /// Whether foreground and background colors should be swapped
    /// (the inverted glyph matched better)
    pub inverted: bool,
}

/// Find the glyph whose bitmap best matches the cell's color assignment bitmap.
///
/// Uses Hamming distance (XOR + popcount). Also checks the inverted bitmap
/// (swap fg/bg) for free, since inverted_distance = 128 - normal_distance.
pub fn best_glyph(cell_bits: u128, atlas: &[GlyphBitmap]) -> GlyphMatch {
    let mut best_ch = ' ';
    let mut best_dist = u32::MAX;
    let mut best_inverted = false;

    for glyph in atlas {
        let xor = cell_bits ^ glyph.bits;
        let dist_normal = xor.count_ones();
        let dist_inverted = 128 - dist_normal;

        if dist_normal <= dist_inverted {
            if dist_normal < best_dist {
                best_dist = dist_normal;
                best_ch = glyph.ch;
                best_inverted = false;
            }
        } else if dist_inverted < best_dist {
            best_dist = dist_inverted;
            best_ch = glyph.ch;
            best_inverted = true;
        }
    }

    GlyphMatch {
        ch: best_ch,
        inverted: best_inverted,
    }
}

/// Get the glyph atlas filtered by rendering mode.
pub fn atlas_for_mode(mode: ImageMode) -> Vec<&'static GlyphBitmap> {
    match mode {
        ImageMode::Full => GLYPH_ATLAS.iter().collect(),
        ImageMode::Block => GLYPH_ATLAS
            .iter()
            .filter(|g| matches!(g.category, GlyphCategory::Block | GlyphCategory::Curated))
            .collect(),
        ImageMode::Ascii => Vec::new(), // not used for block-color rendering
    }
}

// ── Block element bitmaps (programmatically defined, exact geometry) ─────

/// Helper: create a bitmap where rows `start..end` are fully set (all 8 cols).
const fn full_rows(start: u32, end: u32) -> u128 {
    let mut bits: u128 = 0;
    let mut row = start;
    while row < end {
        // Set all 8 bits in this row
        bits |= 0xFFu128 << (row * 8);
        row += 1;
    }
    bits
}

/// Helper: create a bitmap where columns `start..end` are set (all 16 rows).
const fn full_cols(start: u32, end: u32) -> u128 {
    let mut bits: u128 = 0;
    let mut row = 0u32;
    while row < 16 {
        let mut col = start;
        while col < end {
            bits |= 1u128 << (row * 8 + col);
            col += 1;
        }
        row += 1;
    }
    bits
}

/// Helper: create a bitmap for a quadrant (2x2 grid of the cell).
/// top_left, top_right, bottom_left, bottom_right indicate which quadrants are filled.
const fn quadrant(tl: bool, tr: bool, bl: bool, br: bool) -> u128 {
    let mut bits: u128 = 0;
    let mut row = 0u32;
    while row < 16 {
        let mut col = 0u32;
        while col < 8 {
            let is_top = row < 8;
            let is_left = col < 4;
            let fill = match (is_top, is_left) {
                (true, true) => tl,
                (true, false) => tr,
                (false, true) => bl,
                (false, false) => br,
            };
            if fill {
                bits |= 1u128 << (row * 8 + col);
            }
            col += 1;
        }
        row += 1;
    }
    bits
}

/// Helper: shade pattern — every Nth pixel set.
const fn shade(n: u32) -> u128 {
    let mut bits: u128 = 0;
    let mut i = 0u32;
    while i < 128 {
        if i.is_multiple_of(n) {
            bits |= 1u128 << i;
        }
        i += 1;
    }
    bits
}

// Include the generated glyph data from the font rasterizer.
// This file defines GENERATED_GLYPHS: &[GlyphBitmap].
include!("glyph_atlas_data.rs");

/// The complete glyph atlas, combining block elements and generated glyphs.
static GLYPH_ATLAS: std::sync::LazyLock<Vec<GlyphBitmap>> = std::sync::LazyLock::new(|| {
    let mut atlas = vec![
        // ── Block elements (exact geometry) ──
        GlyphBitmap { ch: ' ',  bits: 0,                                    category: GlyphCategory::Block },
        GlyphBitmap { ch: '█', bits: full_rows(0, 16),                     category: GlyphCategory::Block },
        GlyphBitmap { ch: '▀', bits: full_rows(0, 8),                      category: GlyphCategory::Block },
        GlyphBitmap { ch: '▄', bits: full_rows(8, 16),                     category: GlyphCategory::Block },
        GlyphBitmap { ch: '▌', bits: full_cols(0, 4),                      category: GlyphCategory::Block },
        GlyphBitmap { ch: '▐', bits: full_cols(4, 8),                      category: GlyphCategory::Block },
        // Quadrant characters
        GlyphBitmap { ch: '▖', bits: quadrant(false, false, true, false),   category: GlyphCategory::Block },
        GlyphBitmap { ch: '▗', bits: quadrant(false, false, false, true),   category: GlyphCategory::Block },
        GlyphBitmap { ch: '▘', bits: quadrant(true, false, false, false),   category: GlyphCategory::Block },
        GlyphBitmap { ch: '▝', bits: quadrant(false, true, false, false),   category: GlyphCategory::Block },
        GlyphBitmap { ch: '▙', bits: quadrant(true, false, true, true),     category: GlyphCategory::Block },
        GlyphBitmap { ch: '▛', bits: quadrant(true, true, true, false),     category: GlyphCategory::Block },
        GlyphBitmap { ch: '▜', bits: quadrant(true, true, false, true),     category: GlyphCategory::Block },
        GlyphBitmap { ch: '▟', bits: quadrant(false, true, true, true),     category: GlyphCategory::Block },
        // Shade characters
        GlyphBitmap { ch: '░', bits: shade(4),                              category: GlyphCategory::Block },
        GlyphBitmap { ch: '▒', bits: shade(2),                              category: GlyphCategory::Block },
        GlyphBitmap { ch: '▓', bits: shade(4) | shade(2),                   category: GlyphCategory::Block },
    ];

    // Add all generated glyphs from the font rasterizer
    atlas.extend_from_slice(GENERATED_GLYPHS);

    atlas
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_space_is_empty() {
        let space = GLYPH_ATLAS.iter().find(|g| g.ch == ' ').unwrap();
        assert_eq!(space.bits, 0);
    }

    #[test]
    fn test_full_block_is_full() {
        let full = GLYPH_ATLAS.iter().find(|g| g.ch == '█').unwrap();
        // All 128 bits should be set
        assert_eq!(full.bits.count_ones(), 128);
    }

    #[test]
    fn test_upper_half_block() {
        let upper = GLYPH_ATLAS.iter().find(|g| g.ch == '▀').unwrap();
        assert_eq!(upper.bits.count_ones(), 64);
        // Only top 8 rows should be set
        for row in 0..8 {
            for col in 0..8 {
                assert!(upper.bits & (1u128 << (row * 8 + col)) != 0);
            }
        }
        for row in 8..16 {
            for col in 0..8 {
                assert!(upper.bits & (1u128 << (row * 8 + col)) == 0);
            }
        }
    }

    #[test]
    fn test_lower_half_block() {
        let lower = GLYPH_ATLAS.iter().find(|g| g.ch == '▄').unwrap();
        assert_eq!(lower.bits.count_ones(), 64);
    }

    #[test]
    fn test_best_glyph_all_zeros() {
        let atlas = atlas_for_mode(ImageMode::Block);
        let result = best_glyph(0, &atlas.iter().map(|g| **g).collect::<Vec<_>>());
        // All-zero bitmap should match space (or full block inverted)
        assert!(result.ch == ' ' || (result.ch == '█' && result.inverted));
    }

    #[test]
    fn test_best_glyph_all_ones() {
        let all_ones: u128 = u128::MAX;
        let atlas = atlas_for_mode(ImageMode::Block);
        let glyphs: Vec<GlyphBitmap> = atlas.iter().map(|g| **g).collect();
        let result = best_glyph(all_ones, &glyphs);
        // All-one bitmap should match full block (or space inverted)
        assert!(result.ch == '█' || (result.ch == ' ' && result.inverted));
    }

    #[test]
    fn test_best_glyph_upper_half() {
        let upper_bits = full_rows(0, 8);
        let atlas = atlas_for_mode(ImageMode::Block);
        let glyphs: Vec<GlyphBitmap> = atlas.iter().map(|g| **g).collect();
        let result = best_glyph(upper_bits, &glyphs);
        assert!(
            result.ch == '▀' || (result.ch == '▄' && result.inverted),
            "expected ▀ or inverted ▄, got '{}' inverted={}",
            result.ch,
            result.inverted
        );
    }
}
