use super::super::{AnimationStats, LoopCount};
use super::{push_field, push_section_header};
use crate::theme::{PeekTheme, lerp_color};

#[allow(clippy::too_many_arguments)]
pub(super) fn render_section(
    lines: &mut Vec<String>,
    width: u32,
    height: u32,
    color_type: &str,
    bit_depth: u8,
    hdr_format: Option<&str>,
    icc_profile: Option<&str>,
    animation: Option<&AnimationStats>,
    exif: &[(String, String)],
    xmp: &[(String, String)],
    theme: &PeekTheme,
) {
    lines.push(String::new());
    push_section_header(lines, "Image", theme);
    push_field(
        lines,
        "Dimensions",
        &paint_dimensions(width, height, theme),
        theme,
    );
    push_field(
        lines,
        "Megapixels",
        &paint_megapixels(width, height, theme),
        theme,
    );
    push_field(lines, "Color", &theme.paint_value(color_type), theme);
    if bit_depth > 0 {
        push_field(
            lines,
            "Bit Depth",
            &theme.paint_value(&format!("{bit_depth} bits/channel")),
            theme,
        );
    }
    if let Some(icc) = icc_profile {
        push_field(lines, "ICC Profile", &theme.paint_value(icc), theme);
    }
    if let Some(hdr) = hdr_format {
        push_field(lines, "HDR", &theme.paint_accent(hdr), theme);
    }
    if let Some(anim) = animation {
        push_animation(lines, anim, theme);
    }

    if !exif.is_empty() {
        lines.push(String::new());
        push_section_header(lines, "EXIF", theme);
        for (label, value) in exif {
            push_field(lines, label, &theme.paint_value(value), theme);
        }
    }

    if !xmp.is_empty() {
        lines.push(String::new());
        push_section_header(lines, "XMP", theme);
        for (label, value) in xmp {
            push_field(lines, label, &theme.paint_value(value), theme);
        }
    }
}

/// Paint image dimensions with resolution-based coloring.
fn paint_dimensions(width: u32, height: u32, theme: &PeekTheme) -> String {
    let megapixels = (width as f64 * height as f64) / 1_000_000.0;
    let color = if megapixels < 0.5 {
        lerp_color(theme.muted, theme.value, (megapixels * 2.0) as f32)
    } else if megapixels < 8.0 {
        theme.value
    } else {
        let t = ((megapixels / 8.0).clamp(1.0, 10.0).ln() / 10_f64.ln()) as f32;
        lerp_color(theme.value, theme.accent, t)
    };
    theme.paint(&format!("{width} \u{00d7} {height}"), color)
}

fn paint_megapixels(width: u32, height: u32, theme: &PeekTheme) -> String {
    let mp = (width as f64 * height as f64) / 1_000_000.0;
    let text = if mp < 1.0 {
        format!("{mp:.2} MP")
    } else {
        format!("{mp:.1} MP")
    };
    theme.paint(&text, theme.value)
}

fn push_animation(lines: &mut Vec<String>, anim: &AnimationStats, theme: &PeekTheme) {
    if let Some(count) = anim.frame_count {
        push_field(
            lines,
            "Frames",
            &theme.paint_value(&format!("{count} (animated)")),
            theme,
        );
    }
    if let Some(ms) = anim.total_duration_ms {
        let secs = ms as f64 / 1000.0;
        let label = if secs < 60.0 {
            format!("{secs:.2} s")
        } else {
            let mins = (secs / 60.0).floor();
            let rem = secs - mins * 60.0;
            format!("{mins:.0}m {rem:.2}s")
        };
        push_field(lines, "Duration", &theme.paint_value(&label), theme);
        if let Some(count) = anim.frame_count
            && ms > 0
        {
            let fps = count as f64 / (ms as f64 / 1000.0);
            push_field(
                lines,
                "Avg FPS",
                &theme.paint_muted(&format!("{fps:.1}")),
                theme,
            );
        }
    }
    if let Some(loops) = &anim.loop_count {
        let text = match loops {
            LoopCount::Infinite => "infinite".to_string(),
            LoopCount::Finite(0) => "infinite".to_string(),
            LoopCount::Finite(1) => "play once".to_string(),
            LoopCount::Finite(n) => format!("{n} times"),
        };
        push_field(lines, "Loop", &theme.paint_value(&text), theme);
    }
}
