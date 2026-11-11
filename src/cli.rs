use std::io::IsTerminal;
use std::path::PathBuf;

use clap::{CommandFactory, FromArgMatches, Parser, ValueHint};
use peek_foundation::viewer::ComposeOpts;
use peek_theme::StyleMode;

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
    #[arg(value_hint = ValueHint::FilePath)]
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

    /// Open the peek user manual. Combine with --list, --extract or --print
    /// like any other container
    #[arg(long, conflicts_with = "file")]
    pub manual: bool,

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
        default_value_t = peek_theme::PeekThemeName::default(),
        value_enum,
        hide_short_help = true,
    )]
    pub theme: peek_theme::PeekThemeName,

    /// Output color encoding (auto / truecolor / 256 / 16 / grayscale /
    /// plain). `auto` colors only when stdout is a terminal
    #[arg(
        short = 'C',
        long = "color",
        value_name = "COLOR",
        env = "PEEK_COLOR",
        default_value = "auto",
        value_enum,
        hide_short_help = true
    )]
    pub color_arg: ColorArg,

    /// The encoding in effect: `plain` under `--plain`, else `color_arg` with
    /// `auto` resolved. Set by [`parse`]; `Args::parse_from` (tests) leaves
    /// the truecolor default.
    #[arg(skip)]
    pub color: StyleMode,

    /// Force a specific language for syntax highlighting (skip auto-detection)
    #[arg(short = 'L', long, hide_short_help = true)]
    pub language: Option<String>,

    /// Image rendering width in characters (0 = auto-fit terminal, max 2048)
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

    /// Emit `--info` as machine-readable JSON (for shell pipelines, e.g.
    /// `peek x --info --json | jq .size_bytes`). Requires `--info`.
    #[arg(long, hide_short_help = true)]
    pub json: bool,

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

    /// Print the shell completion script, for use with eval in the shell
    /// startup file
    #[arg(long, value_name = "SHELL", hide_short_help = true)]
    pub completions: Option<clap_complete::Shell>,

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

    /// Force extract operations to keep payloads in RAM instead of
    /// spooling large archive entries to a tempfile. Drops the 256 MiB
    /// in-memory cap on the extract path — use only when you'd rather
    /// risk OOM than touch `$TMPDIR`.
    #[arg(long = "no-tempfile", hide_short_help = true)]
    pub no_tempfile: bool,

    /// Override terminal cell aspect ratio (height ÷ width) for image
    /// scaling. Auto-detected by default; set explicitly (e.g. `2.0`) when
    /// detection is wrong.
    #[arg(long = "cell-aspect", value_name = "RATIO", hide_short_help = true)]
    pub cell_aspect: Option<f64>,

    /// Pre-grant slow / large operations: skip the load confirmation that
    /// a big transparently-compressed file (`.xz` / `.gz` / …) would
    /// otherwise show before decompressing. Non-interactive output (pipe /
    /// `--print` / `--info`) is always pre-granted.
    #[arg(short = 'y', long = "yes", hide_short_help = true)]
    pub yes: bool,
}

/// Parse CLI args, reporting whether the theme was set explicitly (via
/// `--theme` or `PEEK_THEME`) rather than left at clap's default. The
/// caller uses that to auto-pick a light/dark default theme from the
/// detected terminal background only when the user hasn't chosen one.
pub fn parse() -> (Args, bool) {
    let matches = Args::command().get_matches();
    let theme_explicit =
        matches.value_source("theme") != Some(clap::parser::ValueSource::DefaultValue);
    let mut args = Args::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    // `--plain` implies `--color plain`: one "sterile output" knob. Otherwise
    // the status bar and UI chrome would stay themed around unstyled content.
    args.color = if args.plain {
        StyleMode::Plain
    } else {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        args.color_arg
            .resolve(std::io::stdout().is_terminal(), no_color)
    };
    (args, theme_explicit)
}

/// `--color` as given. `auto` is a startup decision, not an encoding, so it
/// stays out of [`StyleMode`] and is resolved once in [`parse`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorArg {
    Auto,
    Mode(StyleMode),
}

impl ColorArg {
    /// `auto`: truecolor on a terminal, plain when piped or redirected, and
    /// plain when `NO_COLOR` is set (<https://no-color.org>). An explicit
    /// encoding always wins, so `--color truecolor | less -R` keeps colors.
    pub fn resolve(self, stdout_is_terminal: bool, no_color: bool) -> StyleMode {
        match self {
            Self::Mode(mode) => mode,
            Self::Auto if stdout_is_terminal && !no_color => StyleMode::TrueColor,
            Self::Auto => StyleMode::Plain,
        }
    }
}

impl clap::ValueEnum for ColorArg {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Self::Auto,
            Self::Mode(StyleMode::TrueColor),
            Self::Mode(StyleMode::Ansi256),
            Self::Mode(StyleMode::Ansi16),
            Self::Mode(StyleMode::Grayscale),
            Self::Mode(StyleMode::Plain),
        ]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            Self::Auto => Some(
                clap::builder::PossibleValue::new("auto")
                    .help("Truecolor on a terminal, plain when piped (default)"),
            ),
            Self::Mode(mode) => mode.to_possible_value(),
        }
    }
}

impl Args {
    /// Project the clap-parsed args down to the plain `ComposeOpts` the
    /// compose path consumes — the seam that keeps clap out of the
    /// reader/compose layer (and, after the crate split, out of
    /// `peek-types`).
    pub fn compose_opts(&self) -> ComposeOpts {
        ComposeOpts {
            theme: self.theme,
            color: self.color,
            plain: self.plain,
            raw: self.raw,
            line_numbers: self.line_numbers,
            no_svg_anim: self.no_svg_anim,
            language: self.language.clone(),
            width: self.width,
            margin: self.margin,
            image_mode: self.image_mode.clone(),
            background: self.background.clone(),
            edge_density: self.edge_density,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_colors_only_on_a_terminal_without_no_color() {
        assert_eq!(ColorArg::Auto.resolve(true, false), StyleMode::TrueColor);
        assert_eq!(ColorArg::Auto.resolve(false, false), StyleMode::Plain);
        assert_eq!(ColorArg::Auto.resolve(true, true), StyleMode::Plain);
    }

    #[test]
    fn explicit_encoding_always_wins() {
        let mode = ColorArg::Mode(StyleMode::Ansi256);
        assert_eq!(mode.resolve(false, true), StyleMode::Ansi256);
    }

    #[test]
    fn color_flag_parses_auto_default_and_every_encoding() {
        assert_eq!(Args::parse_from(["peek"]).color_arg, ColorArg::Auto);
        for (name, mode) in [
            ("truecolor", StyleMode::TrueColor),
            ("256", StyleMode::Ansi256),
            ("16", StyleMode::Ansi16),
            ("grayscale", StyleMode::Grayscale),
            ("plain", StyleMode::Plain),
        ] {
            let args = Args::parse_from(["peek", "--color", name]);
            assert_eq!(args.color_arg, ColorArg::Mode(mode), "{name}");
        }
    }
}
