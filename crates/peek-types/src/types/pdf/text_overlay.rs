//! Reconstructed-text overlay for the PDF page view (`o`).
//!
//! The page render turns text into glyph-cell "garbage" — readable
//! shapes only at exactly the right zoom. This module projects the
//! document's real text layer onto the rendered cell grid: each word's
//! page-point bounding box maps through the same linear projection the
//! rasteriser uses (page pt → source px → effective cells), and the
//! word's characters overwrite the rendered glyphs at those cells —
//! centered in the word's rendered box, cropped from both sides when
//! the box is narrower than the word, padded with spaces when wider.
//! Cell colors are kept (the letter takes the cell's existing SGR
//! state); only when a cell's fg≈bg (a letter landing on blank paper
//! would be invisible) is the foreground nudged to a contrasting tone.
//!
//! Words rendered far smaller than a terminal cell are skipped — at
//! those sizes several text lines collapse into one cell row and the
//! overlay would be soup. Overzoomed words still paint (centered, with
//! glyph garbage around them) so zooming past 1:1 keeps the text
//! readable somewhere.
//!
//! Pure cell geometry + string splicing — no Pdfium types — so the
//! whole layout is unit-testable without a document.

use std::collections::HashMap;

use crate::theme::{self as sgr, Sgr, SgrKind};
use crate::types::image::paged_render::GridMap;

/// One word from the page's text layer. Coordinates are page points
/// with a top-left origin (y grows downward, matching raster space);
/// the box is the union of the word's character loose bounds.
pub struct WordBox {
    pub text: String,
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// All overlay words of one page plus the page dimensions in points —
/// the source space the projection maps from.
pub struct PageWords {
    pub page_w: f32,
    pub page_h: f32,
    pub words: Vec<WordBox>,
}

/// Smallest rendered word height (in effective cell rows) that still
/// gets an overlay. Below this, neighbouring text lines share a cell
/// row and overlaid words would overwrite each other unreadably.
const MIN_WORD_ROWS: f32 = 0.55;

/// Rendered-box-to-char-count ratio above which a word counts as
/// overzoomed. Up to it, the whole box is space-padded so the word
/// replaces its own half-readable glyph cells; beyond it the rendered
/// glyphs are big enough to read on their own, so the overlay keeps
/// them and blanks only one delimiting cell each side of the word.
const OVERZOOM_RATIO: usize = 3;

/// Luminance gap below which a cell's fg and bg are "the same color"
/// for letter visibility, triggering the contrast nudge.
const MIN_CONTRAST: i32 = 48;

/// Overlay cells for the current viewport: per viewport row, the
/// (column, char) substitutions sorted by column. Built by [`layout`],
/// consumed by [`splice`] one line at a time.
pub type OverlayCells = HashMap<u32, Vec<(u32, char)>>;

/// Project `words` onto the rendered viewport described by `map`.
/// `margin` is the image-pipeline margin in source pixels (each side)
/// — the page bitmap sits inset by it inside `map.src_w` × `map.src_h`.
pub(crate) fn layout(words: &PageWords, map: &GridMap, margin: u32) -> OverlayCells {
    let mut out: OverlayCells = HashMap::new();
    if words.page_w <= 0.0 || words.page_h <= 0.0 || map.src_w == 0 || map.src_h == 0 {
        return out;
    }
    let bitmap_w = map.src_w.saturating_sub(margin * 2).max(1) as f32;
    let bitmap_h = map.src_h.saturating_sub(margin * 2).max(1) as f32;
    // Page pt → effective cell, composed from pt → source px → cell.
    let cell_x = |pt: f32| -> f32 {
        let px = margin as f32 + pt * bitmap_w / words.page_w;
        px * map.effective_cols as f32 / map.src_w as f32
    };
    let cell_y = |pt: f32| -> f32 {
        let px = margin as f32 + pt * bitmap_h / words.page_h;
        px * map.effective_rows as f32 / map.src_h as f32
    };

    // Words of one text line must land on one cell row — rounding each
    // word's own center independently scatters a sentence across ±1
    // row, because descenders / punctuation nudge the loose-bounds box.
    // Words stream in reading order, so a running baseline group
    // suffices: consecutive words whose vertical centers sit within
    // half a line height share the group, and every member snaps to
    // the row of the group's first center.
    let mut line: Option<(f32, f32, i64)> = None; // (center pt, height pt, cell row)
    for word in &words.words {
        let rows_high = cell_y(word.top + word.height) - cell_y(word.top);
        if rows_high < MIN_WORD_ROWS {
            continue;
        }
        let center = word.top + word.height / 2.0;
        let row = match line {
            Some((line_cy, line_h, line_row))
                if (center - line_cy).abs() < line_h.max(word.height) * 0.5 =>
            {
                line = Some((line_cy, line_h.max(word.height), line_row));
                line_row
            }
            _ => {
                let row = cell_y(center).floor() as i64;
                line = Some((center, word.height, row));
                row
            }
        };
        let col0_f = cell_x(word.left);
        let col1_f = cell_x(word.left + word.width);
        let box_cells = ((col1_f - col0_f).round() as i64).max(1) as usize;

        let chars: Vec<char> = word.text.chars().collect();
        // Fit the word into its rendered box: crop evenly from both
        // sides when too long, pad with spaces (centering) when short,
        // and at overzoom keep the box's glyphs — pad just one
        // delimiting cell per side.
        let (placed, start_col): (Vec<char>, i64) = if chars.len() > box_cells {
            let skip = (chars.len() - box_cells) / 2;
            (
                chars[skip..skip + box_cells].to_vec(),
                col0_f.round() as i64,
            )
        } else if box_cells > chars.len() * OVERZOOM_RATIO {
            let placed: Vec<char> = std::iter::once(' ')
                .chain(chars.iter().copied())
                .chain(std::iter::once(' '))
                .collect();
            let center = (col0_f + col1_f) / 2.0;
            let start = (center - placed.len() as f32 / 2.0).round() as i64;
            (placed, start)
        } else {
            let pad = box_cells - chars.len();
            let left_pad = pad / 2;
            let placed = std::iter::repeat_n(' ', left_pad)
                .chain(chars.iter().copied())
                .chain(std::iter::repeat_n(' ', pad - left_pad))
                .collect();
            (placed, col0_f.round() as i64)
        };

        let vrow = row - map.scroll_y as i64;
        if vrow < 0 || vrow >= map.viewport_rows as i64 {
            continue;
        }
        let row_cells = out.entry(vrow as u32).or_default();
        for (i, ch) in placed.into_iter().enumerate() {
            let vcol = start_col + i as i64 - map.scroll_x as i64;
            if vcol < 0 || vcol >= map.viewport_cols as i64 {
                continue;
            }
            row_cells.push((vcol as u32, ch));
        }
    }
    for cells in out.values_mut() {
        // Later words win on collision: stable sort keeps insertion
        // order within a column, and splice takes the last match.
        cells.sort_by_key(|(c, _)| *c);
    }
    out
}

/// Overwrite the glyphs of one rendered viewport line with the overlay
/// `cells` (sorted by column), preserving SGR state. A letter landing
/// on a low-contrast cell (fg≈bg, e.g. blank paper) gets a temporary
/// contrasting foreground so it doesn't vanish; the cell's own fg is
/// re-established right after.
pub fn splice(line: &str, cells: &[(u32, char)]) -> String {
    if cells.is_empty() {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len() + cells.len() * 8);
    let mut col: u32 = 0;
    let mut style = CellStyle::default();
    for tok in sgr::scan(line) {
        match tok {
            Sgr::Esc(esc) => {
                style.observe(esc);
                out.push_str(esc);
            }
            Sgr::Text(text) => {
                for ch in text.chars() {
                    let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0) as u32;
                    let replacement = (w == 1)
                        .then(|| cells.iter().rev().find(|(c, _)| *c == col).map(|&(_, r)| r))
                        .flatten();
                    match replacement {
                        Some(r) => style.push_contrasted(&mut out, r),
                        None => out.push(ch),
                    }
                    col += w;
                }
            }
        }
    }
    out
}

/// Tracked truecolor fg/bg of the splice cursor, for the contrast
/// nudge. Only `38;2;…` / `48;2;…` escapes are parsed — in 256/16
/// color modes the overlay keeps cell colors untouched.
#[derive(Default)]
struct CellStyle {
    fg: Option<(String, u8)>,
    bg_lum: Option<u8>,
}

impl CellStyle {
    fn observe(&mut self, esc: &str) {
        match sgr::classify(esc) {
            SgrKind::ResetAll => {
                self.fg = None;
                self.bg_lum = None;
            }
            SgrKind::ResetFg => self.fg = None,
            SgrKind::ResetBg => self.bg_lum = None,
            SgrKind::Fg => {
                self.fg = parse_truecolor(esc).map(|l| (esc.to_string(), l));
            }
            SgrKind::Bg => self.bg_lum = parse_truecolor(esc),
            SgrKind::Other => {}
        }
    }

    /// Append the overlay char, nudging the foreground when it would
    /// be invisible against the cell's background.
    fn push_contrasted(&self, out: &mut String, ch: char) {
        if ch == ' ' {
            out.push(ch);
            return;
        }
        let (Some((fg_esc, fg_lum)), Some(bg_lum)) = (&self.fg, self.bg_lum) else {
            out.push(ch);
            return;
        };
        if (*fg_lum as i32 - bg_lum as i32).abs() >= MIN_CONTRAST {
            out.push(ch);
            return;
        }
        if bg_lum > 128 {
            sgr::write_fg_truecolor(out, 0, 0, 0);
        } else {
            sgr::write_fg_truecolor(out, 255, 255, 255);
        }
        out.push(ch);
        out.push_str(fg_esc);
    }
}

/// Parse the luminance of a `38;2;r;g;b` / `48;2;r;g;b` escape.
/// Returns `None` for every other color form.
fn parse_truecolor(esc: &str) -> Option<u8> {
    let body = esc.strip_prefix("\x1b[")?.strip_suffix('m')?;
    let mut parts = body.split(';');
    let lead = parts.next()?;
    if lead != "38" && lead != "48" {
        return None;
    }
    if parts.next()? != "2" {
        return None;
    }
    let r: u8 = parts.next()?.parse().ok()?;
    let g: u8 = parts.next()?.parse().ok()?;
    let b: u8 = parts.next()?.parse().ok()?;
    Some(sgr::rgb_to_luminance(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> GridMap {
        // Page 100×100 pt → source 1000×1000 px (no margin) →
        // effective grid 50×50 cells, full grid visible.
        GridMap {
            effective_cols: 50,
            effective_rows: 50,
            viewport_cols: 50,
            viewport_rows: 50,
            scroll_x: 0,
            scroll_y: 0,
            src_w: 1000,
            src_h: 1000,
        }
    }

    fn page(words: Vec<WordBox>) -> PageWords {
        PageWords {
            page_w: 100.0,
            page_h: 100.0,
            words,
        }
    }

    fn word(text: &str, left: f32, top: f32, width: f32, height: f32) -> WordBox {
        WordBox {
            text: text.to_string(),
            left,
            top,
            width,
            height,
        }
    }

    fn row_string(cells: &OverlayCells, row: u32, width: usize) -> String {
        let mut s = vec![' '; width];
        if let Some(v) = cells.get(&row) {
            for &(c, ch) in v {
                s[c as usize] = ch;
            }
        }
        s.into_iter().collect()
    }

    #[test]
    fn word_lands_at_projected_cells() {
        // 2pt-high word at pt (10..18, 10..12): 1 cell row high (2pt =
        // 1 cell), box 4 cells wide starting at col 5, center row 5.
        let cells = layout(&page(vec![word("ab", 10.0, 10.0, 8.0, 2.0)]), &map(), 0);
        assert_eq!(row_string(&cells, 5, 12), "      ab    ");
    }

    #[test]
    fn long_word_crops_evenly_from_both_sides() {
        // Box 4 cells, word 8 chars → crop 2 from each side.
        let cells = layout(
            &page(vec![word("abcdefgh", 10.0, 10.0, 8.0, 2.0)]),
            &map(),
            0,
        );
        assert_eq!(row_string(&cells, 5, 12), "     cdef   ");
    }

    #[test]
    fn short_word_pads_centered_with_spaces() {
        // Box 5 cells (10pt), word 2 chars → under the overzoom ratio,
        // so the whole box is padded: 1 space left, 2 right. The
        // spaces are real overlay cells (they blank the glyph garbage).
        let cells = layout(&page(vec![word("ab", 10.0, 10.0, 10.0, 2.0)]), &map(), 0);
        let row = cells.get(&5).expect("row populated");
        assert_eq!(row.len(), 5);
        assert_eq!(row_string(&cells, 5, 16), "      ab        ");
    }

    #[test]
    fn overzoomed_word_keeps_box_glyphs() {
        // Box 8 cells vs 2 chars → past the overzoom ratio: only the
        // word plus one delimiting space per side is written, centered
        // in the box; the rest of the box keeps its rendered glyphs.
        let cells = layout(&page(vec![word("ab", 10.0, 10.0, 16.0, 2.0)]), &map(), 0);
        let row = cells.get(&5).expect("row populated");
        assert_eq!(row.len(), 4);
        assert_eq!(row_string(&cells, 5, 16), "        ab      ");
    }

    #[test]
    fn same_line_words_snap_to_one_row() {
        // Word B's box reaches lower (descender), so its own center
        // would round to row 6 — but it follows A on the same baseline
        // and must snap to A's row 5.
        let cells = layout(
            &page(vec![
                word("ab", 10.0, 10.0, 4.0, 2.0),
                word("cd", 16.0, 10.8, 4.0, 2.4),
            ]),
            &map(),
            0,
        );
        assert_eq!(cells.len(), 1, "both words on one row: {cells:?}");
        assert!(cells.contains_key(&5));
    }

    #[test]
    fn tiny_word_skipped() {
        // 0.5pt high → 0.25 cell rows, far under MIN_WORD_ROWS.
        let cells = layout(&page(vec![word("ab", 10.0, 10.0, 8.0, 0.5)]), &map(), 0);
        assert!(cells.is_empty());
    }

    #[test]
    fn scroll_offsets_shift_viewport_cells() {
        let mut m = map();
        m.scroll_x = 4;
        m.scroll_y = 3;
        m.viewport_cols = 10;
        m.viewport_rows = 10;
        let cells = layout(&page(vec![word("ab", 10.0, 10.0, 8.0, 2.0)]), &m, 0);
        // Effective (row 5, cols 6..8) → viewport (row 2, cols 2..4).
        assert_eq!(row_string(&cells, 2, 10), "  ab      ");
    }

    #[test]
    fn off_viewport_word_clipped() {
        let mut m = map();
        m.viewport_rows = 4; // word's row 5 is below the viewport
        let cells = layout(&page(vec![word("ab", 10.0, 10.0, 8.0, 2.0)]), &m, 0);
        assert!(cells.is_empty());
    }

    #[test]
    fn margin_insets_projection() {
        // 100px margin on a 1000px source: bitmap spans px 100..900,
        // so pt 10 → px 100 + 10*8 = 180 → col 9.
        let mut m = map();
        m.src_w = 1000;
        m.src_h = 1000;
        let cells = layout(&page(vec![word("ab", 10.0, 10.0, 8.0, 4.0)]), &m, 100);
        let row = cells.keys().next().copied().expect("one row");
        let v = &cells[&row];
        assert_eq!(v.first().map(|&(c, _)| c), Some(9));
    }

    #[test]
    fn splice_replaces_plain_cells() {
        assert_eq!(splice("XXXXXX", &[(1, 'a'), (2, 'b')]), "XabXXX");
    }

    #[test]
    fn splice_preserves_escapes_and_columns() {
        let line = "\x1b[38;2;1;2;3mAB\x1b[0mCD";
        // Display cols: A=0 B=1 C=2 D=3.
        assert_eq!(
            splice(line, &[(1, 'x'), (2, 'y')]),
            "\x1b[38;2;1;2;3mAx\x1b[0myD"
        );
    }

    #[test]
    fn splice_nudges_invisible_letter_to_contrast() {
        // White on white: letter would vanish → black fg injected,
        // original fg restored after.
        let line = "\x1b[38;2;250;250;250m\x1b[48;2;255;255;255mAB";
        let out = splice(line, &[(0, 'x')]);
        assert_eq!(
            out,
            "\x1b[38;2;250;250;250m\x1b[48;2;255;255;255m\x1b[38;2;0;0;0mx\x1b[38;2;250;250;250mB"
        );
    }

    #[test]
    fn splice_keeps_contrasting_cell_colors() {
        let line = "\x1b[38;2;0;0;0m\x1b[48;2;255;255;255mAB";
        let out = splice(line, &[(0, 'x')]);
        assert_eq!(out, "\x1b[38;2;0;0;0m\x1b[48;2;255;255;255mxB");
    }

    #[test]
    fn splice_space_never_color_nudged() {
        let line = "\x1b[38;2;250;250;250m\x1b[48;2;255;255;255mAB";
        let out = splice(line, &[(0, ' ')]);
        assert_eq!(out, "\x1b[38;2;250;250;250m\x1b[48;2;255;255;255m B");
    }
}
