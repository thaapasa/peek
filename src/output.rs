//! CLI help / version screens. The pipe writer (`PrintOutput`) and the
//! logo painter live in `peek-foundation` (`peek_foundation::output`);
//! reference those directly.

use std::io::{self, Write};

use anyhow::Result;
use clap::CommandFactory;
use peek_foundation::output::{DESCRIPTION, paint_logo};
use peek_theme::{PeekThemeName, ThemeManager};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const MANUAL_URL: &str = "https://thaapasa.github.io/peek/";

pub fn render_version() -> Result<()> {
    let mut out = io::stdout();
    writeln!(out, "peek {VERSION}")?;
    out.flush()?;
    Ok(())
}

pub fn render_help(theme_manager: &ThemeManager, short: bool) -> Result<()> {
    let pt = theme_manager.peek_theme();
    let mut out = io::stdout();

    for line in paint_logo(pt) {
        writeln!(out, "{line}")?;
    }
    writeln!(out)?;

    // Version
    writeln!(out, "{}", pt.paint_heading(&format!("peek v{VERSION}")))?;
    writeln!(out, "{}", pt.paint(DESCRIPTION, pt.foreground))?;
    writeln!(out)?;

    // Usage
    writeln!(out, "{}", pt.paint_heading("USAGE"))?;
    writeln!(
        out,
        "  {} {} {}",
        pt.paint_label("peek"),
        pt.paint_muted("[OPTIONS]"),
        pt.paint_value("<FILE>"),
    )?;
    writeln!(out)?;

    // Options
    writeln!(out, "{}", pt.paint_heading("OPTIONS"))?;
    let cmd = crate::Args::command();
    for arg in cmd.get_arguments() {
        if short && arg.is_hide_short_help_set() {
            continue;
        }
        let long = arg.get_long();
        let short_flag = arg.get_short();
        let help_text = arg.get_help().map(|h| h.to_string()).unwrap_or_default();

        // Build the flag string
        let flag = match (short_flag, long) {
            (Some(s), Some(l)) => format!("-{s}, --{l}"),
            (None, Some(l)) => format!("    --{l}"),
            (Some(s), None) => format!("-{s}"),
            (None, None) => continue, // positional — skip
        };

        // Show value name if the option takes a value
        let is_bool = arg.get_action().takes_values();
        let value_name = if is_bool {
            let name = arg
                .get_value_names()
                .and_then(|v| v.first())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "VALUE".to_string());
            format!(" {}", pt.paint_value(&format!("<{name}>")))
        } else {
            String::new()
        };

        writeln!(out, "  {}{value_name}", pt.paint_label(&flag),)?;
        if !help_text.is_empty() {
            writeln!(out, "      {}", pt.paint_muted(&help_text))?;
        }
    }
    writeln!(out)?;

    writeln!(out, "{}", pt.paint_heading("MANUAL"))?;
    writeln!(out, "  {}", pt.paint_value(MANUAL_URL))?;
    writeln!(out)?;

    if short {
        writeln!(
            out,
            "{} {} {}",
            pt.paint_muted("Run"),
            pt.paint_label("peek --help"),
            pt.paint_muted("to see all options."),
        )?;
    } else {
        // Themes
        writeln!(out, "{}", pt.paint_heading("THEMES"))?;
        for variant in <PeekThemeName as clap::ValueEnum>::value_variants() {
            let name = variant.cli_name();
            let desc = variant.help_text();
            let marker = if *variant == theme_manager.theme_name {
                " (active)"
            } else {
                ""
            };
            writeln!(
                out,
                "  {}  {}{}",
                pt.paint_value(&format!("{name:<24}")),
                pt.paint_muted(desc),
                pt.paint_accent(marker),
            )?;
        }
    }

    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::DESCRIPTION;

    /// The shared tagline lives in peek-foundation (About renders there,
    /// where the bin's `CARGO_PKG_DESCRIPTION` is out of reach). This pins
    /// it to the root manifest's description so the two can't drift.
    #[test]
    fn tagline_matches_bin_description() {
        assert_eq!(DESCRIPTION, env!("CARGO_PKG_DESCRIPTION"));
    }
}
