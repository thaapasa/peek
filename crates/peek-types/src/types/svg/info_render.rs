//! The SVG info section: an SVG block plus a Source block (shared text
//! stats), driven by one [`SvgView`] that derives both `serde::Serialize`
//! (JSON) and [`InfoView`](crate::info::InfoView) (themed print).
//! [`SvgStats`] stays the gather struct; the view projects it.
//!
//! The SVG stats flatten into the top-level object; the Source block nests
//! under `"source"`. Security flags print a warning ` yes` only when set;
//! animation prints one composite row but serializes as an `animation` object
//! or an `animation_warning` string.

use peek_theme::PeekTheme;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::info::{InfoNode, InfoValue, Value};
use crate::types::svg::info::SvgStats;
use crate::types::text::info_render::TextView;

crate::info_section!(SvgStats, SvgView, "svg");

#[derive(Serialize, crate::info::InfoView)]
struct SvgView {
    #[info(nest)]
    #[serde(flatten)]
    svg: SvgSection,
    #[info(nest)]
    #[serde(rename = "source")]
    content: Source,
}

/// The shared text stats, but titled `Source` (not `Content`) for SVG.
struct Source(TextView);

impl crate::info::InfoView for Source {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        let mut nodes = self.0.info_nodes(theme);
        for node in &mut nodes {
            if let InfoNode::Block { title, .. } = node {
                "Source".clone_into(title);
            }
        }
        nodes
    }
}

impl Serialize for Source {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(ser)
    }
}

#[derive(Serialize, crate::info::InfoView)]
#[info(title = "SVG")]
struct SvgSection {
    #[info(label = "viewBox")]
    #[serde(rename = "view_box", skip_serializing_if = "Option::is_none")]
    view_box: Option<String>,
    #[info(label = "Width")]
    #[serde(rename = "declared_width", skip_serializing_if = "Option::is_none")]
    declared_width: Option<String>,
    #[info(label = "Height")]
    #[serde(rename = "declared_height", skip_serializing_if = "Option::is_none")]
    declared_height: Option<String>,
    #[info(label = "Paths", skip_if_zero)]
    path_count: Value,
    #[info(label = "Groups", skip_if_zero)]
    group_count: Value,
    #[info(label = "Rects", skip_if_zero)]
    rect_count: Value,
    #[info(label = "Circles", skip_if_zero)]
    circle_count: Value,
    #[info(label = "Text Elems", skip_if_zero)]
    text_count: Value,
    #[info(label = "Script", skip_if = "Flag::clear")]
    has_script: Flag,
    #[info(label = "External ref", skip_if = "Flag::clear")]
    has_external_href: Flag,
    #[info(nest)]
    #[serde(flatten)]
    animation: Animation,
}

impl From<&SvgStats> for SvgView {
    fn from(s: &SvgStats) -> Self {
        SvgView {
            svg: SvgSection {
                view_box: s.view_box.clone(),
                declared_width: s.declared_width.clone(),
                declared_height: s.declared_height.clone(),
                path_count: Value::count(s.path_count as u64),
                group_count: Value::count(s.group_count as u64),
                rect_count: Value::count(s.rect_count as u64),
                circle_count: Value::count(s.circle_count as u64),
                text_count: Value::count(s.text_count as u64),
                has_script: Flag(s.has_script),
                has_external_href: Flag(s.has_external_href),
                animation: Animation {
                    anim: s.animation.as_ref().map(|a| AnimSummary {
                        frame_count: a.frame_count,
                        total_duration_ms: a.total_duration_ms,
                        infinite: a.infinite,
                    }),
                    warning: s.animation_warning.clone(),
                },
            },
            content: Source(TextView::from(&s.text)),
        }
    }
}

/// A security flag: JSON bool always, print a warning ` yes` only when set.
struct Flag(bool);
impl Flag {
    fn clear(&self) -> bool {
        !self.0
    }
}
impl InfoValue for Flag {
    fn render_value(&self, theme: &PeekTheme) -> String {
        theme.paint(" yes", theme.warning)
    }
}
impl Serialize for Flag {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_bool(self.0)
    }
}

/// Plain copy of [`SvgAnimationStats`] for serde.
struct AnimSummary {
    frame_count: usize,
    total_duration_ms: u64,
    infinite: bool,
}

/// Animation summary. Print: one `Animation` row (a playable summary or a
/// warning), or nothing. JSON: an `animation` object when playable, else an
/// `animation_warning` string.
struct Animation {
    anim: Option<AnimSummary>,
    warning: Option<String>,
}

impl crate::info::InfoView for Animation {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        if let Some(a) = &self.anim {
            let dur_s = a.total_duration_ms as f64 / 1000.0;
            let label = if a.infinite { "looping" } else { "one-shot" };
            let value = format!("{} frames, {:.2}s ({label})", a.frame_count, dur_s);
            vec![InfoNode::Row {
                label: "Animation".into(),
                value: theme.paint_value(&value),
            }]
        } else if let Some(reason) = &self.warning {
            vec![InfoNode::Row {
                label: "Animation".into(),
                value: theme.paint(&format!(" {reason}"), theme.warning),
            }]
        } else {
            Vec::new()
        }
    }
}

impl Serialize for Animation {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        // 0–2 keys: `animation` (when playable) and/or `animation_warning`.
        let len = self.anim.is_some() as usize + self.warning.is_some() as usize;
        let mut st = ser.serialize_struct("animation", len)?;
        if let Some(a) = &self.anim {
            st.serialize_field("animation", &AnimJson(a))?;
        }
        if let Some(reason) = &self.warning {
            st.serialize_field("animation_warning", reason)?;
        }
        st.end()
    }
}

/// Serializes an [`AnimSummary`] as `{ frame_count, total_duration_ms, infinite }`.
struct AnimJson<'a>(&'a AnimSummary);
impl Serialize for AnimJson<'_> {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut st = ser.serialize_struct("animation", 3)?;
        st.serialize_field("frame_count", &self.0.frame_count)?;
        st.serialize_field("total_duration_ms", &self.0.total_duration_ms)?;
        st.serialize_field("infinite", &self.0.infinite)?;
        st.end()
    }
}
