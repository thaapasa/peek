//! CSS info shape: text-stats sidecar plus the CSS-specific scanner
//! output (rule / selector / at-rule counts, `@import` list, colour
//! palette).

use crate::types::text::info::TextStats;

pub struct CssInfo {
    pub text: TextStats,
    pub stats: CssStats,
}

pub struct CssStats {
    /// Style (qualified) rules — `selector { … }`. Excludes at-rules and
    /// `@keyframes` stops.
    pub rule_count: usize,
    /// Comma-separated selectors summed across every style rule.
    pub selector_count: usize,
    pub selector_kinds: SelectorKindCounts,
    /// Distinct `--custom-property` names declared.
    pub custom_property_count: usize,
    pub media_query_count: usize,
    pub keyframes_count: usize,
    /// `@import` rules, in source order.
    pub imports: Vec<CssImport>,
    /// Deduped colours, most-frequent first. Capped at a display limit;
    /// see `total_colors` for the true distinct count.
    pub palette: Vec<ColorSwatch>,
    /// Distinct colours found before the palette was capped.
    pub total_colors: usize,
}

/// Per-kind occurrence counts across all style-rule selectors. Counts
/// occurrences, not selectors — `.a.b` contributes 2 to `class`.
#[derive(Default)]
pub struct SelectorKindCounts {
    pub class: usize,
    pub id: usize,
    pub element: usize,
    pub pseudo: usize,
    pub attribute: usize,
    pub universal: usize,
}

pub struct CssImport {
    pub url: String,
    /// True for absolute / protocol-relative URLs (`http(s)://`, `//`,
    /// `ftp://`) — surfaced in the warning style like the SVG viewer's
    /// external-reference row.
    pub external: bool,
}

pub struct ColorSwatch {
    pub rgb: (u8, u8, u8),
    /// Canonical `#rrggbb` label.
    pub hex: String,
    /// Occurrences across all declaration values.
    pub count: usize,
}
