use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

mod detect;
mod error;
mod pager;
mod theme;
mod viewer;

/// peek — a modern file viewer for the terminal.
///
/// View any file with automatic syntax highlighting, structured data
/// pretty-printing, and ASCII art image rendering. Works like `less`
/// by default on interactive terminals.
#[derive(Parser, Debug)]
#[command(name = "peek", version, about, long_about)]
pub struct Args {
    /// Files to view. Use `-` for stdin.
    #[arg(required = true)]
    files: Vec<PathBuf>,

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
        default_value = "base16-ocean.dark"
    )]
    theme: String,

    /// Force a specific language for syntax highlighting (skip auto-detection)
    #[arg(short, long)]
    language: Option<String>,

    /// Image rendering width in characters (0 = auto-fit terminal)
    #[arg(long, default_value = "0")]
    width: u32,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let viewers = viewer::Registry::new(&args)?;
    let mut output = pager::Output::new(&args)?;

    for path in &args.files {
        let file_type = detect::detect(path)?;
        let viewer = viewers.viewer_for(&file_type);
        viewer
            .render(path, &file_type, &mut output)
            .with_context(|| format!("failed to render {}", path.display()))?;
    }

    output.finish()?;
    Ok(())
}
