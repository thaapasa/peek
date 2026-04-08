use std::io::{self, BufReader};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal;
use image::{AnimationDecoder, DynamicImage, GenericImageView};
use syntect::highlighting::Color;

use crate::detect::FileType;
use crate::theme::PeekThemeName;

use super::render;
use super::ImageConfig;

use crate::viewer::ui::{
    KeyAction, ViewMode, ViewerState, compose_status_line, with_alternate_screen,
};

/// A single decoded animation frame with its display duration.
pub struct AnimFrame {
    pub image: DynamicImage,
    pub delay: Duration,
}

// ---------------------------------------------------------------------------
// Frame decoding
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Help keys for the animation viewer
// ---------------------------------------------------------------------------

const HELP_KEYS_ANIMATED: &[(&str, &str)] = &[
    ("q / Esc", "Quit"),
    ("p", "Play / pause"),
    ("n / Right", "Next frame"),
    ("N / Left", "Previous frame"),
    ("b", "Cycle background"),
    ("Up / Down", "Scroll (info/help)"),
    ("Home / End", "Top / bottom"),
    ("Tab", "Toggle content / file info"),
    ("i", "File info"),
    ("h / ?", "Toggle help"),
    ("t", "Next theme"),
];

// ---------------------------------------------------------------------------
// Interactive animated viewer
// ---------------------------------------------------------------------------

/// Interactive animated GIF/WebP viewer with frame-rate-driven playback.
pub fn view_animated(
    path: &Path,
    file_type: &FileType,
    frames: Vec<AnimFrame>,
    config: ImageConfig,
    initial_theme: PeekThemeName,
) -> Result<()> {
    with_alternate_screen(|stdout| {
        run_animation_loop(stdout, path, file_type, &frames, config, initial_theme)
    })
}

// ---------------------------------------------------------------------------
// Animation event loop
// ---------------------------------------------------------------------------

fn run_animation_loop(
    stdout: &mut io::Stdout,
    path: &Path,
    file_type: &FileType,
    frames: &[AnimFrame],
    mut config: ImageConfig,
    initial_theme: PeekThemeName,
) -> Result<()> {
    let mut playing = true;
    let mut current_frame: usize = 0;
    let frame_count = frames.len();

    let content_lines = render_frame(&frames[current_frame], &config);
    let mut state = ViewerState::new(path, file_type, initial_theme, content_lines, HELP_KEYS_ANIMATED)?;

    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");

    let redraw = |stdout: &mut io::Stdout,
                  state: &ViewerState,
                  frame_idx: usize,
                  playing: bool|
     -> Result<()> {
        let status = render_anim_status_line(filename, state, frame_idx, frame_count, playing);
        state.draw(stdout, &status)
    };

    redraw(stdout, &state, current_frame, playing)?;

    let mut last_advance = Instant::now();

    loop {
        let timeout = if playing && state.view_mode == ViewMode::Content {
            let elapsed = last_advance.elapsed();
            frames[current_frame].delay.saturating_sub(elapsed)
        } else {
            Duration::from_secs(86400)
        };

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => match state.handle_key(key) {
                    KeyAction::Quit => return Ok(()),
                    KeyAction::Redraw => {
                        redraw(stdout, &state, current_frame, playing)?;
                    }
                    KeyAction::ThemeChanged => {
                        // Animation content doesn't depend on theme — just redraw
                        redraw(stdout, &state, current_frame, playing)?;
                    }
                    KeyAction::Unhandled(key) => match key.code {
                        // Play/pause
                        KeyCode::Char('p') => {
                            playing = !playing;
                            if playing {
                                last_advance = Instant::now();
                            }
                            redraw(stdout, &state, current_frame, playing)?;
                        }
                        // Next frame
                        KeyCode::Char('n') | KeyCode::Right => {
                            current_frame = (current_frame + 1) % frame_count;
                            state.content_lines = render_frame(
                                &frames[current_frame], &config,
                            );
                            last_advance = Instant::now();
                            redraw(stdout, &state, current_frame, playing)?;
                        }
                        // Previous frame
                        KeyCode::Char('N') | KeyCode::Left => {
                            current_frame = (current_frame + frame_count - 1) % frame_count;
                            state.content_lines = render_frame(
                                &frames[current_frame], &config,
                            );
                            last_advance = Instant::now();
                            redraw(stdout, &state, current_frame, playing)?;
                        }
                        // Background cycling
                        KeyCode::Char('b') => {
                            config.background = config.background.next();
                            state.content_lines = render_frame(
                                &frames[current_frame], &config,
                            );
                            redraw(stdout, &state, current_frame, playing)?;
                        }
                        _ => {}
                    },
                },
                Event::Resize(_, _) => {
                    state.content_lines = render_frame(
                        &frames[current_frame], &config,
                    );
                    redraw(stdout, &state, current_frame, playing)?;
                }
                _ => {}
            }
        } else {
            // Timeout: advance to next frame
            current_frame = (current_frame + 1) % frame_count;
            state.content_lines = render_frame(
                &frames[current_frame], &config,
            );
            last_advance = Instant::now();
            if state.view_mode == ViewMode::Content {
                redraw(stdout, &state, current_frame, playing)?;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Frame rendering
// ---------------------------------------------------------------------------

fn render_frame(frame: &AnimFrame, config: &ImageConfig) -> Vec<String> {
    use super::glyph_atlas::{CELL_H, CELL_W};

    let mut term = render::TermSize::detect();
    term.rows = term.rows.saturating_sub(1);
    let img = render::add_margin(frame.image.clone(), config.margin);
    let (img_w, img_h) = img.dimensions();
    let (cols, rows) = render::contain_size(img_w, img_h, term, config.width);

    // Resize to target resolution before compositing so checkerboard
    // aligns to the glyph grid.
    let (px_w, px_h) = match config.mode {
        super::ImageMode::Ascii => (cols, rows),
        _ => (cols * CELL_W, rows * CELL_H),
    };
    let img = img.resize_exact(px_w, px_h, image::imageops::FilterType::Lanczos3);
    let img = render::composite_with_bg(img, config.background);

    match config.mode {
        super::ImageMode::Ascii => render::render_density(&img, cols, rows),
        _ => render::render_block_color(&img, cols, rows, config.mode),
    }
}

// ---------------------------------------------------------------------------
// Animation status line
// ---------------------------------------------------------------------------

fn render_anim_status_line(
    filename: &str,
    state: &ViewerState,
    frame_idx: usize,
    frame_count: usize,
    playing: bool,
) -> String {
    let theme = &state.peek_theme;

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
        fg(state.view_mode.label(), theme.label),
        fg("\u{2502}", theme.muted),
        fg(state.current_theme.cli_name(), theme.muted),
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

    let cols = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    let bg = theme.selection;
    format!(
        "\x1b[48;2;{};{};{}m{}\x1b[0m",
        bg.r, bg.g, bg.b,
        compose_status_line(&left, &hints, cols),
    )
}
