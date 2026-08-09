//! The CSS info sections — Content (text stats, print-only), a CSS block, and
//! a Colors swatch grid — driven by one [`CssView`] that derives both
//! `serde::Serialize` (JSON) and [`InfoView`](crate::info::InfoView) (themed
//! print). [`CssInfo`] stays the gather struct; the view projects it.
//!
//! The CSS + Colors stats flatten into one flat JSON object (the Content block
//! is print-only). The selector breakdown, `@import` list, and colour palette
//! each print specially (a composite row, count + URL rows, a swatch grid of
//! `Line`s) while serializing as their structured JSON.

use peek_theme::PeekTheme;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use syntect::highlighting::Color;

use crate::info::{InfoNode, InfoValue, Value, paint_count};
use crate::types::css::info::{CssInfo, CssStats, SelectorKindCounts};
use crate::types::text::info_render::TextView;

/// Swatches per row in the palette grid.
const SWATCH_COLS: usize = 4;

crate::info_section!(CssInfo, CssView, "css");

#[derive(Serialize, crate::info::InfoView)]
struct CssView {
    // Print-only Content block; CSS JSON carries just the CSS/Colors stats.
    #[info(nest)]
    #[serde(skip)]
    content: TextView,
    #[info(nest)]
    #[serde(flatten)]
    css: CssSection,
    #[info(nest)]
    #[serde(flatten)]
    colors: Colors,
}

#[derive(Serialize, crate::info::InfoView)]
#[info(title = "CSS")]
struct CssSection {
    #[info(label = "Rules")]
    rule_count: Value,
    #[info(label = "Selectors")]
    #[serde(flatten)]
    selectors: Selectors,
    #[info(label = "Custom Props")]
    #[serde(
        rename = "custom_property_count",
        skip_serializing_if = "Option::is_none"
    )]
    custom_property_count: Option<Value>,
    #[info(label = "@media")]
    #[serde(rename = "media_query_count", skip_serializing_if = "Option::is_none")]
    media_query_count: Option<Value>,
    #[info(label = "@keyframes")]
    #[serde(rename = "keyframes_count", skip_serializing_if = "Option::is_none")]
    keyframes_count: Option<Value>,
    #[info(nest)]
    #[serde(flatten)]
    imports: Imports,
}

impl From<&CssInfo> for CssView {
    fn from(info: &CssInfo) -> Self {
        let s: &CssStats = &info.stats;
        let opt = |n: usize| (n > 0).then(|| Value::count(n as u64));
        CssView {
            content: TextView::from(&info.text),
            css: CssSection {
                rule_count: Value::count(s.rule_count as u64),
                selectors: Selectors {
                    count: s.selector_count,
                    kinds: copy_kinds(&s.selector_kinds),
                },
                custom_property_count: opt(s.custom_property_count),
                media_query_count: opt(s.media_query_count),
                keyframes_count: opt(s.keyframes_count),
                imports: Imports(
                    s.imports
                        .iter()
                        .map(|i| (i.url.clone(), i.external))
                        .collect(),
                ),
            },
            colors: Colors {
                palette: s
                    .palette
                    .iter()
                    .map(|c| (c.rgb, c.hex.clone(), c.count))
                    .collect(),
                total_colors: s.total_colors,
            },
        }
    }
}

/// `(json_key, print_label, count)` per selector kind, in display order.
type Kinds = [(&'static str, &'static str, usize); 6];

fn copy_kinds(k: &SelectorKindCounts) -> Kinds {
    [
        ("class", "class", k.class),
        ("id", "id", k.id),
        ("element", "element", k.element),
        ("pseudo", "pseudo", k.pseudo),
        ("attribute", "attr", k.attribute),
        ("universal", "universal", k.universal),
    ]
}

/// Selector tally. Print: a count plus a muted `(3 class · 1 id)` breakdown.
/// JSON: `selector_count` + a `selector_kinds` object (non-zero kinds only).
struct Selectors {
    count: usize,
    kinds: Kinds,
}

impl InfoValue for Selectors {
    fn render_value(&self, theme: &PeekTheme) -> String {
        let mut value = paint_count(self.count, theme);
        let parts: Vec<String> = self
            .kinds
            .iter()
            .filter(|(_, _, n)| *n > 0)
            .map(|(_, label, n)| format!("{n} {label}"))
            .collect();
        if !parts.is_empty() {
            value.push_str("  ");
            value.push_str(&theme.paint_muted(&format!("({})", parts.join(" \u{00b7} "))));
        }
        value
    }
}

impl Serialize for Selectors {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let present: Vec<(&'static str, usize)> = self
            .kinds
            .iter()
            .filter(|(_, _, n)| *n > 0)
            .map(|(key, _, n)| (*key, *n))
            .collect();
        let len = 1 + !present.is_empty() as usize;
        let mut st = ser.serialize_struct("selectors", len)?;
        st.serialize_field("selector_count", &self.count)?;
        if !present.is_empty() {
            st.serialize_field("selector_kinds", &KindMap(&present))?;
        }
        st.end()
    }
}

/// Serializes the non-zero selector kinds as an object.
struct KindMap<'a>(&'a [(&'static str, usize)]);
impl Serialize for KindMap<'_> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = ser.serialize_map(Some(self.0.len()))?;
        for (k, v) in self.0 {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

/// `@import` rules. Print: a count plus one indented `URL` row each (external
/// URLs in warning style). JSON: an `imports` array of `{ url, external }`.
struct Imports(Vec<(String, bool)>);

impl crate::info::InfoView for Imports {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        if self.0.is_empty() {
            return Vec::new();
        }
        let mut nodes = vec![InfoNode::Row {
            label: "@import".into(),
            value: paint_count(self.0.len(), theme),
        }];
        for (url, external) in &self.0 {
            // `@import` URLs are file-controlled; strip terminal controls.
            let url = peek_io::sanitize_terminal_controls(url);
            let value = if *external {
                theme.paint(&format!(" {url}  (external)"), theme.warning)
            } else {
                theme.paint_muted(&url)
            };
            nodes.push(InfoNode::Row {
                label: "  URL".into(),
                value,
            });
        }
        nodes
    }
}

impl Serialize for Imports {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        if self.0.is_empty() {
            return ser.serialize_struct("imports", 0)?.end();
        }
        let arr: Vec<ImportJson> = self
            .0
            .iter()
            .map(|(url, external)| ImportJson {
                url,
                external: *external,
            })
            .collect();
        let mut st = ser.serialize_struct("imports", 1)?;
        st.serialize_field("imports", &arr)?;
        st.end()
    }
}

#[derive(Serialize)]
struct ImportJson<'a> {
    url: &'a str,
    external: bool,
}

/// The colour palette. Print: a `Colors` block — a `Palette` summary row then
/// a swatch grid. JSON: a `palette` array + `total_colors`.
struct Colors {
    palette: Vec<((u8, u8, u8), String, usize)>,
    total_colors: usize,
}

impl crate::info::InfoView for Colors {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        if self.palette.is_empty() {
            return Vec::new();
        }
        let shown = self.palette.len();
        let label = if self.total_colors > shown {
            format!("{shown} of {} colors", self.total_colors)
        } else {
            format!("{shown} color{}", if shown == 1 { "" } else { "s" })
        };
        let mut body = vec![InfoNode::Row {
            label: "Palette".into(),
            value: theme.paint_value(&label),
        }];
        for row in self.palette.chunks(SWATCH_COLS) {
            body.push(InfoNode::Line(swatch_row(row, theme)));
        }
        vec![InfoNode::Block {
            title: "Colors".to_string(),
            body,
        }]
    }
}

impl Serialize for Colors {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        if self.palette.is_empty() {
            return ser.serialize_struct("colors", 0)?.end();
        }
        let arr: Vec<SwatchJson> = self
            .palette
            .iter()
            .map(|(rgb, hex, count)| SwatchJson {
                hex,
                rgb: [rgb.0, rgb.1, rgb.2],
                count: *count,
            })
            .collect();
        let mut st = ser.serialize_struct("colors", 2)?;
        st.serialize_field("palette", &arr)?;
        st.serialize_field("total_colors", &self.total_colors)?;
        st.end()
    }
}

#[derive(Serialize)]
struct SwatchJson<'a> {
    hex: &'a str,
    rgb: [u8; 3],
    count: usize,
}

fn swatch_row(row: &[((u8, u8, u8), String, usize)], theme: &PeekTheme) -> String {
    let mut line = String::from("    ");
    for (rgb, hex, _) in row {
        let color = Color {
            r: rgb.0,
            g: rgb.1,
            b: rgb.2,
            a: 0xFF,
        };
        line.push_str(&theme.paint("\u{2588}\u{2588}\u{2588}", color));
        line.push(' ');
        line.push_str(&theme.paint_muted(hex));
        line.push_str("  ");
    }
    line
}
