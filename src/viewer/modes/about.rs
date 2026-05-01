use anyhow::Result;

use super::{Mode, ModeId, RenderCtx};
use crate::output::help::paint_logo;
use crate::theme::PeekTheme;
use crate::viewer::ui::Action;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
const LICENSE: &str = env!("CARGO_PKG_LICENSE");
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

pub(crate) struct AboutMode;

impl AboutMode {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Mode for AboutMode {
    fn id(&self) -> ModeId {
        ModeId::About
    }

    fn label(&self) -> &str {
        "About"
    }

    fn is_aux(&self) -> bool {
        true
    }

    fn render(&mut self, ctx: &RenderCtx) -> Result<Vec<String>> {
        let pt = ctx.peek_theme;
        let theme_name = ctx.theme_name.cli_name();
        let color_mode = pt.color_mode.cli_name();
        let mut lines: Vec<String> = Vec::new();

        // Logo with theme-color gradient — gives a quick read on what the
        // active theme looks like at a glance.
        for line in paint_logo(pt) {
            lines.push(line);
        }
        lines.push(String::new());

        // Title + description
        lines.push(pt.paint_heading(&format!("peek {VERSION}")));
        lines.push(pt.paint(DESCRIPTION, pt.foreground));
        lines.push(String::new());

        // Build & metadata
        lines.push(pt.paint_heading("ABOUT"));
        lines.push(kv_line(pt, "Author", AUTHORS));
        lines.push(kv_line(pt, "License", LICENSE));
        lines.push(kv_line(pt, "Repository", REPOSITORY));
        lines.push(String::new());

        // Live display state
        lines.push(pt.paint_heading("DISPLAY"));
        lines.push(kv_line(pt, "Theme", theme_name));
        lines.push(kv_line(pt, "Color mode", color_mode));
        lines.push(String::new());

        // Palette swatches: the colors the active theme actually paints.
        // Useful side-by-side readout when cycling themes with `t`.
        lines.push(pt.paint_heading("PALETTE"));
        lines.push(palette_line(pt));
        lines.push(String::new());

        // Tips
        lines.push(pt.paint_heading("TIPS"));
        lines.push(tip_line(pt, "t", "Cycle themes (compare them on this screen)"));
        lines.push(tip_line(pt, "c", "Cycle output color encoding"));
        lines.push(tip_line(pt, "h / ?", "Full keybinding reference"));
        lines.push(tip_line(pt, "a / Tab", "Exit this screen"));

        Ok(lines)
    }

    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        // SwitchToAbout is global; no extras here. Listed for symmetry
        // with HexMode/InfoMode.
        &[]
    }
}

fn kv_line(pt: &PeekTheme, key: &str, value: &str) -> String {
    format!(
        "  {}  {}",
        pt.paint_label(&format!("{key:<11}")),
        pt.paint(value, pt.foreground),
    )
}

fn tip_line(pt: &PeekTheme, key: &str, desc: &str) -> String {
    format!(
        "  {}  {}",
        pt.paint_accent(&format!("{key:<11}")),
        pt.paint_muted(desc),
    )
}

/// One line showing a swatch + label for each semantic color slot.
/// Six full-block characters per slot make the color easy to read at a glance.
fn palette_line(pt: &PeekTheme) -> String {
    const BLOCK: &str = "\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}";
    let entries: &[(&str, syntect::highlighting::Color)] = &[
        ("foreground", pt.foreground),
        ("accent", pt.accent),
        ("heading", pt.heading),
        ("label", pt.label),
        ("value", pt.value),
        ("muted", pt.muted),
        ("warning", pt.warning),
    ];
    entries
        .iter()
        .map(|(name, color)| {
            format!(
                "{} {}",
                pt.paint(BLOCK, *color),
                pt.paint_muted(name),
            )
        })
        .collect::<Vec<_>>()
        .join("  ")
}
