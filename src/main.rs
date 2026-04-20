use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;

mod detect;
mod help;
mod info;
mod input;
mod pager;
mod theme;
mod viewer;

use input::InputSource;

/// peek — a modern file viewer for the terminal.
///
/// View any file with automatic syntax highlighting, structured data
/// pretty-printing, and ASCII art image rendering. Works like `less`
/// by default on interactive terminals.
#[derive(Parser, Debug)]
#[command(name = "peek", about, long_about, disable_help_flag = true, disable_version_flag = true)]
pub struct Args {
    /// Files to view. Use `-` to read stdin.
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

    /// Output verbatim source (no pretty-printing, still highlighted)
    #[arg(short, long)]
    raw: bool,

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

    /// Image transparency background: "auto" (detect), "black", "white", "checkerboard"
    #[arg(long, default_value = "auto", value_parser = ["auto", "black", "white", "checkerboard", "checker"])]
    background: String,

    /// Image margin in pixels of transparent padding (0 = no margin)
    #[arg(long, default_value = "0")]
    margin: u32,

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

    let sources = build_sources(&args)?;

    let is_tty = std::io::stdout().is_terminal();
    let use_pager = !args.print && is_tty;

    let viewers = viewer::Registry::new(&args)?;

    // Detect file type for each source
    let inputs: Vec<(InputSource, detect::FileType)> = sources
        .into_iter()
        .map(|source| {
            let file_type = detect::detect(&source)?;
            Ok((source, file_type))
        })
        .collect::<Result<Vec<_>>>()?;

    // --info mode: show metadata instead of content
    if args.info {
        let mut output = pager::Output::new(&args)?;
        for (source, file_type) in &inputs {
            let file_info = info::gather(source, file_type)
                .with_context(|| format!("failed to read info for {}", source.name()))?;
            let lines = info::render(&file_info, viewers.peek_theme());
            for line in &lines {
                output.write_line(line)?;
            }
        }
        output.finish()?;
        return Ok(());
    }

    if use_pager {
        // Interactive TTY: each input gets its own interactive viewer
        // with Tab/i view switching between content and file info.
        for (source, file_type) in &inputs {
            if matches!(file_type, detect::FileType::Binary) {
                // Binary files: print info and exit (no content to display)
                let file_info = info::gather(source, file_type)
                    .with_context(|| format!("failed to read info for {}", source.name()))?;
                let lines = info::render(&file_info, viewers.peek_theme());
                for line in &lines {
                    println!("{line}");
                }
            } else if matches!(file_type, detect::FileType::Image) && !args.plain {
                // Images re-render on resize for correct aspect ratio
                viewers
                    .image_viewer()
                    .view_interactive(source, file_type)
                    .with_context(|| format!("failed to render {}", source.name()))?;
            } else if matches!(file_type, detect::FileType::Svg) && !args.plain {
                // SVGs: rasterized preview with r to toggle XML source
                viewers
                    .svg_viewer()
                    .view_interactive(source, file_type)
                    .with_context(|| format!("failed to render {}", source.name()))?;
            } else {
                // Other files: render on demand with theme-aware re-rendering
                let render_content = viewers
                    .content_renderer(source, file_type)
                    .with_context(|| format!("failed to read {}", source.name()))?;

                viewer::interactive::view_interactive(
                    source,
                    file_type,
                    viewers.theme_name(),
                    false,
                    !args.raw,
                    render_content,
                )
                .with_context(|| format!("failed to render {}", source.name()))?;
            }
        }
    } else {
        // Piped or --no-pager: direct output
        let mut output = pager::Output::new(&args)?;
        for (source, file_type) in &inputs {
            if matches!(file_type, detect::FileType::Binary) {
                // Binary files: show file info instead of content
                let file_info = info::gather(source, file_type)
                    .with_context(|| format!("failed to read info for {}", source.name()))?;
                let lines = info::render(&file_info, viewers.peek_theme());
                for line in &lines {
                    output.write_line(line)?;
                }
            } else {
                let viewer = viewers.viewer_for(file_type);
                viewer
                    .render(source, file_type, &mut output)
                    .with_context(|| format!("failed to render {}", source.name()))?;
            }
        }
        output.finish()?;
    }

    Ok(())
}

/// Decide the input sources based on args and stdin state.
///
/// Behavior:
/// - `peek` with stdin TTY and no args   → error
/// - `peek` with stdin piped, no args    → read stdin
/// - `peek -`                            → read stdin (blocks on TTY)
/// - `peek file.rs`                      → file, stdin ignored even if piped
/// - `peek - file.rs`                    → stdin + file
///
/// After consuming piped stdin, fd 0 is reopened from `/dev/tty` so the
/// interactive crossterm event loop can still read keystrokes.
fn build_sources(args: &Args) -> Result<Vec<InputSource>> {
    let has_dash = args.files.iter().any(|p| p.as_os_str() == "-");
    let stdin_is_tty = std::io::stdin().is_terminal();
    let implicit_stdin = args.files.is_empty() && !stdin_is_tty;

    if args.files.is_empty() && !implicit_stdin {
        bail!("no files specified; run `peek --help` for usage");
    }

    let want_stdin = has_dash || implicit_stdin;

    let stdin_data = if want_stdin {
        let mut buf = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buf)
            .context("failed to read stdin")?;
        reopen_stdin_from_tty();
        Some(buf)
    } else {
        None
    };

    if args.files.is_empty() {
        return Ok(vec![InputSource::Stdin {
            data: stdin_data.expect("stdin requested but not read"),
        }]);
    }

    let mut stdin_slot = stdin_data;
    let sources = args
        .files
        .iter()
        .map(|p| {
            if p.as_os_str() == "-" {
                // First `-` takes the data; extra `-`s get an empty buffer.
                InputSource::Stdin {
                    data: stdin_slot.take().unwrap_or_default(),
                }
            } else {
                InputSource::File(p.clone())
            }
        })
        .collect();

    Ok(sources)
}

/// Reopen fd 0 from the controlling terminal after piped stdin is consumed,
/// so crossterm's event loop can read keystrokes. No-op if no TTY is available.
///
/// Uses `ttyname()` on stderr/stdout to resolve the actual device path (e.g.
/// `/dev/ttys000`) rather than opening `/dev/tty`. On macOS, kqueue rejects
/// `/dev/tty` with EINVAL when mio tries to register it — the magic routing
/// device isn't pollable, but the real device node is.
///
/// Opened read+write because mio requires writable fds for interest
/// registration, and crossterm uses fd 0 directly when `isatty(0)` is true.
#[cfg(unix)]
fn reopen_stdin_from_tty() {
    use std::os::unix::io::AsRawFd;

    let tty_path = resolve_tty_path();
    let open_result = tty_path
        .as_deref()
        .or(Some("/dev/tty"))
        .and_then(|p| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(p)
                .ok()
        });
    if let Some(tty) = open_result {
        unsafe {
            libc::dup2(tty.as_raw_fd(), 0);
        }
    }
}

/// Resolve the controlling terminal's device path by calling `ttyname()` on
/// stderr, then stdout. Returns `None` if neither is a TTY.
#[cfg(unix)]
fn resolve_tty_path() -> Option<String> {
    for fd in [2, 1] {
        unsafe {
            let p = libc::ttyname(fd);
            if !p.is_null() {
                return Some(std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned());
            }
        }
    }
    None
}

#[cfg(not(unix))]
fn reopen_stdin_from_tty() {
    // TODO: Windows support via CONIN$
}
