use std::io::{self, BufReader};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::{
    cursor, execute,
    event::{self, Event, KeyCode, KeyModifiers},
    terminal,
};
use image::{AnimationDecoder, DynamicImage, GenericImageView};
use syntect::highlighting::Color;

use crate::detect::FileType;
use crate::theme::{PeekTheme, PeekThemeName};

use super::render;
use super::{Background, ImageMode};

use crate::viewer::interactive::{
    ViewMode, content_rows, current_scroll, draw, lines_for, make_peek_theme,
    render_help_with_keys, scroll_mut, strip_ansi_width,
};

/// A single decoded animation frame with its display duration.
pub struct AnimFrame {
    pub image: DynamicImage,
    pub delay: Duration,
}

/// Decode all frames from an animated image (GIF or WebP).
/// Returns `None` if the file is not animated or has ≤1 frame.
pub fn decode_anim_frames(path: &Path) -> Result<Option<Vec<AnimFrame>>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "gif" => decode_gif_frames(path),
        "webp" => decode_webp_frames(path),
        _ => Ok(None),
    }
}

fn decode_gif_frames(path: &Path) -> Result<Option<Vec<AnimFrame>>> {
    let file = std::fs::File::open(path).context("failed to open GIF")?;
    let decoder = image::codecs::gif::GifDecoder::new(BufReader::new(file))
        .context("failed to decode GIF")?;
    let frames_iter = decoder.into_frames();

    let mut frames = Vec::new();
    for frame_result in frames_iter {
        let frame = frame_result.context("failed to decode GIF frame")?;
        let (numer, denom) = frame.delay().numer_denom_ms();
        let ms = if denom == 0 { 100 } else { numer / denom };
        let delay = Duration::from_millis(ms.max(20) as u64);
        let image = DynamicImage::ImageRgba8(frame.into_buffer());
        frames.push(AnimFrame { image, delay });
    }

    if frames.len() <= 1 {
        return Ok(None);
    }

    Ok(Some(frames))
}

fn decode_webp_frames(path: &Path) -> Result<Option<Vec<AnimFrame>>> {
    let file = std::fs::File::open(path).context("failed to open WebP")?;
    let decoder = image::codecs::webp::WebPDecoder::new(BufReader::new(file))
        .context("failed to decode WebP")?;
    let frames_iter = decoder.into_frames();

    let mut frames = Vec::new();
    for frame_result in frames_iter {
        let frame = frame_result.context("failed to decode WebP frame")?;
        let (numer, denom) = frame.delay().numer_denom_ms();
        let ms = if denom == 0 { 100 } else { numer / denom };
        let delay = Duration::from_millis(ms.max(20) as u64);
        let image = DynamicImage::ImageRgba8(frame.into_buffer());
        frames.push(AnimFrame { image, delay });
    }

    if frames.len() <= 1 {
        return Ok(None);
    }

    Ok(Some(frames))
}

/// Count animation frames without full pixel decoding.
/// Returns None for non-animated files.
pub fn anim_frame_count(path: &Path) -> Option<usize> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "gif" => gif_frame_count(path),
        "webp" => webp_frame_count(path),
        _ => None,
    }
}

fn gif_frame_count(path: &Path) -> Option<usize> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = image::codecs::gif::GifDecoder::new(BufReader::new(file)).ok()?;
    let count = decoder.into_frames().count();
    if count > 1 { Some(count) } else { None }
}

fn webp_frame_count(path: &Path) -> Option<usize> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = image::codecs::webp::WebPDecoder::new(BufReader::new(file)).ok()?;
    let count = decoder.into_frames().count();
    if count > 1 { Some(count) } else { None }
}

const HELP_KEYS_ANIMATED: &[(&str, &str)] = &[
    ("q / Esc", "Quit"),
    ("p", "Play / pause"),
    ("n / Right", "Next frame"),
    ("N / Left", "Previous frame"),
    ("b", "Cycle background"),
    ("Tab", "Toggle content / file info"),
    ("i", "File info"),
    ("h / ?", "Toggle help"),
    ("t", "Next theme"),
];

/// Interactive animated GIF viewer with frame-rate-driven playback.
#[allow(clippy::too_many_arguments)]
pub fn view_animated(
    path: &Path,
    file_type: &FileType,
    frames: Vec<AnimFrame>,
    mode: ImageMode,
    forced_width: u32,
    background: Background,
    margin: u32,
    initial_theme: PeekThemeName,
) -> Result<()> {
    let mut stdout = io::stdout();

    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::MoveTo(0, 0),
        cursor::Hide,
    )?;
    terminal::enable_raw_mode()?;

    let result = run_animation_loop(
        &mut stdout,
        path,
        file_type,
        &frames,
        mode,
        forced_width,
        background,
        margin,
        initial_theme,
    );

    terminal::disable_raw_mode()?;
    execute!(
        stdout,
        cursor::Show,
        terminal::LeaveAlternateScreen,
    )?;

    result
}

#[allow(clippy::too_many_arguments)]
fn run_animation_loop(
    stdout: &mut io::Stdout,
    path: &Path,
    file_type: &FileType,
    frames: &[AnimFrame],
    mode: ImageMode,
    forced_width: u32,
    mut background: Background,
    margin: u32,
    initial_theme: PeekThemeName,
) -> Result<()> {
    let mut view_mode = ViewMode::Content;
    let mut current_theme = initial_theme;
    let mut peek_theme = make_peek_theme(current_theme);
    let mut playing = true;
    let mut current_frame: usize = 0;
    let frame_count = frames.len();

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?");

    // Render initial frame
    let mut content_lines = render_frame(&frames[current_frame], mode, forced_width, background, margin);

    // File info and help
    let file_info = crate::info::gather(path, file_type)?;
    let mut info_lines = crate::info::render(&file_info, &peek_theme);
    let mut help_lines = render_help_with_keys(&peek_theme, current_theme, HELP_KEYS_ANIMATED);

    // Scroll offsets
    let mut content_scroll: usize = 0;
    let mut info_scroll: usize = 0;
    let mut help_scroll: usize = 0;

    let redraw = |stdout: &mut io::Stdout,
                  mode: ViewMode,
                  scroll: usize,
                  content: &[String],
                  info: &[String],
                  help: &[String],
                  theme: &PeekTheme,
                  theme_name: PeekThemeName,
                  frame_idx: usize,
                  playing: bool|
     -> Result<()> {
        let status = render_anim_status_line(
            filename, mode, theme_name, theme, frame_idx, frame_count, playing,
        );
        draw(stdout, mode, content, info, help, scroll, &status)
    };

    redraw(
        stdout, view_mode, content_scroll,
        &content_lines, &info_lines, &help_lines,
        &peek_theme, current_theme, current_frame, playing,
    )?;

    let mut last_advance = Instant::now();

    loop {
        let timeout = if playing && view_mode == ViewMode::Content {
            let elapsed = last_advance.elapsed();
            let frame_delay = frames[current_frame].delay;
            frame_delay.saturating_sub(elapsed)
        } else {
            Duration::from_secs(86400)
        };

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }

                    // Play/pause
                    KeyCode::Char('p') => {
                        playing = !playing;
                        if playing {
                            last_advance = Instant::now();
                        }
                        redraw(
                            stdout, view_mode,
                            current_scroll(view_mode, content_scroll, info_scroll, help_scroll),
                            &content_lines, &info_lines, &help_lines,
                            &peek_theme, current_theme, current_frame, playing,
                        )?;
                    }

                    // Next frame
                    KeyCode::Char('n') | KeyCode::Right => {
                        current_frame = (current_frame + 1) % frame_count;
                        content_lines = render_frame(&frames[current_frame], mode, forced_width, background, margin);
                        last_advance = Instant::now();
                        redraw(
                            stdout, view_mode, content_scroll,
                            &content_lines, &info_lines, &help_lines,
                            &peek_theme, current_theme, current_frame, playing,
                        )?;
                    }

                    // Previous frame
                    KeyCode::Char('N') | KeyCode::Left => {
                        current_frame = (current_frame + frame_count - 1) % frame_count;
                        content_lines = render_frame(&frames[current_frame], mode, forced_width, background, margin);
                        last_advance = Instant::now();
                        redraw(
                            stdout, view_mode, content_scroll,
                            &content_lines, &info_lines, &help_lines,
                            &peek_theme, current_theme, current_frame, playing,
                        )?;
                    }

                    // Background cycling
                    KeyCode::Char('b') => {
                        background = background.next();
                        content_lines = render_frame(&frames[current_frame], mode, forced_width, background, margin);
                        redraw(
                            stdout, view_mode, content_scroll,
                            &content_lines, &info_lines, &help_lines,
                            &peek_theme, current_theme, current_frame, playing,
                        )?;
                    }

                    // View switching
                    KeyCode::Tab => {
                        view_mode = match view_mode {
                            ViewMode::Content => ViewMode::Info,
                            ViewMode::Info | ViewMode::Help => ViewMode::Content,
                        };
                        let scroll = current_scroll(view_mode, content_scroll, info_scroll, help_scroll);
                        redraw(
                            stdout, view_mode, scroll,
                            &content_lines, &info_lines, &help_lines,
                            &peek_theme, current_theme, current_frame, playing,
                        )?;
                    }
                    KeyCode::Char('i') => {
                        if view_mode != ViewMode::Info {
                            view_mode = ViewMode::Info;
                            redraw(
                                stdout, view_mode, info_scroll,
                                &content_lines, &info_lines, &help_lines,
                                &peek_theme, current_theme, current_frame, playing,
                            )?;
                        }
                    }
                    KeyCode::Char('h') | KeyCode::Char('?') => {
                        view_mode = if view_mode == ViewMode::Help {
                            ViewMode::Content
                        } else {
                            ViewMode::Help
                        };
                        let scroll = current_scroll(view_mode, content_scroll, info_scroll, help_scroll);
                        redraw(
                            stdout, view_mode, scroll,
                            &content_lines, &info_lines, &help_lines,
                            &peek_theme, current_theme, current_frame, playing,
                        )?;
                    }

                    // Theme cycling
                    KeyCode::Char('t') => {
                        current_theme = current_theme.next();
                        peek_theme = make_peek_theme(current_theme);
                        info_lines = crate::info::render(&file_info, &peek_theme);
                        help_lines = render_help_with_keys(&peek_theme, current_theme, HELP_KEYS_ANIMATED);
                        let scroll = current_scroll(view_mode, content_scroll, info_scroll, help_scroll);
                        redraw(
                            stdout, view_mode, scroll,
                            &content_lines, &info_lines, &help_lines,
                            &peek_theme, current_theme, current_frame, playing,
                        )?;
                    }

                    // Scrolling (for info/help views)
                    KeyCode::Up | KeyCode::Char('k') => {
                        if view_mode != ViewMode::Content {
                            let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                            *s = s.saturating_sub(1);
                            redraw(
                                stdout, view_mode, *s,
                                &content_lines, &info_lines, &help_lines,
                                &peek_theme, current_theme, current_frame, playing,
                            )?;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if view_mode != ViewMode::Content {
                            let lines = lines_for(view_mode, &content_lines, &info_lines, &help_lines);
                            let rows = content_rows();
                            let max = lines.len().saturating_sub(rows);
                            let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                            *s = (*s + 1).min(max);
                            redraw(
                                stdout, view_mode, *s,
                                &content_lines, &info_lines, &help_lines,
                                &peek_theme, current_theme, current_frame, playing,
                            )?;
                        }
                    }
                    KeyCode::PageUp => {
                        if view_mode != ViewMode::Content {
                            let rows = content_rows();
                            let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                            *s = s.saturating_sub(rows.saturating_sub(1));
                            redraw(
                                stdout, view_mode, *s,
                                &content_lines, &info_lines, &help_lines,
                                &peek_theme, current_theme, current_frame, playing,
                            )?;
                        }
                    }
                    KeyCode::PageDown | KeyCode::Char(' ') => {
                        if view_mode != ViewMode::Content {
                            let lines = lines_for(view_mode, &content_lines, &info_lines, &help_lines);
                            let rows = content_rows();
                            let max = lines.len().saturating_sub(rows);
                            let s = scroll_mut(view_mode, &mut content_scroll, &mut info_scroll, &mut help_scroll);
                            *s = (*s + rows.saturating_sub(1)).min(max);
                            redraw(
                                stdout, view_mode, *s,
                                &content_lines, &info_lines, &help_lines,
                                &peek_theme, current_theme, current_frame, playing,
                            )?;
                        }
                    }

                    _ => {}
                },
                Event::Resize(_, _) => {
                    content_lines = render_frame(&frames[current_frame], mode, forced_width, background, margin);
                    let scroll = current_scroll(view_mode, content_scroll, info_scroll, help_scroll);
                    redraw(
                        stdout, view_mode, scroll,
                        &content_lines, &info_lines, &help_lines,
                        &peek_theme, current_theme, current_frame, playing,
                    )?;
                }
                _ => {}
            }
        } else {
            // Timeout: advance to next frame
            current_frame = (current_frame + 1) % frame_count;
            content_lines = render_frame(&frames[current_frame], mode, forced_width, background, margin);
            last_advance = Instant::now();
            if view_mode == ViewMode::Content {
                redraw(
                    stdout, view_mode, content_scroll,
                    &content_lines, &info_lines, &help_lines,
                    &peek_theme, current_theme, current_frame, playing,
                )?;
            }
        }
    }
}

fn render_frame(
    frame: &AnimFrame,
    mode: ImageMode,
    forced_width: u32,
    bg: Background,
    margin: u32,
) -> Vec<String> {
    let mut term = render::TermSize::detect();
    term.rows = term.rows.saturating_sub(1);
    let img = render::add_margin(frame.image.clone(), margin);
    let img = render::composite_with_bg(img, bg);
    let (img_w, img_h) = img.dimensions();
    let (cols, rows) = render::contain_size(img_w, img_h, term, forced_width);
    match mode {
        ImageMode::Ascii => render::render_density(&img, cols, rows),
        _ => render::render_block_color(&img, cols, rows, mode),
    }
}

fn render_anim_status_line(
    filename: &str,
    mode: ViewMode,
    theme_name: PeekThemeName,
    theme: &PeekTheme,
    frame_idx: usize,
    frame_count: usize,
    playing: bool,
) -> String {
    let fg = |text: &str, color: Color| -> String {
        format!("\x1b[38;2;{};{};{}m{}", color.r, color.g, color.b, text)
    };

    let play_icon = if playing { "\u{25b6}" } else { "\u{23f8}" };
    let frame_info = format!("Frame {}/{} {}", frame_idx + 1, frame_count, play_icon);

    let left = format!(
        " {} {} {} {} {} {} {}",
        fg(filename, theme.accent),
        fg("\u{2502}", theme.muted),
        fg(&frame_info, theme.label),
        fg("\u{2502}", theme.muted),
        fg(mode.label(), theme.label),
        fg("\u{2502}", theme.muted),
        fg(theme_name.cli_name(), theme.muted),
    );

    let hints = if playing {
        format!(
            "{}  {}  {}  {} ",
            fg("h:help", theme.muted),
            fg("p:pause", theme.muted),
            fg("b:bg", theme.muted),
            fg("q:quit", theme.muted),
        )
    } else {
        format!(
            "{}  {}  {}  {}  {} ",
            fg("h:help", theme.muted),
            fg("p:play", theme.muted),
            fg("n/N:step", theme.muted),
            fg("b:bg", theme.muted),
            fg("q:quit", theme.muted),
        )
    };

    let left_visible = strip_ansi_width(&left);
    let hints_visible = strip_ansi_width(&hints);
    let cols = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);

    let gap = cols.saturating_sub(left_visible + hints_visible);
    let padding = " ".repeat(gap);

    let bg = theme.selection;
    format!(
        "\x1b[48;2;{};{};{}m{}{}{}\x1b[0m",
        bg.r, bg.g, bg.b,
        left, padding, hints,
    )
}
