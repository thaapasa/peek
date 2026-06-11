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
//! Cell colors are kept, but oriented: the half-block renderer assigns
//! a cell's two pixel colors to fg/bg by position, not meaning, so the
//! splice paints the letter in whichever of the pair lies nearer the
//! word's font color (swapping fg/bg when needed) — and when the pair
//! has no usable contrast (a letter landing on blank paper would be
//! invisible) the foreground is nudged to black/white.
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
    /// The word's font color (fill color of its first character; black
    /// when the document doesn't declare one). Orients the splice: the
    /// half-block renderer assigns ink and paper to fg/bg arbitrarily
    /// per cell, so the splice swaps them when the bg is the one nearer
    /// this color.
    pub ink: Rgb,
}

/// 24-bit color triple shared by the word ink and the cell-style
/// tracker.
pub type Rgb = (u8, u8, u8);

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

/// One substituted cell: viewport column, replacement char, and the
/// word's ink color. Padding spaces carry the ink too — their
/// background must orient to the word's paper side just like the
/// letters', or a space lands ink-colored and reads as a dark blob in
/// the middle of the word.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayCell {
    pub col: u32,
    pub ch: char,
    pub ink: Rgb,
}

/// Overlay cells for the current viewport: per viewport row, the
/// substitutions sorted by column. Built by [`layout`], consumed by
/// [`splice`] one line at a time.
pub type OverlayCells = HashMap<u32, Vec<OverlayCell>>;

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
            row_cells.push(OverlayCell {
                col: vcol as u32,
                ch,
                ink: word.ink,
            });
        }
    }
    for cells in out.values_mut() {
        // Later words win on collision: stable sort keeps insertion
        // order within a column, and splice takes the last match.
        cells.sort_by_key(|c| c.col);
    }
    out
}

/// Overwrite the glyphs of one rendered viewport line with the overlay
/// `cells` (sorted by column), preserving SGR state. Two per-cell color
/// fixes, both temporary (the cell's own colors are re-established
/// right after the letter):
///
/// * **Orientation** — the half-block renderer assigns a cell's two
///   pixel colors to fg/bg by pixel position, not meaning, so the fg a
///   letter would paint in is the *paper* color about half the time.
///   When the cell's bg sits nearer the word's ink color than the fg
///   does, the pair is swapped. A padding space orients the same way
///   (only its bg shows): without the swap it lands ink-colored and
///   reads as a dark blob inside the word.
/// * **Contrast** — a letter landing on a low-contrast cell (fg≈bg,
///   e.g. blank paper) gets a black/white foreground so it doesn't
///   vanish.
///
/// Works in every color encoding the renderer emits — truecolor,
/// 256-palette, and 16-color escapes are all parsed (compared via
/// their nominal RGB) and swapped by re-planing the cell's own escape,
/// so the output stays in the line's encoding.
pub fn splice(line: &str, cells: &[OverlayCell]) -> String {
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
                    let replacement =
                        (w == 1).then(|| cells.iter().rev().find(|c| c.col == col).copied());
                    match replacement.flatten() {
                        Some(cell) => style.push_letter(&mut out, cell),
                        None => out.push(ch),
                    }
                    col += w;
                }
            }
        }
    }
    out
}

/// Tracked fg/bg of the splice cursor, for the orientation swap +
/// contrast nudge: the raw escape (to restore / re-plane) plus its
/// nominal RGB (to compare). Truecolor, 256-palette, and 16-color
/// escapes are all parsed; in plain mode there are no colors and the
/// overlay chars land as-is.
#[derive(Default)]
struct CellStyle {
    fg: Option<(String, Rgb)>,
    bg: Option<(String, Rgb)>,
}

impl CellStyle {
    fn observe(&mut self, esc: &str) {
        match sgr::classify(esc) {
            SgrKind::ResetAll => {
                self.fg = None;
                self.bg = None;
            }
            SgrKind::ResetFg => self.fg = None,
            SgrKind::ResetBg => self.bg = None,
            SgrKind::Fg | SgrKind::Bg => {
                let slot = if sgr::classify(esc) == SgrKind::Fg {
                    &mut self.fg
                } else {
                    &mut self.bg
                };
                *slot = parse_color(esc).map(|rgb| (esc.to_string(), rgb));
            }
            SgrKind::Other => {}
        }
    }

    /// Append the overlay char with the orientation swap + contrast
    /// nudge described on [`splice`], restoring the cell's own colors
    /// after.
    fn push_letter(&self, out: &mut String, cell: OverlayCell) {
        let (Some((fg_esc, fg_rgb)), Some((bg_esc, bg_rgb))) = (&self.fg, &self.bg) else {
            out.push(cell.ch);
            return;
        };
        // Orientation: paint the char in whichever of the cell's two
        // colors lies nearer the word's ink, on the other one.
        let swap = color_dist(*bg_rgb, cell.ink) < color_dist(*fg_rgb, cell.ink);
        if cell.ch == ' ' {
            // Only the background shows under a space; orient it to
            // the paper side, no contrast concern.
            if swap {
                push_replaned(out, fg_esc, SgrKind::Bg);
                out.push(cell.ch);
                out.push_str(bg_esc);
            } else {
                out.push(cell.ch);
            }
            return;
        }
        let (letter_fg, letter_bg) = if swap {
            (*bg_rgb, *fg_rgb)
        } else {
            (*fg_rgb, *bg_rgb)
        };
        // Contrast: a still-invisible letter gets a black/white fg.
        let bg_lum = lum(letter_bg);
        let nudge = (lum(letter_fg) as i32 - bg_lum as i32).abs() < MIN_CONTRAST;
        if !swap && !nudge {
            out.push(cell.ch);
            return;
        }
        if swap {
            push_replaned(out, fg_esc, SgrKind::Bg);
        }
        if nudge {
            // Black/white in the same encoding as the cell's escapes,
            // so a 256/16-color stream stays in its palette.
            let c = if bg_lum > 128 { 0u8 } else { 255 };
            out.push_str(&match escape_kind(fg_esc) {
                EscapeKind::TrueColor => format!("\x1b[38;2;{c};{c};{c}m"),
                EscapeKind::Ansi256 => format!("\x1b[38;5;{}m", if c == 0 { 16 } else { 231 }),
                EscapeKind::Ansi16 => (if c == 0 { "\x1b[30m" } else { "\x1b[97m" }).to_string(),
            });
        } else if swap {
            push_replaned(out, bg_esc, SgrKind::Fg);
        }
        out.push(cell.ch);
        out.push_str(fg_esc);
        if swap {
            out.push_str(bg_esc);
        }
    }
}

fn lum(rgb: Rgb) -> u8 {
    sgr::rgb_to_luminance(rgb.0, rgb.1, rgb.2)
}

/// Squared RGB distance — only compared against itself, so no sqrt.
fn color_dist(a: Rgb, b: Rgb) -> u32 {
    let d = |x: u8, y: u8| {
        let d = x as i32 - y as i32;
        (d * d) as u32
    };
    d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
}

/// Color-escape encoding, for emitting swaps / nudges in the same
/// palette the line uses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EscapeKind {
    TrueColor,
    Ansi256,
    Ansi16,
}

/// Classify a fg/bg color escape's encoding by its leading parameter.
/// Only called on escapes [`parse_color`] accepted.
fn escape_kind(esc: &str) -> EscapeKind {
    let lead = lead_param(esc);
    match lead {
        38 | 48 => {
            if esc.contains(";5;") {
                EscapeKind::Ansi256
            } else {
                EscapeKind::TrueColor
            }
        }
        _ => EscapeKind::Ansi16,
    }
}

fn lead_param(esc: &str) -> u32 {
    esc.trim_start_matches("\x1b[")
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0u32, |acc, b| acc * 10 + (b - b'0') as u32)
}

/// Append `esc` converted to the other plane (fg color emitted as a
/// bg escape or vice versa), preserving its encoding: `38;…` ↔ `48;…`
/// for truecolor / 256, `3x` ↔ `4x` and `9x` ↔ `10x` for the base 16.
fn push_replaned(out: &mut String, esc: &str, to: SgrKind) {
    let body = esc
        .trim_start_matches("\x1b[")
        .trim_end_matches('m')
        .to_string();
    let lead = lead_param(esc);
    let rest = body.split_once(';').map(|(_, r)| r);
    let new_lead = match (lead, to) {
        (38, SgrKind::Bg) => 48,
        (48, SgrKind::Fg) => 38,
        (30..=37, SgrKind::Bg) => lead + 10,
        (40..=47, SgrKind::Fg) => lead - 10,
        (90..=97, SgrKind::Bg) => lead + 10,
        (100..=107, SgrKind::Fg) => lead - 10,
        _ => lead, // already on the requested plane
    };
    match rest {
        Some(rest) => {
            out.push_str(&format!("\x1b[{new_lead};{rest}m"));
        }
        None => {
            out.push_str(&format!("\x1b[{new_lead}m"));
        }
    }
}

/// Parse the nominal RGB of a fg/bg color escape: truecolor
/// (`38;2;r;g;b`), 256-palette (`38;5;n` via the xterm palette), and
/// base-16 (`30..=37` / `90..=97` and bg counterparts, via the
/// nominal xterm table). Returns `None` for malformed escapes.
fn parse_color(esc: &str) -> Option<Rgb> {
    let body = esc.strip_prefix("\x1b[")?.strip_suffix('m')?;
    let mut parts = body.split(';');
    let lead: u32 = parts.next()?.parse().ok()?;
    match lead {
        38 | 48 => match parts.next()? {
            "2" => {
                let r: u8 = parts.next()?.parse().ok()?;
                let g: u8 = parts.next()?.parse().ok()?;
                let b: u8 = parts.next()?.parse().ok()?;
                Some((r, g, b))
            }
            "5" => {
                let idx: u8 = parts.next()?.parse().ok()?;
                Some(sgr::ansi256_to_rgb(idx))
            }
            _ => None,
        },
        30..=37 => Some(sgr::ansi16_to_rgb((lead - 30) as u8)),
        90..=97 => Some(sgr::ansi16_to_rgb((lead - 90 + 8) as u8)),
        40..=47 => Some(sgr::ansi16_to_rgb((lead - 40) as u8)),
        100..=107 => Some(sgr::ansi16_to_rgb((lead - 100 + 8) as u8)),
        _ => None,
    }
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
            ink: (0, 0, 0),
        }
    }

    /// Black-ink cell, for splice tests where only geometry matters.
    fn cell(col: u32, ch: char) -> OverlayCell {
        OverlayCell {
            col,
            ch,
            ink: (0, 0, 0),
        }
    }

    fn row_string(cells: &OverlayCells, row: u32, width: usize) -> String {
        let mut s = vec![' '; width];
        if let Some(v) = cells.get(&row) {
            for c in v {
                s[c.col as usize] = c.ch;
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
        assert_eq!(v.first().map(|c| c.col), Some(9));
    }

    #[test]
    fn splice_replaces_plain_cells() {
        assert_eq!(splice("XXXXXX", &[cell(1, 'a'), cell(2, 'b')]), "XabXXX");
    }

    #[test]
    fn splice_preserves_escapes_and_columns() {
        let line = "\x1b[38;2;1;2;3mAB\x1b[0mCD";
        // Display cols: A=0 B=1 C=2 D=3.
        assert_eq!(
            splice(line, &[cell(1, 'x'), cell(2, 'y')]),
            "\x1b[38;2;1;2;3mAx\x1b[0myD"
        );
    }

    #[test]
    fn splice_nudges_invisible_letter_to_contrast() {
        // White on white: letter would vanish → black fg injected,
        // original fg restored after.
        let line = "\x1b[38;2;250;250;250m\x1b[48;2;255;255;255mAB";
        let out = splice(line, &[cell(0, 'x')]);
        assert_eq!(
            out,
            "\x1b[38;2;250;250;250m\x1b[48;2;255;255;255m\x1b[38;2;0;0;0mx\x1b[38;2;250;250;250mB"
        );
    }

    #[test]
    fn splice_keeps_contrasting_cell_colors() {
        let line = "\x1b[38;2;0;0;0m\x1b[48;2;255;255;255mAB";
        let out = splice(line, &[cell(0, 'x')]);
        assert_eq!(out, "\x1b[38;2;0;0;0m\x1b[48;2;255;255;255mxB");
    }

    #[test]
    fn splice_space_untouched_on_paper_oriented_cell() {
        // Bg already the paper side (white, ink black) — a space needs
        // no escape at all, and never a contrast nudge.
        let line = "\x1b[38;2;250;250;250m\x1b[48;2;255;255;255mAB";
        let out = splice(line, &[cell(0, ' ')]);
        assert_eq!(out, "\x1b[38;2;250;250;250m\x1b[48;2;255;255;255m B");
    }

    #[test]
    fn splice_space_orients_bg_to_paper_side() {
        // Inverted cell (white fg, near-black bg, ink black): only the
        // bg shows under a space, so it takes the cell's paper color —
        // the fg re-planed to a bg escape — and is restored after.
        let line = "\x1b[38;2;255;255;255m\x1b[48;2;10;10;10mAB";
        let out = splice(line, &[cell(0, ' ')]);
        assert_eq!(
            out,
            "\x1b[38;2;255;255;255m\x1b[48;2;10;10;10m\
             \x1b[48;2;255;255;255m \x1b[48;2;10;10;10mB"
        );
    }

    #[test]
    fn splice_swaps_in_ansi256_palette() {
        // 256-mode cell: white fg (231) on black bg (16), black ink →
        // swap, emitted by re-planing the cell's own `;5;` escapes so
        // the stream stays in the 256 palette.
        let line = "\x1b[38;5;231m\x1b[48;5;16mAB";
        let out = splice(line, &[cell(0, 'x')]);
        assert_eq!(
            out,
            "\x1b[38;5;231m\x1b[48;5;16m\
             \x1b[48;5;231m\x1b[38;5;16mx\
             \x1b[38;5;231m\x1b[48;5;16mB"
        );
    }

    #[test]
    fn splice_swaps_in_ansi16_palette() {
        // 16-color cell: bright-white fg (97) on black bg (40), black
        // ink → swap via the `9x` → `10x` / `4x` → `3x` plane shift.
        let line = "\x1b[97m\x1b[40mAB";
        let out = splice(line, &[cell(0, 'x')]);
        assert_eq!(out, "\x1b[97m\x1b[40m\x1b[107m\x1b[30mx\x1b[97m\x1b[40mB");
    }

    #[test]
    fn splice_nudges_in_ansi256_palette() {
        // 256-mode near-white-on-white cell: nudge emits the palette's
        // black (16), not a truecolor escape.
        let line = "\x1b[38;5;255m\x1b[48;5;231mAB";
        let out = splice(line, &[cell(0, 'x')]);
        assert_eq!(
            out,
            "\x1b[38;5;255m\x1b[48;5;231m\x1b[38;5;16mx\x1b[38;5;255mB"
        );
    }

    #[test]
    fn splice_swaps_inverted_cell_for_ink() {
        // Cell rendered white-on-black, but the word's ink is black —
        // the bg is the ink side, so the letter paints black-on-white
        // and the cell's own pair is restored after.
        let line = "\x1b[38;2;255;255;255m\x1b[48;2;10;10;10mAB";
        let ink = OverlayCell {
            col: 0,
            ch: 'x',
            ink: (0, 0, 0),
        };
        let out = splice(line, &[ink]);
        assert_eq!(
            out,
            "\x1b[38;2;255;255;255m\x1b[48;2;10;10;10m\
             \x1b[48;2;255;255;255m\x1b[38;2;10;10;10mx\
             \x1b[38;2;255;255;255m\x1b[48;2;10;10;10mB"
        );
    }

    #[test]
    fn splice_keeps_orientation_when_fg_is_ink_side() {
        // Cell already ink-on-paper (black fg, white bg) — no swap, no
        // extra escapes.
        let line = "\x1b[38;2;10;10;10m\x1b[48;2;255;255;255mAB";
        let ink = OverlayCell {
            col: 0,
            ch: 'x',
            ink: (0, 0, 0),
        };
        let out = splice(line, &[ink]);
        assert_eq!(out, "\x1b[38;2;10;10;10m\x1b[48;2;255;255;255mxB");
    }

    #[test]
    fn splice_swap_then_nudge_when_pair_lacks_contrast() {
        // Both cell colors near-white but bg infinitesimally nearer the
        // white ink: the swap alone leaves fg≈bg, so the contrast nudge
        // must still fire on the swapped pair (black letter on the
        // near-white bg).
        let line = "\x1b[38;2;240;240;240m\x1b[48;2;250;250;250mAB";
        let ink = OverlayCell {
            col: 0,
            ch: 'x',
            ink: (255, 255, 255),
        };
        let out = splice(line, &[ink]);
        assert_eq!(
            out,
            "\x1b[38;2;240;240;240m\x1b[48;2;250;250;250m\
             \x1b[48;2;240;240;240m\x1b[38;2;0;0;0mx\
             \x1b[38;2;240;240;240m\x1b[48;2;250;250;250mB"
        );
    }
}
