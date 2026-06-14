//! Row-painting primitives shared by the file-shaped listing sources —
//! [`super::tree_source::TreeListSource`] for container TOCs and the
//! directory listing for on-disk browsing. Both render identical columns
//! (perms, size, mtime, name), so column widths, palette, and formatters
//! live here and stay in sync by construction.

use syntect::highlighting::Color;

use crate::info::{format_archive_mtime_zoned, format_size_human, thousands_sep};
use crate::theme::{PeekTheme, lerp_color};

/// Width (chars) of the size column, including thousands separators.
pub const SIZE_COL_WIDTH: usize = 12;
/// Width (chars) of the permissions column. 10-char `drwxr-xr-x` form.
pub const PERMS_COL_WIDTH: usize = 10;
/// Below this terminal width the mtime column is dropped to leave room
/// for the path.
pub const MTIME_HIDE_BELOW_COLS: usize = 80;

/// What kind of value sits in the size cell. Directories render as
/// "-", failed stats as "?", real byte counts as a thousands-separated
/// integer. Picked by the caller because the source data shapes
/// differ (archive entries don't have `stat_error`, on-disk entries
/// do).
pub enum SizeCell {
    Dir,
    Bytes(u64),
    Unknown,
}

/// Render the 10-char `drwxr-xr-x`-style permission string. Caller
/// supplies the type character (`'d'` / `'-'` / `'l'` / `'?'`); when
/// `mode` is unset (sources that don't carry mode bits at all, or
/// implicit tree parents), fall back to typical defaults — `rwxr-xr-x`
/// for dirs, `rw-r--r--` for files — so the column stays informative
/// instead of dissolving into a wall of `?`s.
pub fn format_perms(type_ch: char, mode: Option<u32>, is_dir: bool) -> String {
    let mode = mode.unwrap_or(if is_dir { 0o755 } else { 0o644 });
    let mut s = String::with_capacity(PERMS_COL_WIDTH);
    s.push(type_ch);
    for (r, w, x) in [
        (0o400, 0o200, 0o100),
        (0o040, 0o020, 0o010),
        (0o004, 0o002, 0o001),
    ] {
        s.push(if mode & r != 0 { 'r' } else { '-' });
        s.push(if mode & w != 0 { 'w' } else { '-' });
        s.push(if mode & x != 0 { 'x' } else { '-' });
    }
    s
}

/// Paint a perms string with per-char colors and the dim separator
/// between owner/group/other triplets.
pub fn paint_perms(perms: &str, theme: &PeekTheme) -> String {
    let mut out = String::new();
    for (i, ch) in perms.chars().enumerate() {
        let color = match ch {
            'r' => theme.value,
            'w' => theme.accent,
            'x' => theme.heading,
            'd' | 'l' => theme.heading,
            '-' => lerp_color(theme.muted, theme.background, 0.3),
            _ => theme.foreground,
        };
        out.push_str(&theme.paint(&ch.to_string(), color));
        if (i == 3 || i == 6) && i + 1 < PERMS_COL_WIDTH {
            out.push_str(&theme.paint("\u{2500}", lerp_color(theme.muted, theme.background, 0.5)));
        }
    }
    out
}

/// Right-pad the size-cell raw text to [`SIZE_COL_WIDTH`].
pub fn pad_size(raw: &str) -> String {
    format!("{raw:>w$}", w = SIZE_COL_WIDTH)
}

/// Format a size cell, padded to [`SIZE_COL_WIDTH`]. `human` picks
/// human-readable units (KiB/MiB/GiB) over exact thousands-separated
/// bytes — driven by the runtime size-unit toggle.
pub fn format_size(cell: SizeCell, human: bool) -> String {
    match cell {
        SizeCell::Dir => pad_size("-"),
        SizeCell::Bytes(n) if human => pad_size(&format_size_human(n)),
        SizeCell::Bytes(n) => pad_size(&thousands_sep(n)),
        SizeCell::Unknown => pad_size("?"),
    }
}

/// Paint a size cell. Dirs and zero-byte entries render muted; real
/// byte counts get a value→accent gradient by [`size_color`].
pub fn paint_size(text: &str, size: u64, is_dir: bool, theme: &PeekTheme) -> String {
    if is_dir || size == 0 {
        theme.paint(text, theme.muted)
    } else {
        theme.paint(text, size_color(size, theme))
    }
}

/// Map byte count to a colour on the value→accent gradient: tiny files
/// fade toward muted, mid-range files stay on `value`, large files
/// climb toward `accent`.
pub fn size_color(bytes: u64, theme: &PeekTheme) -> Color {
    let kb = bytes as f64 / 1024.0;
    if kb < 1.0 {
        lerp_color(theme.muted, theme.value, (kb as f32).max(0.2))
    } else if kb < 1024.0 {
        theme.value
    } else {
        let t = ((kb / 1024.0).ln() / 100_f64.ln()) as f32;
        lerp_color(theme.value, theme.accent, t.clamp(0.0, 1.0))
    }
}

/// Format an mtime cell from an epoch-second timestamp using the
/// archive-style "YYYY-MM-DD HH:MM" formatter.
pub fn format_mtime_epoch(secs: u64, utc: bool) -> String {
    format_archive_mtime_zoned(secs, utc)
}

/// Two-cell caret prefix marking the selected row — paired with a
/// 2-space gutter on non-selected rows ([`ROW_GUTTER`]) so columns
/// stay aligned across the viewport.
pub fn paint_selected_marker(line: &str, theme: &PeekTheme) -> String {
    let marker = theme.paint("\u{25b8} ", theme.accent);
    format!("{marker}{line}")
}

/// Two-space gutter for non-selected rows, matching the width of
/// [`paint_selected_marker`]'s caret prefix.
pub const ROW_GUTTER: &str = "  ";

/// Assemble the row layout: `perms  size  [mtime  ]name`. Painters
/// must be applied before calling this — args are pre-painted strings.
/// `mtime` carries `(left-padded-text, _column_width_hint)`; when
/// `None`, the mtime column is omitted entirely (narrow terminals).
///
/// The 2-space column gutter is the single source of truth — keeps
/// ListingMode and DirectoryMode visually aligned by construction.
pub fn compose_row(
    painted_perms: &str,
    painted_size: &str,
    painted_mtime: Option<&str>,
    painted_name: &str,
) -> String {
    match painted_mtime {
        Some(mtime) => format!("{painted_perms}  {painted_size}  {mtime}  {painted_name}"),
        None => format!("{painted_perms}  {painted_size}  {painted_name}"),
    }
}

/// Apply selection marker or its gutter to a composed row. Picks
/// between [`paint_selected_marker`] and [`ROW_GUTTER`] so callers
/// don't have to.
pub fn with_marker(row: &str, selected: bool, theme: &PeekTheme) -> String {
    if selected {
        paint_selected_marker(row, theme)
    } else {
        format!("{ROW_GUTTER}{row}")
    }
}

/// Widest stringified mtime in the iterator, or 0 when empty. Padding
/// to this width keeps the path column flush against the mtime column
/// across a slice with varying mtime lengths.
pub fn mtime_column_width<I, S>(iter: I) -> usize
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    iter.into_iter()
        .map(|s| s.as_ref().len())
        .max()
        .unwrap_or(0)
}

/// Paint an mtime cell, left-padded to `width`.
pub fn paint_mtime(text: &str, width: usize, theme: &PeekTheme) -> String {
    let padded = format!("{text:<width$}");
    theme.paint(&padded, theme.muted)
}

/// Assemble the left-column cells (perms, size, and — on wide enough
/// terminals — mtime) for the file-shaped listing sources. Each source
/// formats its own per-type pieces (perms/size text from its data shape)
/// and hands them in; this owns the column order and the
/// [`MTIME_HIDE_BELOW_COLS`] gating so the tree and directory listings
/// can't drift. `mtime_text` is lazy — not formatted on narrow terminals
/// where the column is dropped.
// Eight straight-line cell inputs (source-formatted pieces + render
// context); grouping them into a struct would only be destructured back
// out at the two call sites.
#[allow(clippy::too_many_arguments)]
pub fn file_row_left(
    perms: &str,
    size: &str,
    size_bytes: u64,
    is_dir: bool,
    mtime_width: usize,
    term_cols: usize,
    theme: &PeekTheme,
    mtime_text: impl FnOnce() -> String,
) -> Vec<String> {
    let mut left = vec![
        paint_perms(perms, theme),
        paint_size(size, size_bytes, is_dir, theme),
    ];
    if term_cols >= MTIME_HIDE_BELOW_COLS {
        left.push(paint_mtime(&mtime_text(), mtime_width, theme));
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{PeekThemeName, StyleMode, ThemeManager};

    fn plain_theme() -> ThemeManager {
        ThemeManager::new(PeekThemeName::IdeaDark, StyleMode::Plain)
    }

    #[test]
    fn format_size_honours_human_toggle() {
        // Exact bytes (thousands-separated) vs human-readable units, both
        // right-padded to the fixed column width.
        assert_eq!(format_size(SizeCell::Bytes(2956), false).trim(), "2,956");
        assert_eq!(format_size(SizeCell::Bytes(2956), true).trim(), "2.89 KiB");
        assert_eq!(
            format_size(SizeCell::Bytes(5 * 1024 * 1024), true).trim(),
            "5.00 MiB"
        );
        // Dir / unknown cells are unit-agnostic.
        assert_eq!(format_size(SizeCell::Dir, true).trim(), "-");
        assert_eq!(format_size(SizeCell::Unknown, true).trim(), "?");
    }

    /// The mtime column is the only width-gated cell: present at and above
    /// [`MTIME_HIDE_BELOW_COLS`], dropped below it. Pin the breakpoint here
    /// so both file-shaped sources stay aligned through the one helper.
    #[test]
    fn mtime_column_gated_on_term_width() {
        let tm = plain_theme();
        let theme = tm.peek_theme();
        let perms = format_perms('-', None, false);
        let size = format_size(SizeCell::Bytes(42), false);
        let mtime = || "2026-06-10 12:00".to_string();

        let wide = file_row_left(
            &perms,
            &size,
            42,
            false,
            16,
            MTIME_HIDE_BELOW_COLS,
            theme,
            mtime,
        );
        assert_eq!(wide.len(), 3, "perms + size + mtime at the breakpoint");

        let narrow = file_row_left(
            &perms,
            &size,
            42,
            false,
            16,
            MTIME_HIDE_BELOW_COLS - 1,
            theme,
            mtime,
        );
        assert_eq!(
            narrow.len(),
            2,
            "mtime dropped one column below the breakpoint"
        );
    }

    /// The lazy `mtime_text` closure must not run on narrow terminals where
    /// the column is dropped — guards the small allocation per visible row.
    #[test]
    fn mtime_text_not_formatted_when_column_hidden() {
        let tm = plain_theme();
        let theme = tm.peek_theme();
        let perms = format_perms('-', None, false);
        let size = format_size(SizeCell::Bytes(0), false);
        let mut called = false;
        file_row_left(
            &perms,
            &size,
            0,
            true,
            16,
            MTIME_HIDE_BELOW_COLS - 1,
            theme,
            || {
                called = true;
                String::new()
            },
        );
        assert!(!called);
    }
}
