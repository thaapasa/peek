pub mod print;

pub use print::PrintOutput;

use crate::theme::{PeekTheme, lerp_color};

/// The product tagline, shown under the logo on the About view and the
/// CLI help screen. Lives here (not `env!("CARGO_PKG_DESCRIPTION")`)
/// because About renders inside this crate, whose own description is
/// the library blurb, not the product's. Keep in sync with the root
/// `Cargo.toml` `description`.
pub const DESCRIPTION: &str = "Modern terminal file viewer — preview any file, any format";

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
    paint_logo_with(pt, |_, _, t| lerp_color(pt.value, pt.heading, t))
}

/// Paint the [`LOGO`] with a caller-chosen color per glyph. `color_at`
/// receives `(row, col, t)` where `t` is the column's position in the
/// `[0, 1]` left→right ramp; spaces are passed through unpainted. The
/// single glyph-walking loop behind both the static [`paint_logo`]
/// gradient and the About screen's animated variant.
pub fn paint_logo_with(
    pt: &PeekTheme,
    color_at: impl Fn(usize, usize, f32) -> syntect::highlighting::Color,
) -> Vec<String> {
    let total_width = LOGO.iter().map(|l| l.len()).max().unwrap_or(0);
    LOGO.iter()
        .enumerate()
        .map(|(row, line)| {
            let mut out = String::new();
            for (col, ch) in line.chars().enumerate() {
                if ch == ' ' {
                    out.push(' ');
                    continue;
                }
                let t = if total_width > 1 {
                    col as f32 / (total_width - 1) as f32
                } else {
                    0.0
                };
                out.push_str(&pt.paint(&ch.to_string(), color_at(row, col, t)));
            }
            out
        })
        .collect()
}
