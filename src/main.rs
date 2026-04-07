use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;

mod detect;
mod help;
mod info;
mod pager;
mod theme;
mod viewer;

/// peek — a modern file viewer for the terminal.
///
/// View any file with automatic syntax highlighting, structured data
/// pretty-printing, and ASCII art image rendering. Works like `less`
/// by default on interactive terminals.
#[derive(Parser, Debug)]
#[command(name = "peek", about, long_about, disable_help_flag = true, disable_version_flag = true)]
pub struct Args {
    /// Files to view. Use `-` for stdin.
    files: Vec<PathBuf>,

    /// Show themed help screen and exit
    #[arg(short = 'h', long = "help")]
    help: bool,

    /// Show version info and exit
    #[arg(long = "version")]
    version: bool,

    /// Disable syntax highlighting and pretty-printing (plain output)
    #[arg(short = 'P', long)]
    plain: bool,

    /// Force print mode (direct stdout, no interactive viewer)
    #[arg(short = 'p', long = "print")]
    print: bool,

    /// Syntax highlighting theme
    #[arg(
        short,
        long,
        env = "PEEK_THEME",
        default_value_t = theme::PeekThemeName::IslandsDark,
        value_enum,
    )]
    theme: theme::PeekThemeName,

    /// Force a specific language for syntax highlighting (skip auto-detection)
    #[arg(short, long)]
    language: Option<String>,

    /// Image rendering width in characters (0 = auto-fit terminal)
    #[arg(long, default_value = "0")]
    width: u32,

    /// Image rendering mode: "full" (all glyphs), "block" (blocks + punctuation), "geo" (blocks + lines only), "ascii" (legacy density ramp)
    #[arg(long, default_value = "full", value_parser = ["full", "block", "geo", "ascii"])]
    image_mode: String,

    /// Show file info instead of file contents
    #[arg(long)]
    info: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.help || args.version {
        let theme_manager = theme::ThemeManager::new(args.theme);
        if args.help {
            help::render_help(&theme_manager)?;
        } else {
            help::render_version(&theme_manager)?;
        }
        return Ok(());
    }

    if args.files.is_empty() {
        bail!("no files specified; run `peek --help` for usage");
    }

    let is_tty = std::io::stdout().is_terminal();
    let use_pager = !args.print && is_tty;

    let viewers = viewer::Registry::new(&args)?;

    // Collect files and their types
    let files: Vec<_> = args
        .files
        .iter()
        .map(|path| {
            let file_type = detect::detect(path)?;
            Ok((path.clone(), file_type))
        })
        .collect::<Result<Vec<_>>>()?;

    // --info mode: show file metadata instead of content
    if args.info {
        let mut output = pager::Output::new(&args)?;
        for (path, file_type) in &files {
            let file_info = info::gather(path, file_type)
                .with_context(|| format!("failed to read info for {}", path.display()))?;
            let lines = info::render(&file_info, viewers.peek_theme());
            for line in &lines {
                output.write_line(line)?;
            }
        }
        output.finish()?;
        return Ok(());
    }

    if use_pager {
        // Interactive TTY: each file gets its own interactive viewer
        // with Tab/i view switching between content and file info.
        for (path, file_type) in &files {
            if matches!(file_type, detect::FileType::Image) && !args.plain {
                // Images re-render on resize for correct aspect ratio
                viewers
                    .image_viewer()
                    .view_interactive(path, file_type)
                    .with_context(|| format!("failed to render {}", path.display()))?;
            } else {
                // Other files: pre-render content, show in interactive viewer
                let viewer = viewers.viewer_for(file_type);
                let mut buf = pager::Output::buffer();
                viewer.render(path, file_type, &mut buf)?;
                let content_lines = buf.into_lines();

                viewer::interactive::view_interactive(
                    path,
                    file_type,
                    viewers.peek_theme(),
                    |stdout| {
                        use std::io::Write;
                        for line in &content_lines {
                            stdout.write_all(line.as_bytes())?;
                            stdout.write_all(b"\r\n")?;
                        }
                        Ok(())
                    },
                )
                .with_context(|| format!("failed to render {}", path.display()))?;
            }
        }
    } else {
        // Piped or --no-pager: direct output
        let mut output = pager::Output::new(&args)?;
        for (path, file_type) in &files {
            let viewer = viewers.viewer_for(file_type);
            viewer
                .render(path, file_type, &mut output)
                .with_context(|| format!("failed to render {}", path.display()))?;
        }
        output.finish()?;
    }

    Ok(())
}
