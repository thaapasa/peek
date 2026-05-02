use std::io::IsTerminal;

use anyhow::{Context, Result};
use clap::Parser;

mod cli;
mod info;
mod input;
mod output;
mod theme;
mod viewer;

pub use cli::Args;

fn main() -> Result<()> {
    let args = Args::parse();

    if args.version {
        output::help::render_version()?;
        return Ok(());
    }
    // No args + interactive stdin → show short help instead of erroring.
    let no_input = args.file.is_none() && std::io::stdin().is_terminal();
    if args.short_help || args.help || no_input {
        let theme_manager = theme::ThemeManager::new(args.theme, args.color);
        output::help::render_help(&theme_manager, !args.help)?;
        return Ok(());
    }

    let source = input::stdin::build_source(&args)?;
    let detected = input::detect::detect(&source)?;

    let interactive = !args.print && std::io::stdout().is_terminal();

    let viewers = viewer::Registry::new(&args)?;
    let render_opts = info::RenderOptions { utc: args.utc };

    // --info mode: a fixed-size summary, written straight to stdout. For
    // a scrollable view, use the interactive viewer's Info mode (key `i`).
    if args.info {
        let mut output = output::PrintOutput::stdout();
        let file_info = info::gather(&source, &detected)
            .with_context(|| format!("failed to read info for {}", source.name()))?;
        let lines = info::render(&file_info, viewers.peek_theme(), render_opts);
        for line in &lines {
            output.write_line(line)?;
        }
        output.finish()?;
        return Ok(());
    }

    let mut modes = viewers
        .compose_modes(&source, &detected, &args)
        .with_context(|| format!("failed to compose viewer for {}", source.name()))?;

    if interactive {
        // Interactive TTY: compose mode list per file type; one event loop.
        // compose_modes handles animation detection internally, so this
        // path is uniform across file types.
        viewer::interactive::run(
            &source,
            &detected,
            viewers.theme_name(),
            args.color,
            render_opts,
            modes,
        )
        .with_context(|| format!("failed to render {}", source.name()))?;
    } else {
        // Print mode: stdout once, no event loop. Render the primary
        // (first non-aux) mode straight to stdout — for binary files,
        // where every mode is aux, fall back to the first mode (Hex).
        let mut output = output::PrintOutput::stdout();
        let file_info = info::gather(&source, &detected)
            .with_context(|| format!("failed to read info for {}", source.name()))?;
        let ctx = viewer::modes::RenderCtx {
            source: &source,
            detected: &detected,
            file_info: &file_info,
            theme_name: viewers.theme_name(),
            peek_theme: viewers.peek_theme(),
            render_opts,
            term_cols: pipe_term_cols(),
            term_rows: usize::MAX,
        };
        let primary_idx = modes.iter().position(|m| !m.is_aux()).unwrap_or(0);
        modes[primary_idx]
            .render_to_pipe(&ctx, &mut output)
            .with_context(|| format!("failed to render {}", source.name()))?;
        output.finish()?;
    }

    Ok(())
}

/// Terminal width to use for non-interactive (pipe) rendering. Honors
/// `$COLUMNS` if set and at least 24; otherwise falls back to 80. Hex
/// dumps and image rendering use this to size their output sensibly
/// even when stdout isn't a TTY.
fn pipe_term_cols() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n >= 24)
        .unwrap_or(80)
}
