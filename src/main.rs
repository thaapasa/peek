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
use input::InputSource;

fn main() -> Result<()> {
    let args = Args::parse();

    if args.help || args.version {
        let theme_manager = theme::ThemeManager::new(args.theme);
        if args.help {
            output::help::render_help(&theme_manager)?;
        } else {
            output::help::render_version(&theme_manager)?;
        }
        return Ok(());
    }

    let sources = input::stdin::build_sources(&args)?;

    let is_tty = std::io::stdout().is_terminal();
    let use_pager = !args.print && is_tty;

    let viewers = viewer::Registry::new(&args)?;
    let render_opts = info::RenderOptions { utc: args.utc };

    // Detect file type (and capture magic-byte MIME) for each source.
    let inputs: Vec<(InputSource, input::detect::Detected)> = sources
        .into_iter()
        .map(|source| {
            let detected = input::detect::detect(&source)?;
            Ok((source, detected))
        })
        .collect::<Result<Vec<_>>>()?;

    // --info mode: show metadata instead of content
    if args.info {
        let mut output = output::Output::new(&args)?;
        for (source, detected) in &inputs {
            let file_info = info::gather(source, detected)
                .with_context(|| format!("failed to read info for {}", source.name()))?;
            let lines = info::render(&file_info, viewers.peek_theme(), render_opts);
            for line in &lines {
                output.write_line(line)?;
            }
        }
        output.finish()?;
        return Ok(());
    }

    if use_pager {
        // Interactive TTY: compose mode list per file type; one event loop.
        // compose_modes handles animation detection internally, so this
        // path is uniform across file types.
        for (source, detected) in &inputs {
            let modes = viewers
                .compose_modes(source, detected, &args)
                .with_context(|| format!("failed to compose viewer for {}", source.name()))?;
            viewer::interactive::run(source, detected, viewers.theme_name(), render_opts, modes)
                .with_context(|| format!("failed to render {}", source.name()))?;
        }
    } else {
        // Piped or --no-pager: direct output. Binary → hex viewer (registered
        // as the dispatch target for FileType::Binary in viewer_for).
        let mut output = output::Output::new(&args)?;
        for (source, detected) in &inputs {
            let file_type = &detected.file_type;
            let viewer = viewers.viewer_for(file_type);
            viewer
                .render(source, file_type, &mut output)
                .with_context(|| format!("failed to render {}", source.name()))?;
        }
        output.finish()?;
    }

    Ok(())
}

