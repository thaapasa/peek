use std::path::PathBuf;

use clap::Parser;

use crate::theme;

/// peek — a modern file viewer for the terminal.
///
/// View any file with automatic syntax highlighting, structured data
/// pretty-printing, and ASCII art image rendering. Works like `less`
/// by default on interactive terminals.
#[derive(Parser, Debug)]
#[command(name = "peek", about, long_about, disable_help_flag = true, disable_version_flag = true)]
pub struct Args {
    /// Files to view. Use `-` to read stdin.
    pub files: Vec<PathBuf>,

    /// Show themed help screen and exit
    #[arg(short = 'h', long = "help")]
    pub help: bool,

    /// Show version info and exit
    #[arg(long = "version")]
    pub version: bool,

    /// Disable syntax highlighting and pretty-printing (plain output)
    #[arg(short = 'P', long)]
    pub plain: bool,

    /// Output verbatim source (no pretty-printing, still highlighted)
    #[arg(short, long)]
    pub raw: bool,

    /// Force print mode (direct stdout, no interactive viewer)
    #[arg(short = 'p', long = "print")]
    pub print: bool,

    /// Syntax highlighting theme
    #[arg(
        short,
        long,
        env = "PEEK_THEME",
        default_value_t = theme::PeekThemeName::IslandsDark,
        value_enum,
    )]
    pub theme: theme::PeekThemeName,

    /// Output color encoding (truecolor / 256 / 16 / grayscale / plain)
    #[arg(
        short = 'C',
        long,
        env = "PEEK_COLOR",
        default_value_t = theme::ColorMode::TrueColor,
        value_enum,
    )]
    pub color: theme::ColorMode,

    /// Force a specific language for syntax highlighting (skip auto-detection)
    #[arg(short, long)]
    pub language: Option<String>,

    /// Image rendering width in characters (0 = auto-fit terminal)
    #[arg(long, default_value = "0")]
    pub width: u32,

    /// Image rendering mode: "full" (all glyphs), "block" (blocks + punctuation), "geo" (blocks + lines only), "ascii" (legacy density ramp)
    #[arg(long, default_value = "full", value_parser = ["full", "block", "geo", "ascii"])]
    pub image_mode: String,

    /// Image transparency background: "auto" (detect), "black", "white", "checkerboard"
    #[arg(long, default_value = "auto", value_parser = ["auto", "black", "white", "checkerboard", "checker"])]
    pub background: String,

    /// Image margin in pixels of transparent padding (0 = no margin)
    #[arg(long, default_value = "0")]
    pub margin: u32,

    /// Show file info instead of file contents
    #[arg(long)]
    pub info: bool,

    /// Show timestamps in UTC (ISO 8601 with `Z` suffix) instead of
    /// local time with offset.
    #[arg(long)]
    pub utc: bool,
}
