//! Animation stats for GIF / WebP — frame count, total duration, loop
//! count. Cheap header walks; no full pixel decode.

use std::fs;
use std::io::{BufReader, Cursor};
use std::path::Path;

use super::super::{AnimationStats, LoopCount};
use super::image::{IMAGE_HEAD_SCAN, read_head};

#[derive(Clone, Copy, PartialEq, Eq)]
enum AnimFmt {
    Gif,
    Webp,
}

fn animation_format(magic_mime: Option<&str>, head: &[u8]) -> Option<AnimFmt> {
    if let Some(mime) = magic_mime {
        match mime {
            "image/gif" => return Some(AnimFmt::Gif),
            "image/webp" => return Some(AnimFmt::Webp),
            _ => {}
        }
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Some(AnimFmt::Gif);
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return Some(AnimFmt::Webp);
    }
    None
}

pub(super) fn animation_stats_path(
    path: &Path,
    magic_mime: Option<&str>,
) -> Option<AnimationStats> {
    let head = read_head(path, 32);
    match animation_format(magic_mime, &head)? {
        AnimFmt::Gif => {
            let reader = BufReader::new(fs::File::open(path).ok()?);
            gif_animation_stats(reader)
        }
        AnimFmt::Webp => webp_animation_stats_bytes(&read_head(path, IMAGE_HEAD_SCAN)),
    }
}

pub(super) fn animation_stats_bytes(
    data: &[u8],
    magic_mime: Option<&str>,
) -> Option<AnimationStats> {
    match animation_format(magic_mime, data)? {
        AnimFmt::Gif => gif_animation_stats(Cursor::new(data)),
        AnimFmt::Webp => webp_animation_stats_bytes(data),
    }
}

/// Walks GIF frame headers via `gif::Decoder::next_frame_info`, which parses
/// each frame's image descriptor but skips the LZW-compressed pixel data.
fn gif_animation_stats<R: std::io::Read>(reader: R) -> Option<AnimationStats> {
    let mut decoder = gif::DecodeOptions::new().read_info(reader).ok()?;
    let loop_count = match decoder.repeat() {
        gif::Repeat::Infinite => Some(LoopCount::Infinite),
        gif::Repeat::Finite(n) => Some(LoopCount::Finite(n as u32)),
    };
    let mut frames = 0usize;
    let mut total_ms: u64 = 0;
    while let Some(info) = decoder.next_frame_info().ok().flatten() {
        // gif crate exposes `delay` in hundredths of a second.
        let delay = info.delay as u64 * 10;
        // GIF browsers clamp very-low delays to 100ms; we mirror that to
        // make the displayed total realistic.
        total_ms += delay.max(20);
        frames += 1;
    }
    if frames <= 1 {
        return None;
    }
    Some(AnimationStats {
        frame_count: Some(frames),
        total_duration_ms: Some(total_ms),
        loop_count,
    })
}

/// WebP animation stats from raw RIFF chunks. Walks top-level chunks for
/// `ANIM` (loop count) and `ANMF` (frame count + delay).
fn webp_animation_stats_bytes(data: &[u8]) -> Option<AnimationStats> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return None;
    }
    let mut offset = 12;
    let mut frame_count = 0usize;
    let mut total_ms: u64 = 0;
    let mut loop_count: Option<LoopCount> = None;
    while offset + 8 <= data.len() {
        let chunk_id = &data[offset..offset + 4];
        let chunk_size =
            u32::from_le_bytes(data[offset + 4..offset + 8].try_into().ok()?) as usize;
        let body_start = offset + 8;
        let body_end = body_start.checked_add(chunk_size)?;
        if body_end > data.len() {
            break;
        }
        match chunk_id {
            b"ANIM" if chunk_size >= 6 => {
                let loops =
                    u16::from_le_bytes(data[body_start + 4..body_start + 6].try_into().ok()?);
                loop_count = Some(if loops == 0 {
                    LoopCount::Infinite
                } else {
                    LoopCount::Finite(loops as u32)
                });
            }
            b"ANMF" if chunk_size >= 16 => {
                // Duration is a 24-bit LE value at offset 12 of the ANMF body.
                let d = &data[body_start + 12..body_start + 15];
                let dur = u32::from_le_bytes([d[0], d[1], d[2], 0]) as u64;
                total_ms += dur.max(20);
                frame_count += 1;
            }
            _ => {}
        }
        // RIFF chunks are word-aligned: pad to even length.
        let pad = chunk_size & 1;
        offset = body_end + pad;
    }
    if frame_count <= 1 {
        return None;
    }
    Some(AnimationStats {
        frame_count: Some(frame_count),
        total_duration_ms: Some(total_ms),
        loop_count,
    })
}
