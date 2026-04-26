use std::io::{self, BufReader, Cursor};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{self, Event};
use image::{AnimationDecoder, DynamicImage, GenericImageView};

use crate::input::detect::Detected;
use crate::input::InputSource;
use crate::theme::PeekThemeName;

use super::render;
use super::ImageConfig;

use crate::viewer::ui::{
    Action, Outcome, ViewMode, ViewerState, keys, render_themed_status_line, with_alternate_screen,
};

/// A single decoded animation frame with its display duration.
pub struct AnimFrame {
    pub image: DynamicImage,
    pub delay: Duration,
}

// ---------------------------------------------------------------------------
// Frame decoding
// ---------------------------------------------------------------------------

/// Animated image container format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnimFormat {
    Gif,
    Webp,
}

/// Detect GIF/WebP from either the file extension or the first magic bytes.
fn detect_format(source: &InputSource) -> Option<AnimFormat> {
    match source {
        InputSource::File(path) => {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            match ext.as_str() {
                "gif" => Some(AnimFormat::Gif),
                "webp" => Some(AnimFormat::Webp),
                _ => None,
            }
        }
        InputSource::Stdin { data } => sniff_anim_format(data),
    }
}

fn sniff_anim_format(data: &[u8]) -> Option<AnimFormat> {
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some(AnimFormat::Gif)
    } else if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some(AnimFormat::Webp)
    } else {
        None
    }
}

/// Collect `Frame` results from an AnimationDecoder into `AnimFrame`s.
fn collect_frames<'a, D: image::AnimationDecoder<'a>>(decoder: D) -> Result<Vec<AnimFrame>> {
    let mut frames = Vec::new();
    for frame_result in decoder.into_frames() {
        let frame = frame_result.context("failed to decode frame")?;
        let (numer, denom) = frame.delay().numer_denom_ms();
        let ms = if denom == 0 { 100 } else { numer / denom };
        let delay = Duration::from_millis(ms.max(20) as u64);
        let image = DynamicImage::ImageRgba8(frame.into_buffer());
        frames.push(AnimFrame { image, delay });
    }
    Ok(frames)
}

/// Decode all frames from an animated image (GIF or WebP).
/// Returns `None` if the source is not an animated format or has ≤1 frame.
pub fn decode_anim_frames(source: &InputSource) -> Result<Option<Vec<AnimFrame>>> {
    let Some(format) = detect_format(source) else {
        return Ok(None);
    };
    let frames = match (source, format) {
        (InputSource::File(path), AnimFormat::Gif) => {
            let reader = BufReader::new(std::fs::File::open(path).context("failed to open GIF")?);
            collect_frames(
                image::codecs::gif::GifDecoder::new(reader).context("failed to decode GIF")?,
            )?
        }
        (InputSource::File(path), AnimFormat::Webp) => {
            let reader = BufReader::new(std::fs::File::open(path).context("failed to open WebP")?);
            collect_frames(
                image::codecs::webp::WebPDecoder::new(reader).context("failed to decode WebP")?,
            )?
        }
        (InputSource::Stdin { data }, AnimFormat::Gif) => {
            let reader = Cursor::new(data.clone());
            collect_frames(
                image::codecs::gif::GifDecoder::new(reader).context("failed to decode GIF")?,
            )?
        }
        (InputSource::Stdin { data }, AnimFormat::Webp) => {
            let reader = Cursor::new(data.clone());
            collect_frames(
                image::codecs::webp::WebPDecoder::new(reader).context("failed to decode WebP")?,
            )?
        }
    };

    if frames.len() <= 1 {
        return Ok(None);
    }
    Ok(Some(frames))
}

/// Count animation frames without full pixel decoding.
/// Returns None for non-animated sources.
pub fn anim_frame_count(source: &InputSource) -> Option<usize> {
    let format = detect_format(source)?;
    let count = match (source, format) {
        (InputSource::File(path), AnimFormat::Gif) => {
            let reader = BufReader::new(std::fs::File::open(path).ok()?);
            image::codecs::gif::GifDecoder::new(reader).ok()?.into_frames().count()
        }
        (InputSource::File(path), AnimFormat::Webp) => {
            let reader = BufReader::new(std::fs::File::open(path).ok()?);
            image::codecs::webp::WebPDecoder::new(reader).ok()?.into_frames().count()
        }
        (InputSource::Stdin { data }, AnimFormat::Gif) => {
            let reader = Cursor::new(data.clone());
            image::codecs::gif::GifDecoder::new(reader).ok()?.into_frames().count()
        }
        (InputSource::Stdin { data }, AnimFormat::Webp) => {
            let reader = Cursor::new(data.clone());
            image::codecs::webp::WebPDecoder::new(reader).ok()?.into_frames().count()
        }
    };
    if count > 1 { Some(count) } else { None }
}

// ---------------------------------------------------------------------------
// Help keys for the animation viewer
// ---------------------------------------------------------------------------

const ACTIONS: &[(Action, &str)] = &[
    (Action::Quit,              "Quit"),
    (Action::PlayPause,         "Play / pause"),
    (Action::NextFrame,         "Next frame"),
    (Action::PrevFrame,         "Previous frame"),
    (Action::CycleBackground,   "Cycle background"),
    (Action::ScrollUp,          "Scroll up (info/help)"),
    (Action::ScrollDown,        "Scroll down (info/help)"),
    (Action::Top,               "Jump to top"),
    (Action::Bottom,            "Jump to bottom"),
    (Action::ToggleContentInfo, "Toggle content / file info"),
    (Action::SwitchInfo,        "File info"),
    (Action::ToggleHelp,        "Toggle help"),
    (Action::CycleTheme,        "Next theme"),
    (Action::SwitchToHex,       "Hex dump mode"),
];


// ---------------------------------------------------------------------------
// Interactive animated viewer
// ---------------------------------------------------------------------------

/// Interactive animated GIF/WebP viewer with frame-rate-driven playback.
pub fn view_animated(
    source: &InputSource,
    detected: &Detected,
    frames: Vec<AnimFrame>,
    config: ImageConfig,
    initial_theme: PeekThemeName,
) -> Result<()> {
    with_alternate_screen(|stdout| {
        run_animation_loop(stdout, source, detected, &frames, config, initial_theme)
    })
}

// ---------------------------------------------------------------------------
// Animation event loop
// ---------------------------------------------------------------------------

fn run_animation_loop(
    stdout: &mut io::Stdout,
    source: &InputSource,
    detected: &Detected,
    frames: &[AnimFrame],
    mut config: ImageConfig,
    initial_theme: PeekThemeName,
) -> Result<()> {
    let mut playing = true;
    let mut current_frame: usize = 0;
    let frame_count = frames.len();

    let content_lines = render_frame(&frames[current_frame], &config);
    let mut state = ViewerState::new(source, detected, initial_theme, content_lines, ACTIONS)?;

    let name = source.name().to_string();

    let redraw = |stdout: &mut io::Stdout,
                  state: &ViewerState,
                  frame_idx: usize,
                  playing: bool|
     -> Result<()> {
        let status = render_anim_status_line(&name, state, frame_idx, frame_count, playing);
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
                Event::Key(key) => {
                    let Some(action) = keys::dispatch(key, ACTIONS) else {
                        continue;
                    };
                    match state.apply(action) {
                        Outcome::Quit => return Ok(()),
                        Outcome::Redraw => redraw(stdout, &state, current_frame, playing)?,
                        Outcome::RecomputeContent => {
                            // Animation frames don't depend on theme — just redraw.
                            redraw(stdout, &state, current_frame, playing)?;
                        }
                        Outcome::Unhandled => match action {
                            Action::SwitchToHex => {
                                crate::viewer::hex::run_hex_loop(
                                    stdout,
                                    source,
                                    detected,
                                    state.current_theme,
                                    0,
                                    true,
                                )?;
                                last_advance = Instant::now();
                                redraw(stdout, &state, current_frame, playing)?;
                            }
                            Action::CycleBackground => {
                                config.background = config.background.next();
                                state.content_lines = render_frame(&frames[current_frame], &config);
                                redraw(stdout, &state, current_frame, playing)?;
                            }
                            Action::PlayPause => {
                                playing = !playing;
                                if playing {
                                    last_advance = Instant::now();
                                }
                                redraw(stdout, &state, current_frame, playing)?;
                            }
                            Action::NextFrame => {
                                current_frame = (current_frame + 1) % frame_count;
                                state.content_lines = render_frame(&frames[current_frame], &config);
                                last_advance = Instant::now();
                                redraw(stdout, &state, current_frame, playing)?;
                            }
                            Action::PrevFrame => {
                                current_frame = (current_frame + frame_count - 1) % frame_count;
                                state.content_lines = render_frame(&frames[current_frame], &config);
                                last_advance = Instant::now();
                                redraw(stdout, &state, current_frame, playing)?;
                            }
                            _ => {}
                        },
                    }
                }
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
    name: &str,
    state: &ViewerState,
    frame_idx: usize,
    frame_count: usize,
    playing: bool,
) -> String {
    let theme = &state.peek_theme;

    let play_icon = if playing { "\u{25b6}" } else { "\u{23f8}" };
    let frame_info = format!("Frame {}/{} {}", frame_idx + 1, frame_count, play_icon);

    let hints: &[&str] = if playing {
        &["h:help", "p:pause", "b:bg", "q:quit"]
    } else {
        &["h:help", "p:play", "n/N:step", "b:bg", "q:quit"]
    };

    render_themed_status_line(
        &[
            (name, theme.accent),
            (&frame_info, theme.label),
            (state.view_mode.label(), theme.label),
            (state.current_theme.cli_name(), theme.muted),
        ],
        hints,
        theme,
    )
}
