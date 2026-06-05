pub mod print;

pub use print::PrintOutput;

use crate::theme::{PeekTheme, lerp_color};

/// The peek wordmark, shown on the About view and the CLI help/version
/// screens. One row per element.
pub const LOGO: &[&str] = &[
    r"                 __  ",
    r"   ___  ___ ___ / /__",
    r"  / _ \/ -_) -_)  '_/",
    r" / .__/\__/\__/_/\_\ ",
    concat!(r"/_/  any file v", env!("CARGO_PKG_VERSION")),
];

/// Paint the [`LOGO`] with a per-character gradient between the theme's
/// `value` and `heading` colors. One returned String per logo line.
/// Shared by the foundation's About mode and the binary's help screen.
pub fn paint_logo(pt: &PeekTheme) -> Vec<String> {
    let total_width = LOGO.iter().map(|l| l.len()).max().unwrap_or(0);
    LOGO.iter()
        .map(|line| paint_gradient_line(line, total_width, pt))
        .collect()
}

fn paint_gradient_line(line: &str, total_width: usize, pt: &PeekTheme) -> String {
    let mut out = String::new();
    let start = pt.value;
    let end = pt.heading;
    for (i, ch) in line.chars().enumerate() {
        if ch == ' ' {
            out.push(' ');
        } else {
            let t = if total_width > 1 {
                i as f32 / (total_width - 1) as f32
            } else {
                0.0
            };
            let color = lerp_color(start, end, t);
            out.push_str(&pt.paint(&ch.to_string(), color));
        }
    }
    out
}
