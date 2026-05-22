//! Render the CSS info sections: text stats, a CSS-specific block
//! (rules / selectors / at-rules / `@import` list), and a colour-swatch
//! grid.

use syntect::highlighting::Color;

use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;
use crate::types::css::info::{ColorSwatch, CssImport, CssInfo, CssStats, SelectorKindCounts};
use crate::types::text::info_render::push_text_stats;

/// Swatches per row in the palette grid.
const SWATCH_COLS: usize = 4;

pub fn render_section(lines: &mut Vec<String>, info: &CssInfo, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Content", theme);
    push_text_stats(lines, &info.text, theme);

    push_css_section(lines, &info.stats, theme);
    push_colors_section(lines, &info.stats, theme);
}

fn push_css_section(lines: &mut Vec<String>, stats: &CssStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "CSS", theme);

    push_field(lines, "Rules", &paint_count(stats.rule_count, theme), theme);

    let mut selectors = paint_count(stats.selector_count, theme);
    let breakdown = selector_breakdown(&stats.selector_kinds, theme);
    if !breakdown.is_empty() {
        selectors.push_str("  ");
        selectors.push_str(&breakdown);
    }
    push_field(lines, "Selectors", &selectors, theme);

    if stats.custom_property_count > 0 {
        push_field(
            lines,
            "Custom Props",
            &paint_count(stats.custom_property_count, theme),
            theme,
        );
    }
    if stats.media_query_count > 0 {
        push_field(
            lines,
            "@media",
            &paint_count(stats.media_query_count, theme),
            theme,
        );
    }
    if stats.keyframes_count > 0 {
        push_field(
            lines,
            "@keyframes",
            &paint_count(stats.keyframes_count, theme),
            theme,
        );
    }
    push_imports(lines, &stats.imports, theme);
}

/// `(3 class · 1 id · 2 element)` — only the kinds actually present.
fn selector_breakdown(k: &SelectorKindCounts, theme: &PeekTheme) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut add = |count: usize, label: &str| {
        if count > 0 {
            parts.push(format!("{count} {label}"));
        }
    };
    add(k.class, "class");
    add(k.id, "id");
    add(k.element, "element");
    add(k.pseudo, "pseudo");
    add(k.attribute, "attr");
    add(k.universal, "universal");

    if parts.is_empty() {
        String::new()
    } else {
        theme.paint_muted(&format!("({})", parts.join(" \u{00b7} ")))
    }
}

fn push_imports(lines: &mut Vec<String>, imports: &[CssImport], theme: &PeekTheme) {
    if imports.is_empty() {
        return;
    }
    push_field(lines, "@import", &paint_count(imports.len(), theme), theme);
    for imp in imports {
        let value = if imp.external {
            theme.paint(&format!(" {}  (external)", imp.url), theme.warning)
        } else {
            theme.paint_muted(&imp.url)
        };
        push_field(lines, "  URL", &value, theme);
    }
}

fn push_colors_section(lines: &mut Vec<String>, stats: &CssStats, theme: &PeekTheme) {
    if stats.palette.is_empty() {
        return;
    }
    lines.push(String::new());
    push_section_header(lines, "Colors", theme);

    let shown = stats.palette.len();
    let label = if stats.total_colors > shown {
        format!("{shown} of {} colors", stats.total_colors)
    } else {
        format!("{shown} color{}", if shown == 1 { "" } else { "s" })
    };
    push_field(lines, "Palette", &theme.paint_value(&label), theme);

    for row in stats.palette.chunks(SWATCH_COLS) {
        lines.push(swatch_row(row, theme));
    }
}

fn swatch_row(row: &[ColorSwatch], theme: &PeekTheme) -> String {
    let mut line = String::from("    ");
    for sw in row {
        let color = Color {
            r: sw.rgb.0,
            g: sw.rgb.1,
            b: sw.rgb.2,
            a: 0xFF,
        };
        line.push_str(&theme.paint("\u{2588}\u{2588}\u{2588}", color));
        line.push(' ');
        line.push_str(&theme.paint_muted(&sw.hex));
        line.push_str("  ");
    }
    line
}
