use std::path::PathBuf;

use clap::Parser;

use crate::theme;

/// peek — a modern file viewer for the terminal.
///
/// View any file with automatic syntax highlighting, structured data
/// pretty-printing, and ASCII art image rendering. Works like `less`
/// by default on interactive terminals.
///
/// peek is a single-file viewer: it takes one path (or stdin), not a list.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "peek",
    about,
    long_about,
    disable_help_flag = true,
    disable_version_flag = true
)]
pub struct Args {
    /// File to view. Use `-` to read stdin.
    pub file: Option<PathBuf>,

    /// Show concise help (logo and common options)
    #[arg(short = 'h', hide_short_help = true)]
    pub short_help: bool,

    /// Show full help with all options
    #[arg(long = "help", hide_short_help = true)]
    pub help: bool,

    /// Print version and exit
    #[arg(short = 'V', long = "version")]
    pub version: bool,

    /// Disable syntax highlighting, pretty-printing, and color output
    /// (implies `--color plain`)
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
        default_value_t = theme::PeekThemeName::default(),
        value_enum,
        hide_short_help = true,
    )]
    pub theme: theme::PeekThemeName,

    /// Output color encoding (truecolor / 256 / 16 / grayscale / plain)
    #[arg(
        short = 'C',
        long,
        env = "PEEK_COLOR",
        default_value_t = theme::StyleMode::default(),
        value_enum,
        hide_short_help = true,
    )]
    pub color: theme::StyleMode,

    /// Force a specific language for syntax highlighting (skip auto-detection)
    #[arg(short = 'L', long, hide_short_help = true)]
    pub language: Option<String>,

    /// Image rendering width in characters (0 = auto-fit terminal)
    #[arg(short = 'w', long, default_value = "0", hide_short_help = true)]
    pub width: u32,

    /// Image rendering mode: "full" (all glyphs), "block" (blocks + punctuation), "geo" (blocks + lines only), "ascii" (legacy density ramp), "contour" (Sobel edge line-art)
    #[arg(short = 'm', long, default_value = "full", value_parser = ["full", "block", "geo", "ascii", "contour"], hide_short_help = true)]
    pub image_mode: String,

    /// Edge density target for contour mode (fraction of pixels marked as edges, 0.01..0.5)
    #[arg(long, default_value_t = 0.1, hide_short_help = true)]
    pub edge_density: f32,

    /// Image transparency background: "auto" (detect), "black", "white", "checkerboard"
    #[arg(long, default_value = "auto", value_parser = ["auto", "black", "white", "checkerboard", "checker"], hide_short_help = true)]
    pub background: String,

    /// Image margin in pixels of transparent padding (0 = no margin)
    #[arg(long, default_value = "0", hide_short_help = true)]
    pub margin: u32,

    /// Disable SVG animation playback; render the static SVG instead
    #[arg(long, hide_short_help = true)]
    pub no_svg_anim: bool,

    /// Show file info instead of file contents
    #[arg(short = 'i', long)]
    pub info: bool,

    /// List the contents of a container file
    #[arg(short = 'l', long)]
    pub list: bool,

    /// Show line numbers in text views (toggle in viewer with `l`)
    #[arg(short = 'n', long = "line-numbers", hide_short_help = true)]
    pub line_numbers: bool,

    /// Show timestamps in UTC (ISO 8601 with `Z` suffix) instead of
    /// local time with offset.
    #[arg(long, hide_short_help = true)]
    pub utc: bool,

    /// Check GitHub for a newer release and install it via the official installer
    #[arg(long, hide_short_help = true)]
    pub update: bool,

    /// Pull a sub-item out of a container by key. Use --list to discover
    /// available keys.
    #[arg(short = 'x', long, value_name = "KEY")]
    pub extract: Option<String>,

    /// Destination for `--extract`. `-` writes raw bytes to stdout. When
    /// omitted, the extractor's suggested filename is used (relative to
    /// the current directory).
    #[arg(
        short = 'o',
        long = "output",
        value_name = "PATH",
        hide_short_help = true
    )]
    pub output: Option<PathBuf>,

    /// Pixel size for the longest axis when extracting an SVG frame
    /// (default: upscale sub-512px SVGs to 512). Ignored for non-SVG sources.
    #[arg(long = "extract-size", value_name = "PX", hide_short_help = true)]
    pub extract_size: Option<u32>,

    /// Override terminal cell aspect ratio (height ÷ width) for image
    /// scaling. Auto-detected by default; set explicitly (e.g. `2.0`) when
    /// detection is wrong.
    #[arg(long = "cell-aspect", value_name = "RATIO", hide_short_help = true)]
    pub cell_aspect: Option<f64>,
}
