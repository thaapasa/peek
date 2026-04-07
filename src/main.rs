use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;

mod detect;
mod help;
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
    #[arg(short, long)]
    plain: bool,

    /// Disable the built-in pager even on interactive terminals
    #[arg(short = 'P', long)]
    no_pager: bool,

    /// Syntax highlighting theme
    #[arg(
        short,
        long,
        env = "PEEK_THEME",
        default_value_t = theme::PeekThemeName::Base16OceanDark,
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
    let use_pager = !args.no_pager && is_tty;

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

    // Images on interactive terminals get their own interactive viewer
    // with contain-ratio sizing and resize support.
    // Non-image files go through the normal pager pipeline.
    let mut non_image_files = Vec::new();

    for (path, file_type) in &files {
        if matches!(file_type, detect::FileType::Image) && use_pager && !args.plain {
            viewers
                .image_viewer()
                .view_interactive(path)
                .with_context(|| format!("failed to render {}", path.display()))?;
        } else {
            non_image_files.push((path, file_type));
        }
    }

    // Render remaining files through the normal pager pipeline
    if !non_image_files.is_empty() {
        let mut output = pager::Output::new(&args)?;
        for (path, file_type) in non_image_files {
            let viewer = viewers.viewer_for(file_type);
            viewer
                .render(path, file_type, &mut output)
                .with_context(|| format!("failed to render {}", path.display()))?;
        }
        output.finish()?;
    }

    Ok(())
}
