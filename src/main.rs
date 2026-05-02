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

    let use_pager = !args.print && std::io::stdout().is_terminal();

    let viewers = viewer::Registry::new(&args)?;
    let render_opts = info::RenderOptions { utc: args.utc };

    // --info mode: a fixed-size summary, never paginated. To scroll
    // through it, use the interactive viewer's Info mode (key `i`).
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

    if use_pager {
        // Interactive TTY: compose mode list per file type; one event loop.
        // compose_modes handles animation detection internally, so this
        // path is uniform across file types.
        let modes = viewers
            .compose_modes(&source, &detected, &args)
            .with_context(|| format!("failed to compose viewer for {}", source.name()))?;
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
        // Print mode: stdout once, no event loop. Binary → hex viewer
        // (registered as the dispatch target for FileType::Binary in
        // viewer_for).
        let mut output = output::PrintOutput::stdout();
        let file_type = &detected.file_type;
        let viewer = viewers.viewer_for(file_type);
        viewer
            .render(&source, file_type, &mut output)
            .with_context(|| format!("failed to render {}", source.name()))?;
        output.finish()?;
    }

    Ok(())
}
