use anyhow::Result;
use crossterm::terminal;

use super::{Mode, ModeId, RenderCtx, Window, slice_window};
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

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
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

        // Live system state
        lines.push(pt.paint_heading("SYSTEM"));
        let (term_cols, term_rows) = terminal_dimensions();
        lines.push(kv_line(
            pt,
            "Terminal",
            &format!("{term_cols} × {term_rows}"),
        ));
        if let Some(rss) = peak_rss_bytes() {
            lines.push(kv_line(pt, "Peak memory", &format_bytes(rss)));
        }
        lines.push(String::new());

        // Palette swatches: the colors the active theme actually paints.
        // Useful side-by-side readout when cycling themes with `t`.
        lines.push(pt.paint_heading("PALETTE"));
        lines.push(palette_line(pt));
        lines.push(String::new());

        // Tips
        lines.push(pt.paint_heading("TIPS"));
        lines.push(tip_line(
            pt,
            "t",
            "Cycle themes (compare them on this screen)",
        ));
        lines.push(tip_line(pt, "c", "Cycle output color encoding"));
        lines.push(tip_line(pt, "h / ?", "Full keybinding reference"));
        lines.push(tip_line(pt, "a / Tab", "Exit this screen"));

        let total = lines.len();
        let lines = slice_window(&lines, scroll, rows);
        Ok(Window { lines, total })
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

fn terminal_dimensions() -> (u16, u16) {
    terminal::size().unwrap_or((80, 24))
}

/// Peak resident set size for the current process, in bytes.
/// `ru_maxrss` is bytes on macOS, kilobytes on Linux/BSD.
fn peak_rss_bytes() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    let ret = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if ret != 0 {
        return None;
    }
    let raw = unsafe { usage.assume_init() }.ru_maxrss as u64;
    Some(if cfg!(target_os = "macos") {
        raw
    } else {
        raw.saturating_mul(1024)
    })
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    for unit in UNITS {
        if value < 1024.0 {
            return if *unit == "B" {
                format!("{value:.0} {unit}")
            } else {
                format!("{value:.2} {unit}")
            };
        }
        value /= 1024.0;
    }
    format!("{value:.2} PiB")
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
        .map(|(name, color)| format!("{} {}", pt.paint(BLOCK, *color), pt.paint_muted(name),))
        .collect::<Vec<_>>()
        .join("  ")
}
