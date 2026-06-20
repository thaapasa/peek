//! The image info sections — Image (dimensions, colour, animation), plus EXIF
//! and XMP key/value blocks — driven by one [`ImageView`] that derives both
//! `serde::Serialize` (JSON) and [`InfoView`](crate::info::InfoView) (themed
//! print). [`ImageStats`] stays the gather struct; the view projects it.
//!
//! The Image stats flatten into the top-level object; EXIF / XMP nest under
//! their own keys. `Dimensions` prints one row but serializes as `width` +
//! `height`; `Megapixels` is print-only; animation prints inline rows but
//! serializes as a nested `animation` object.

use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Serialize, Serializer};

use serde_json::json;

use crate::info::{Accent, InfoNode, InfoValue, Role, Value};
use crate::types::image::info::{AnimationStats, ImageStats, LoopCount};
use peek_theme::{PeekTheme, lerp_color};

crate::info_section!(ImageStats, ImageView, "image");

#[derive(Serialize, crate::info::InfoView)]
struct ImageView {
    #[info(nest)]
    #[serde(flatten)]
    main: ImageMain,
    #[info(nest)]
    #[serde(rename = "exif", skip_serializing_if = "Pairs::is_empty")]
    exif: Pairs,
    #[info(nest)]
    #[serde(rename = "xmp", skip_serializing_if = "Pairs::is_empty")]
    xmp: Pairs,
}

#[derive(Serialize, crate::info::InfoView)]
#[info(title = "Image")]
struct ImageMain {
    #[info(label = "Dimensions")]
    #[serde(flatten)]
    dimensions: Dims,
    // Print-only — JSON has width/height already.
    #[info(label = "Megapixels")]
    #[serde(skip)]
    megapixels: Megapixels,
    #[info(label = "Color")]
    color_type: String,
    #[info(label = "Bit Depth", skip_if_zero)]
    bit_depth: Value,
    #[info(label = "ICC Profile")]
    #[serde(rename = "icc_profile", skip_serializing_if = "Option::is_none")]
    icc_profile: Option<String>,
    #[info(label = "HDR")]
    #[serde(rename = "hdr_format", skip_serializing_if = "Option::is_none")]
    hdr_format: Option<Accent>,
    #[info(nest)]
    #[serde(skip_serializing_if = "Option::is_none")]
    animation: Option<Anim>,
}

impl From<&ImageStats> for ImageView {
    fn from(s: &ImageStats) -> Self {
        ImageView {
            main: ImageMain {
                dimensions: Dims {
                    width: s.width,
                    height: s.height,
                },
                megapixels: Megapixels {
                    width: s.width,
                    height: s.height,
                },
                color_type: s.color_type.clone(),
                bit_depth: Value::split(
                    format!("{} bits/channel", s.bit_depth),
                    Role::Value,
                    json!(s.bit_depth),
                ),
                icc_profile: s.icc_profile.clone(),
                hdr_format: s.hdr_format.clone().map(Accent),
                animation: s.animation.as_ref().map(Anim::from),
            },
            exif: Pairs {
                title: "EXIF",
                pairs: s.exif.clone(),
            },
            xmp: Pairs {
                title: "XMP",
                pairs: s.xmp.clone(),
            },
        }
    }
}

/// Pixel dimensions. Print: `W × H` on a resolution gradient. JSON: `width` +
/// `height`.
struct Dims {
    width: u32,
    height: u32,
}
impl InfoValue for Dims {
    fn render_value(&self, theme: &PeekTheme) -> String {
        let mp = (self.width as f64 * self.height as f64) / 1_000_000.0;
        let color = if mp < 0.5 {
            lerp_color(theme.muted, theme.value, (mp * 2.0) as f32)
        } else if mp < 8.0 {
            theme.value
        } else {
            let t = ((mp / 8.0).clamp(1.0, 10.0).ln() / 10_f64.ln()) as f32;
            lerp_color(theme.value, theme.accent, t)
        };
        theme.paint(&format!("{} \u{00d7} {}", self.width, self.height), color)
    }
}
impl Serialize for Dims {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut st = ser.serialize_struct("dims", 2)?;
        st.serialize_field("width", &self.width)?;
        st.serialize_field("height", &self.height)?;
        st.end()
    }
}

/// Megapixel count (print-only).
struct Megapixels {
    width: u32,
    height: u32,
}
impl InfoValue for Megapixels {
    fn render_value(&self, theme: &PeekTheme) -> String {
        let mp = (self.width as f64 * self.height as f64) / 1_000_000.0;
        let text = if mp < 1.0 {
            format!("{mp:.2} MP")
        } else {
            format!("{mp:.1} MP")
        };
        theme.paint(&text, theme.value)
    }
}

/// Bit depth: JSON number always, print `N bits/channel` when nonzero.
/// Animation summary. Print: inline `Frames` / `Duration` / `Avg FPS` / `Loop`
/// rows. JSON: a nested `{ frame_count?, total_duration_ms?, loop_count? }`.
struct Anim {
    frame_count: Option<usize>,
    total_duration_ms: Option<u64>,
    loop_count: Option<LoopCount>,
}

impl From<&AnimationStats> for Anim {
    fn from(a: &AnimationStats) -> Self {
        Anim {
            frame_count: a.frame_count,
            total_duration_ms: a.total_duration_ms,
            loop_count: a.loop_count,
        }
    }
}

impl crate::info::InfoView for Anim {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        let mut nodes = Vec::new();
        if let Some(count) = self.frame_count {
            nodes.push(InfoNode::Row {
                label: "Frames".into(),
                value: theme.paint_value(&format!("{count} (animated)")),
            });
        }
        if let Some(ms) = self.total_duration_ms {
            let secs = ms as f64 / 1000.0;
            let label = if secs < 60.0 {
                format!("{secs:.2} s")
            } else {
                let mins = (secs / 60.0).floor();
                let rem = secs - mins * 60.0;
                format!("{mins:.0}m {rem:.2}s")
            };
            nodes.push(InfoNode::Row {
                label: "Duration".into(),
                value: theme.paint_value(&label),
            });
            if let Some(count) = self.frame_count
                && ms > 0
            {
                let fps = count as f64 / (ms as f64 / 1000.0);
                nodes.push(InfoNode::Row {
                    label: "Avg FPS".into(),
                    value: theme.paint_muted(&format!("{fps:.1}")),
                });
            }
        }
        if let Some(loops) = &self.loop_count {
            let text = match loops {
                LoopCount::Infinite | LoopCount::Finite(0) => "infinite".to_string(),
                LoopCount::Finite(1) => "play once".to_string(),
                LoopCount::Finite(n) => format!("{n} times"),
            };
            nodes.push(InfoNode::Row {
                label: "Loop".into(),
                value: theme.paint_value(&text),
            });
        }
        nodes
    }
}

impl Serialize for Anim {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let len = self.frame_count.is_some() as usize
            + self.total_duration_ms.is_some() as usize
            + self.loop_count.is_some() as usize;
        let mut st = ser.serialize_struct("animation", len)?;
        if let Some(fc) = self.frame_count {
            st.serialize_field("frame_count", &fc)?;
        }
        if let Some(ms) = self.total_duration_ms {
            st.serialize_field("total_duration_ms", &ms)?;
        }
        match self.loop_count {
            Some(LoopCount::Infinite) | Some(LoopCount::Finite(0)) => {
                st.serialize_field("loop_count", "infinite")?
            }
            Some(LoopCount::Finite(n)) => st.serialize_field("loop_count", &n)?,
            None => {}
        }
        st.end()
    }
}

/// A key/value block (EXIF or XMP). Print: a titled section of value rows.
/// JSON: a `{ key: value }` object.
struct Pairs {
    title: &'static str,
    pairs: Vec<(String, String)>,
}
impl Pairs {
    fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}
impl crate::info::InfoView for Pairs {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        if self.pairs.is_empty() {
            return Vec::new();
        }
        let body = self
            .pairs
            .iter()
            .map(|(k, v)| InfoNode::Row {
                label: k.clone().into(),
                value: theme.paint_value(v),
            })
            .collect();
        vec![InfoNode::Block {
            title: self.title.to_string(),
            body,
        }]
    }
}
impl Serialize for Pairs {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut m = ser.serialize_map(Some(self.pairs.len()))?;
        for (k, v) in &self.pairs {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}
