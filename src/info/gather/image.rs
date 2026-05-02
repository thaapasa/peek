//! Image metadata: dimensions, color, ICC profile, HDR marker, plus
//! delegation to [`super::exif`], [`super::xmp`], [`super::animation`].

use std::fs;
use std::path::Path;
use std::sync::Arc;

use ::image::ImageDecoder;

use super::super::FileExtras;
use super::{animation, binary, exif, xmp};
use crate::input::InputSource;

/// How many bytes from the head of an image we'll scan for XMP / HDR markers.
pub(super) const IMAGE_HEAD_SCAN: usize = 256 * 1024;

pub(super) fn gather_image_extras(source: &InputSource, magic_mime: Option<&str>) -> FileExtras {
    let Some(decoder) = image_decoder_for(source) else {
        return binary::binary_extras(magic_mime);
    };
    let head = read_source_head(source, IMAGE_HEAD_SCAN);
    let anim = match source {
        InputSource::File(path) => animation::animation_stats_path(path, magic_mime),
        InputSource::Stdin { data } => animation::animation_stats_bytes(data, magic_mime),
    };
    image_extras_from_decoder(decoder, &head, anim)
}

fn image_decoder_for(source: &InputSource) -> Option<Box<dyn ImageDecoder>> {
    match source {
        InputSource::File(path) => ::image::ImageReader::open(path)
            .ok()
            .and_then(|r| r.with_guessed_format().ok())
            .and_then(|r| r.into_decoder().ok())
            .map(|d| Box::new(d) as Box<dyn ImageDecoder>),
        InputSource::Stdin { data } => {
            ::image::ImageReader::new(std::io::Cursor::new(Arc::clone(data)))
                .with_guessed_format()
                .ok()
                .and_then(|r| r.into_decoder().ok())
                .map(|d| Box::new(d) as Box<dyn ImageDecoder>)
        }
    }
}

fn read_source_head(source: &InputSource, max: usize) -> Vec<u8> {
    let Ok(bs) = source.open_byte_source() else {
        return Vec::new();
    };
    bs.read_range(0, max).unwrap_or_default()
}

fn image_extras_from_decoder(
    mut decoder: Box<dyn ImageDecoder>,
    head: &[u8],
    animation: Option<super::super::AnimationStats>,
) -> FileExtras {
    let (width, height) = decoder.dimensions();
    let ct = decoder.color_type();
    let color_type = format!("{ct:?}");
    let bit_depth = (ct.bits_per_pixel() / ct.channel_count() as u16) as u8;

    let icc_profile = decoder
        .icc_profile()
        .ok()
        .flatten()
        .and_then(|p| icc_description(&p));

    let hdr_format = detect_hdr_bytes(head);
    let exif = exif::exif_fields_from_bytes(head);
    let xmp = xmp::xmp_fields_from_bytes(head);

    FileExtras::Image {
        width,
        height,
        color_type,
        bit_depth,
        hdr_format,
        icc_profile,
        animation,
        exif,
        xmp,
    }
}

/// Read up to `max` bytes from the head of `path`. Errors are swallowed —
/// callers handle a short / empty buffer the same way as a present one.
pub(super) fn read_head(path: &Path, max: usize) -> Vec<u8> {
    let Ok(mut file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = vec![0u8; max];
    let n = std::io::Read::read(&mut file, &mut buf).unwrap_or(0);
    buf.truncate(n);
    buf
}

/// Detect HDR format by scanning for known markers in file data. Currently
/// only matches Google's Ultra HDR gain map (XMP namespace `hdrgm:`).
fn detect_hdr_bytes(data: &[u8]) -> Option<String> {
    let slice = &data[..data.len().min(IMAGE_HEAD_SCAN)];
    if slice.windows(6).any(|w| w == b"hdrgm:") {
        return Some("Ultra HDR (gain map)".to_string());
    }
    None
}

/// Pull the human-readable description out of an embedded ICC profile.
///
/// Walks the profile's tag table for the `desc` tag and decodes either the
/// ICCv2 ASCII form or the ICCv4 `mluc` UTF-16BE form. Returns `None` if
/// the profile is malformed, the tag is missing, or the description is empty.
fn icc_description(profile: &[u8]) -> Option<String> {
    if profile.len() < 132 {
        return None;
    }
    let tag_count = u32::from_be_bytes(profile[128..132].try_into().ok()?) as usize;
    let table_start: usize = 132;
    let table_end: usize = table_start.checked_add(tag_count.checked_mul(12)?)?;
    if profile.len() < table_end {
        return None;
    }
    for i in 0..tag_count {
        let off = table_start + i * 12;
        if &profile[off..off + 4] != b"desc" {
            continue;
        }
        let data_off = u32::from_be_bytes(profile[off + 4..off + 8].try_into().ok()?) as usize;
        let data_size = u32::from_be_bytes(profile[off + 8..off + 12].try_into().ok()?) as usize;
        let data_end: usize = data_off.checked_add(data_size)?;
        if profile.len() < data_end || data_size < 8 {
            return None;
        }
        let data = &profile[data_off..data_off + data_size];
        let typ = &data[0..4];
        if typ == b"desc" && data.len() >= 12 {
            // ICCv2 desc: 8 bytes header + 4-byte ASCII length + ASCII string
            let strlen = u32::from_be_bytes(data[8..12].try_into().ok()?) as usize;
            if data.len() < 12 + strlen || strlen == 0 {
                return None;
            }
            let s = &data[12..12 + strlen];
            let s = s.split(|&b| b == 0).next().unwrap_or(s);
            return std::str::from_utf8(s).ok().map(|x| x.trim().to_string());
        }
        if typ == b"mluc" && data.len() >= 16 {
            // ICCv4 mluc: 8 bytes + record count (u32) + record size (u32)
            // First record: language(2) + country(2) + length(4) + offset(4)
            let rec_count = u32::from_be_bytes(data[8..12].try_into().ok()?);
            let rec_size = u32::from_be_bytes(data[12..16].try_into().ok()?) as usize;
            if rec_count < 1 || rec_size < 12 || data.len() < 16 + rec_size {
                return None;
            }
            let len = u32::from_be_bytes(data[20..24].try_into().ok()?) as usize;
            let off = u32::from_be_bytes(data[24..28].try_into().ok()?) as usize;
            let end: usize = off.checked_add(len)?;
            if data.len() < end {
                return None;
            }
            let utf16: Vec<u16> = data[off..off + len]
                .chunks(2)
                .filter_map(|c| {
                    if c.len() == 2 {
                        Some(u16::from_be_bytes([c[0], c[1]]))
                    } else {
                        None
                    }
                })
                .collect();
            let s = String::from_utf16_lossy(&utf16);
            let s = s.trim_matches('\0').trim().to_string();
            return Some(s);
        }
        return None;
    }
    None
}
