use std::io::IsTerminal;

use anyhow::{Context, Result};
use clap::Parser;

mod cli;
mod extract;
mod info;
mod input;
mod output;
mod theme;
mod types;
mod update;
mod viewer;

pub use cli::Args;

fn main() -> Result<()> {
    let args = Args::parse();

    if args.version {
        output::help::render_version()?;
        return Ok(());
    }
    if args.update {
        return update::run();
    }
    // No args + interactive stdin → show short help instead of erroring.
    let no_input = args.file.is_none() && std::io::stdin().is_terminal();
    if args.short_help || args.help || no_input {
        let theme_manager = theme::ThemeManager::new(args.theme, args.color);
        output::help::render_help(&theme_manager, !args.help)?;
        return Ok(());
    }

    if let Some(aspect) = args.cell_aspect {
        viewer::cell_size::set_override(aspect);
    }

    let mut source = input::stdin::build_source(&args)?;
    let mut detected = input::detect::detect(&source)?;

    // --extract: pull an inner item out of a container. With `--print`
    // or `--info`, swap source for the extracted one and fall through
    // to the regular pipeline (recursive peek). Otherwise save it to
    // disk or stream to stdout.
    if let Some(key) = args.extract.as_deref() {
        // `--extract-size` wins; otherwise `--width` flows through as a
        // view-cols hint so SVG extracts raster at the same resolution
        // a live `--print --width N` would produce.
        let view_cols = match (args.extract_size, args.width) {
            (Some(_), _) => None,
            (None, w) if w > 0 => Some(w),
            _ => None,
        };
        let opts = extract::ExtractOptions {
            svg_size: args.extract_size,
            view_cols,
        };
        let extracted = extract::extract(&source, &detected, key, &opts)
            .with_context(|| format!("failed to extract {key:?} from {}", source.name()))?;

        let render_extracted = args.print || args.info;
        if !render_extracted {
            let dest = pick_extract_output(&args, &extracted.suggested_name);
            let written = extract::write::write_extracted(&extracted, dest)
                .with_context(|| format!("failed to write extracted {key:?}"))?;
            if written != std::path::Path::new("-") {
                eprintln!("wrote {}", written.display());
            }
            return Ok(());
        }

        source = extracted.source;
        detected = input::detect::detect(&source)?;
    }

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
        let viewers = std::rc::Rc::new(viewers);
        let viewers_for_builder = viewers.clone();
        let args_for_builder = args.clone();
        let mode_builder: viewer::ui::state::ModeBuilder =
            Box::new(move |s, d| viewers_for_builder.compose_modes(s, d, &args_for_builder));
        let theme_name = viewers.theme_name();
        let source_name = source.name().to_string();
        viewer::interactive::run(
            source,
            detected,
            theme_name,
            args.color,
            render_opts,
            modes,
            mode_builder,
        )
        .with_context(|| format!("failed to render {source_name}"))?;
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
            term_cols: pipe_term_cols(&args),
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

/// `-o` wins; piped stdout → Stdout; else suggested filename in cwd.
fn pick_extract_output(args: &Args, suggested: &str) -> extract::write::Output {
    if let Some(path) = args.output.as_deref() {
        return extract::write::Output::resolve(Some(path), suggested);
    }
    if !std::io::stdout().is_terminal() {
        return extract::write::Output::Stdout;
    }
    extract::write::Output::resolve(None, suggested)
}

/// Terminal width for non-interactive (pipe) rendering. `--width N`
/// wins (user explicitly asked for that output width); otherwise
/// `$COLUMNS` if set and ≥ 24; else 80.
fn pipe_term_cols(args: &Args) -> usize {
    if args.width > 0 {
        return args.width as usize;
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n >= 24)
        .unwrap_or(80)
}
