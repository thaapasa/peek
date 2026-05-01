use std::fs;
use std::io::{BufReader, Cursor};
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use image::ImageDecoder;

use super::{
    AnimationStats, Encoding, FileExtras, FileInfo, IndentStyle, LineEndings, LoopCount,
    StructuredStats, TextStats, TopLevelKind, format_permissions_from_meta,
};
use crate::input::InputSource;
use crate::input::detect::{Detected, FileType, StructuredFormat};
use crate::input::mime;

/// Chunk size for streaming text-extras counting.
const TEXT_SCAN_CHUNK: usize = 64 * 1024;

/// How many bytes from the head of an image we'll scan for XMP / HDR markers.
const IMAGE_HEAD_SCAN: usize = 256 * 1024;

/// Gather metadata for the given input source and detection result.
///
/// `detected.magic_mime` is reused (no re-read of the file) to build the
/// MIME list and to detect extension/content mismatches.
pub fn gather(source: &InputSource, detected: &Detected) -> Result<FileInfo> {
    match source {
        InputSource::File(path) => gather_file(path, detected),
        InputSource::Stdin { data } => Ok(gather_stdin(data, detected)),
    }
}

fn gather_file(path: &Path, detected: &Detected) -> Result<FileInfo> {
    let meta = fs::metadata(path)?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let display_path = path.to_string_lossy().into_owned();

    let mimes = mime::mimes_for_path(
        &detected.file_type,
        Some(path),
        detected.magic_mime.as_deref(),
    );
    let warnings = collect_warnings(Some(path), detected);

    let permissions = format_permissions_from_meta(&meta);
    let extras = gather_extras(path, &detected.file_type, detected.magic_mime.as_deref());

    Ok(FileInfo {
        file_name,
        path: display_path,
        size_bytes: meta.len(),
        mimes,
        warnings,
        modified: meta.modified().ok(),
        created: meta.created().ok(),
        permissions,
        extras,
    })
}

fn gather_stdin(data: &Arc<[u8]>, detected: &Detected) -> FileInfo {
    let mimes = mime::mimes_for_path(&detected.file_type, None, detected.magic_mime.as_deref());
    let warnings = collect_warnings(None, detected);
    let extras = gather_extras_stdin(data, &detected.file_type, detected.magic_mime.as_deref());

    FileInfo {
        file_name: "<stdin>".to_string(),
        path: "<stdin>".to_string(),
        size_bytes: data.len() as u64,
        mimes,
        warnings,
        modified: None,
        created: None,
        permissions: None,
        extras,
    }
}

/// Build the warnings list. Currently: extension-mismatch only (file path).
fn collect_warnings(path: Option<&Path>, detected: &Detected) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(p) = path
        && let Some(w) = mime::extension_mismatch(p, detected.magic_mime.as_deref())
    {
        warnings.push(w);
    }
    warnings
}

fn gather_extras_stdin(
    data: &Arc<[u8]>,
    file_type: &FileType,
    magic_mime: Option<&str>,
) -> FileExtras {
    let stdin_source = InputSource::Stdin {
        data: Arc::clone(data),
    };
    match file_type {
        FileType::SourceCode { .. } => match gather_text_stats_streaming(&stdin_source) {
            Some(stats) => FileExtras::Text(stats),
            None => binary_extras(magic_mime),
        },
        FileType::Svg => match gather_text_stats_streaming(&stdin_source) {
            Some(stats) => svg_extras(stats, data),
            None => binary_extras(magic_mime),
        },
        FileType::Structured(fmt) => structured_extras(*fmt, data),
        FileType::Image => gather_image_extras_from_bytes(data, magic_mime),
        FileType::Binary => binary_extras(magic_mime),
    }
}

fn gather_extras(path: &Path, file_type: &FileType, magic_mime: Option<&str>) -> FileExtras {
    match file_type {
        FileType::Image => gather_image_extras(path, magic_mime),
        FileType::SourceCode { .. } => {
            match gather_text_stats_streaming(&InputSource::File(path.to_path_buf())) {
                Some(stats) => FileExtras::Text(stats),
                None => binary_extras(magic_mime),
            }
        }
        FileType::Svg => {
            let source = InputSource::File(path.to_path_buf());
            match (gather_text_stats_streaming(&source), source.read_bytes()) {
                (Some(stats), Ok(bytes)) => svg_extras(stats, &bytes),
                _ => binary_extras(magic_mime),
            }
        }
        FileType::Structured(fmt) => match fs::read(path) {
            Ok(bytes) => structured_extras(*fmt, &bytes),
            Err(_) => FileExtras::Structured {
                format_name: structured_format_name(*fmt),
                stats: None,
            },
        },
        FileType::Binary => binary_extras(magic_mime),
    }
}

// ---------------------------------------------------------------------------
// Image extras
// ---------------------------------------------------------------------------

fn gather_image_extras_from_bytes(data: &Arc<[u8]>, magic_mime: Option<&str>) -> FileExtras {
    let decoder = match image::ImageReader::new(std::io::Cursor::new(data.as_ref()))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_decoder().ok())
    {
        Some(d) => d,
        None => return binary_extras(magic_mime),
    };
    image_extras_from_decoder(decoder, data, magic_mime, |mime| {
        animation_stats_bytes(data, mime)
    })
}

fn gather_image_extras(path: &Path, magic_mime: Option<&str>) -> FileExtras {
    let decoder = match image::ImageReader::open(path)
        .ok()
        .and_then(|r| r.with_guessed_format().ok())
        .and_then(|r| r.into_decoder().ok())
    {
        Some(d) => d,
        None => return binary_extras(magic_mime),
    };
    let head = read_head(path, IMAGE_HEAD_SCAN);
    image_extras_from_decoder(decoder, &head, magic_mime, |mime| {
        animation_stats_path(path, mime)
    })
}

fn image_extras_from_decoder<D, F>(
    mut decoder: D,
    head: &[u8],
    magic_mime: Option<&str>,
    anim_fn: F,
) -> FileExtras
where
    D: ImageDecoder,
    F: FnOnce(Option<&str>) -> Option<AnimationStats>,
{
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
    let animation = anim_fn(magic_mime);
    let exif = exif_fields_from_bytes(head);
    let xmp = xmp_fields_from_bytes(head);

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

fn read_head(path: &Path, max: usize) -> Vec<u8> {
    let Ok(mut file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = vec![0u8; max];
    let n = std::io::Read::read(&mut file, &mut buf).unwrap_or(0);
    buf.truncate(n);
    buf
}

/// Detect HDR format by scanning for known markers in file data.
fn detect_hdr_bytes(data: &[u8]) -> Option<String> {
    let slice = &data[..data.len().min(IMAGE_HEAD_SCAN)];
    if slice.windows(6).any(|w| w == b"hdrgm:") {
        return Some("Ultra HDR (gain map)".to_string());
    }
    None
}

// ---------------------------------------------------------------------------
// EXIF
// ---------------------------------------------------------------------------

fn exif_fields_from_bytes(data: &[u8]) -> Vec<(String, String)> {
    let mut cursor = Cursor::new(data);
    exif_fields(&mut cursor)
}

fn exif_fields<R: std::io::BufRead + std::io::Seek>(reader: &mut R) -> Vec<(String, String)> {
    let exif_reader = exif::Reader::new();
    let exif = match exif_reader.read_from_container(reader) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    // Fields to extract, in display order
    const FIELDS: &[(exif::Tag, &str)] = &[
        (exif::Tag::Make, "Camera Make"),
        (exif::Tag::Model, "Camera Model"),
        (exif::Tag::LensModel, "Lens"),
        (exif::Tag::Orientation, "Orientation"),
        (exif::Tag::XResolution, "X Resolution"),
        (exif::Tag::YResolution, "Y Resolution"),
        (exif::Tag::ResolutionUnit, "Resolution Unit"),
        (exif::Tag::ExposureTime, "Exposure"),
        (exif::Tag::FNumber, "Aperture"),
        (exif::Tag::PhotographicSensitivity, "ISO"),
        (exif::Tag::FocalLength, "Focal Length"),
        (exif::Tag::FocalLengthIn35mmFilm, "Focal Length (35mm)"),
        (exif::Tag::ExposureBiasValue, "Exposure Bias"),
        (exif::Tag::MeteringMode, "Metering"),
        (exif::Tag::Flash, "Flash"),
        (exif::Tag::WhiteBalance, "White Balance"),
        (exif::Tag::DateTimeOriginal, "Date Taken"),
        (exif::Tag::Software, "Software"),
        (exif::Tag::ImageDescription, "Description"),
        (exif::Tag::Artist, "Artist"),
        (exif::Tag::Copyright, "Copyright"),
        (exif::Tag::GPSLatitude, "GPS Latitude"),
        (exif::Tag::GPSLongitude, "GPS Longitude"),
        (exif::Tag::GPSAltitude, "GPS Altitude"),
    ];

    let mut result = Vec::new();
    for &(tag, label) in FIELDS {
        if let Some(field) = exif.get_field(tag, exif::In::PRIMARY) {
            let value = field.display_value().with_unit(&exif).to_string();
            let value = value.trim().to_string();
            if !value.is_empty() {
                result.push((label.to_string(), value));
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// XMP — scrape common Dublin Core / xmp tags from the head bytes
// ---------------------------------------------------------------------------

fn xmp_fields_from_bytes(data: &[u8]) -> Vec<(String, String)> {
    // XMP packet is wrapped in `<?xpacket begin="..." ...?>` markers. Find
    // the packet boundaries; bail if not present.
    let needle_start = b"<x:xmpmeta";
    let needle_end = b"</x:xmpmeta>";
    let start = match data.windows(needle_start.len()).position(|w| w == needle_start) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let end = match data[start..]
        .windows(needle_end.len())
        .position(|w| w == needle_end)
    {
        Some(p) => start + p + needle_end.len(),
        None => return Vec::new(),
    };
    let packet = match std::str::from_utf8(&data[start..end]) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    // Tags to pull out. `(label, [candidate XML element names])` — XMP uses
    // namespaced names; we scan for any of the candidates as substrings.
    const TAGS: &[(&str, &[&str])] = &[
        ("Title", &["dc:title"]),
        ("Subject", &["dc:subject"]),
        ("Description", &["dc:description"]),
        ("Creator", &["dc:creator"]),
        ("Rights", &["dc:rights"]),
        ("Rating", &["xmp:Rating", "MicrosoftPhoto:Rating"]),
        ("Label", &["xmp:Label"]),
        ("Keywords", &["dc:subject"]),
    ];

    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (label, candidates) in TAGS {
        if !seen.insert(*label) {
            continue;
        }
        for tag in *candidates {
            if let Some(value) = xmp_extract_tag(packet, tag) {
                let value = value.trim();
                if !value.is_empty() {
                    result.push((label.to_string(), value.to_string()));
                    break;
                }
            }
        }
    }
    result
}

/// Pull the inner text of an XMP element, joining `rdf:li` items with
/// commas (XMP often stores text in `<rdf:Alt>` or `<rdf:Bag>` containers).
fn xmp_extract_tag(packet: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = packet.find(&open)?;
    let after_open = &packet[start..];
    // Skip to end of opening tag
    let close_bracket = after_open.find('>')?;
    let body_start = start + close_bracket + 1;
    let close = format!("</{tag}>");
    let close_at = packet[body_start..].find(&close)?;
    let inner = &packet[body_start..body_start + close_at];

    // Collect rdf:li items if present, otherwise return inner text.
    let mut items = Vec::new();
    let mut cursor = inner;
    while let Some(li_start) = cursor.find("<rdf:li") {
        let after = &cursor[li_start..];
        let Some(open_end) = after.find('>') else {
            break;
        };
        // Self-closing `<rdf:li/>` — no content; skip past it.
        if after.as_bytes().get(open_end.saturating_sub(1)) == Some(&b'/') {
            cursor = &cursor[li_start + open_end + 1..];
            continue;
        }
        let item_start = li_start + open_end + 1;
        let Some(item_end) = cursor[item_start..].find("</rdf:li>") else {
            break;
        };
        let text = &cursor[item_start..item_start + item_end];
        if !text.trim().is_empty() {
            items.push(text.trim().to_string());
        }
        cursor = &cursor[item_start + item_end + "</rdf:li>".len()..];
    }
    if !items.is_empty() {
        return Some(items.join(", "));
    }
    // Inner is just an empty `<rdf:Alt>` / `<rdf:Bag>` / `<rdf:Seq>` shell
    // — treat as empty so the field gets dropped by the caller.
    if inner.contains("<rdf:") {
        return None;
    }
    Some(inner.trim().to_string())
}

// ---------------------------------------------------------------------------
// ICC profile — extract description from the profile's `desc` tag
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Animation stats — frame count + duration + loop count
// ---------------------------------------------------------------------------

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

fn animation_stats_path(path: &Path, magic_mime: Option<&str>) -> Option<AnimationStats> {
    let head = read_head(path, 32);
    match animation_format(magic_mime, &head)? {
        AnimFmt::Gif => {
            let reader = BufReader::new(fs::File::open(path).ok()?);
            gif_animation_stats(reader)
        }
        AnimFmt::Webp => webp_animation_stats_bytes(&read_head(path, IMAGE_HEAD_SCAN)),
    }
}

fn animation_stats_bytes(data: &[u8], magic_mime: Option<&str>) -> Option<AnimationStats> {
    match animation_format(magic_mime, data)? {
        AnimFmt::Gif => gif_animation_stats(Cursor::new(data)),
        AnimFmt::Webp => webp_animation_stats_bytes(data),
    }
}

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

/// WebP animation stats from raw RIFF chunks. We avoid full decode and just
/// walk top-level chunks for ANIM (loop count) and ANMF (frame count + delay).
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
                let loops = u16::from_le_bytes(
                    data[body_start + 4..body_start + 6].try_into().ok()?,
                );
                loop_count = Some(if loops == 0 {
                    LoopCount::Infinite
                } else {
                    LoopCount::Finite(loops as u32)
                });
            }
            b"ANMF" if chunk_size >= 16 => {
                // duration is a 24-bit LE value at offset 12 of the ANMF body
                let d = &data[body_start + 12..body_start + 15];
                let dur = u32::from_le_bytes([d[0], d[1], d[2], 0]) as u64;
                total_ms += dur.max(20);
                frame_count += 1;
            }
            _ => {}
        }
        // Chunks are word-aligned: pad to even length
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

// ---------------------------------------------------------------------------
// SVG-specific extras (alongside text stats)
// ---------------------------------------------------------------------------

fn svg_extras(text: TextStats, bytes: &[u8]) -> FileExtras {
    let s = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            return FileExtras::Svg {
                text,
                view_box: None,
                declared_width: None,
                declared_height: None,
                path_count: 0,
                group_count: 0,
                rect_count: 0,
                circle_count: 0,
                text_count: 0,
                has_script: false,
                has_external_href: false,
            };
        }
    };

    let view_box = svg_root_attr(s, "viewBox");
    let declared_width = svg_root_attr(s, "width");
    let declared_height = svg_root_attr(s, "height");
    let path_count = count_open_tag(s, "path");
    let group_count = count_open_tag(s, "g");
    let rect_count = count_open_tag(s, "rect");
    let circle_count = count_open_tag(s, "circle");
    let text_count = count_open_tag(s, "text");
    let has_script = s.contains("<script");
    let has_external_href = svg_has_external_href(s);

    FileExtras::Svg {
        text,
        view_box,
        declared_width,
        declared_height,
        path_count,
        group_count,
        rect_count,
        circle_count,
        text_count,
        has_script,
        has_external_href,
    }
}

fn svg_root_attr(svg: &str, attr: &str) -> Option<String> {
    let svg_open = svg.find("<svg")?;
    let after = &svg[svg_open..];
    let end = after.find('>')?;
    let header = &after[..end];
    extract_attr(header, attr)
}

fn extract_attr(header: &str, attr: &str) -> Option<String> {
    // Look for attr= or " attr=" — guard against e.g. `viewBox` matching `data-viewBox`
    let needle = format!(" {attr}=");
    let pos = header.find(&needle)?;
    let after = &header[pos + needle.len()..];
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let body = &after[1..];
    let close = body.find(quote)?;
    Some(body[..close].to_string())
}

fn count_open_tag(svg: &str, tag: &str) -> usize {
    let mut count = 0usize;
    let needle = format!("<{tag}");
    let mut cursor = svg;
    while let Some(pos) = cursor.find(&needle) {
        let after = &cursor[pos + needle.len()..];
        // Must be followed by `>`, ` `, `\t`, `\r`, `\n`, `/`, or end — not by another letter.
        match after.chars().next() {
            Some(c) if c.is_ascii_alphanumeric() || c == '-' || c == '_' => {}
            _ => count += 1,
        }
        cursor = after;
    }
    count
}

fn svg_has_external_href(svg: &str) -> bool {
    for needle in ["xlink:href=", "href="] {
        let mut cursor = svg;
        while let Some(pos) = cursor.find(needle) {
            let after = &cursor[pos + needle.len()..];
            let value = after.trim_start_matches(['"', '\'']);
            if value.starts_with("http://")
                || value.starts_with("https://")
                || value.starts_with("//")
            {
                return true;
            }
            cursor = after;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Structured stats (JSON / YAML / TOML / XML)
// ---------------------------------------------------------------------------

fn structured_format_name(fmt: StructuredFormat) -> &'static str {
    match fmt {
        StructuredFormat::Json => "JSON",
        StructuredFormat::Yaml => "YAML",
        StructuredFormat::Toml => "TOML",
        StructuredFormat::Xml => "XML",
    }
}

fn structured_extras(fmt: StructuredFormat, bytes: &[u8]) -> FileExtras {
    let format_name = structured_format_name(fmt);
    let stats = match std::str::from_utf8(bytes) {
        Ok(s) => match fmt {
            StructuredFormat::Json => json_stats(s),
            StructuredFormat::Yaml => yaml_stats(s),
            StructuredFormat::Toml => toml_stats(s),
            StructuredFormat::Xml => xml_stats(s),
        },
        Err(_) => None,
    };
    FileExtras::Structured { format_name, stats }
}

fn json_stats(s: &str) -> Option<StructuredStats> {
    let value: serde_json::Value = serde_json::from_str(s).ok()?;
    let (kind, count) = match &value {
        serde_json::Value::Object(o) => (TopLevelKind::Object, o.len()),
        serde_json::Value::Array(a) => (TopLevelKind::Array, a.len()),
        _ => (TopLevelKind::Scalar, 0),
    };
    let mut max_depth = 0;
    let mut total_nodes = 0;
    walk_json(&value, 1, &mut max_depth, &mut total_nodes);
    Some(StructuredStats {
        top_level_kind: kind,
        top_level_count: count,
        max_depth,
        total_nodes,
        xml_root: None,
        xml_namespaces: Vec::new(),
    })
}

fn walk_json(v: &serde_json::Value, depth: usize, max_depth: &mut usize, total: &mut usize) {
    *total += 1;
    if depth > *max_depth {
        *max_depth = depth;
    }
    match v {
        serde_json::Value::Object(o) => {
            for (_, val) in o {
                walk_json(val, depth + 1, max_depth, total);
            }
        }
        serde_json::Value::Array(a) => {
            for val in a {
                walk_json(val, depth + 1, max_depth, total);
            }
        }
        _ => {}
    }
}

fn yaml_stats(s: &str) -> Option<StructuredStats> {
    use serde::de::Deserialize;
    use serde_yaml::Value;

    // Multi-document support: count `---` separated docs.
    let docs: Vec<Value> = serde_yaml::Deserializer::from_str(s)
        .map(Value::deserialize)
        .filter_map(|r| r.ok())
        .collect();
    if docs.is_empty() {
        return None;
    }
    let primary = if docs.len() == 1 {
        &docs[0]
    } else {
        // Use first doc for stats but mark multi-doc.
        &docs[0]
    };
    let (kind, count) = match primary {
        Value::Mapping(m) => (TopLevelKind::Object, m.len()),
        Value::Sequence(s) => (TopLevelKind::Array, s.len()),
        Value::Null => (TopLevelKind::Scalar, 0),
        _ => (TopLevelKind::Scalar, 0),
    };
    let kind = if docs.len() > 1 {
        TopLevelKind::MultiDoc(docs.len())
    } else {
        kind
    };
    let mut max_depth = 0;
    let mut total_nodes = 0;
    for doc in &docs {
        walk_yaml(doc, 1, &mut max_depth, &mut total_nodes);
    }
    Some(StructuredStats {
        top_level_kind: kind,
        top_level_count: count,
        max_depth,
        total_nodes,
        xml_root: None,
        xml_namespaces: Vec::new(),
    })
}

fn walk_yaml(v: &serde_yaml::Value, depth: usize, max_depth: &mut usize, total: &mut usize) {
    use serde_yaml::Value;
    *total += 1;
    if depth > *max_depth {
        *max_depth = depth;
    }
    match v {
        Value::Mapping(m) => {
            for (_, val) in m {
                walk_yaml(val, depth + 1, max_depth, total);
            }
        }
        Value::Sequence(seq) => {
            for val in seq {
                walk_yaml(val, depth + 1, max_depth, total);
            }
        }
        _ => {}
    }
}

fn toml_stats(s: &str) -> Option<StructuredStats> {
    let value: toml::Value = toml::from_str(s).ok()?;
    let (kind, count) = match &value {
        toml::Value::Table(t) => (TopLevelKind::Table, t.len()),
        _ => (TopLevelKind::Scalar, 0),
    };
    let mut max_depth = 0;
    let mut total_nodes = 0;
    walk_toml(&value, 1, &mut max_depth, &mut total_nodes);
    Some(StructuredStats {
        top_level_kind: kind,
        top_level_count: count,
        max_depth,
        total_nodes,
        xml_root: None,
        xml_namespaces: Vec::new(),
    })
}

fn walk_toml(v: &toml::Value, depth: usize, max_depth: &mut usize, total: &mut usize) {
    *total += 1;
    if depth > *max_depth {
        *max_depth = depth;
    }
    match v {
        toml::Value::Table(t) => {
            for (_, val) in t {
                walk_toml(val, depth + 1, max_depth, total);
            }
        }
        toml::Value::Array(a) => {
            for val in a {
                walk_toml(val, depth + 1, max_depth, total);
            }
        }
        _ => {}
    }
}

fn xml_stats(s: &str) -> Option<StructuredStats> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(s);
    let mut depth: usize = 0;
    let mut max_depth: usize = 0;
    let mut total_nodes: usize = 0;
    let mut top_level_count: usize = 0;
    let mut xml_root: Option<String> = None;
    let mut xml_namespaces: Vec<String> = Vec::new();

    let mut error_count = 0usize;
    loop {
        let event = match reader.read_event() {
            Err(_) => {
                error_count += 1;
                if error_count > 64 {
                    break;
                }
                continue;
            }
            Ok(ev) => ev,
        };
        let (start_like, empty, name_attrs): (bool, bool, Option<_>) = match &event {
            Event::Eof => break,
            Event::Start(e) => (true, false, Some(e.clone())),
            Event::Empty(e) => (true, true, Some(e.clone())),
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ => continue,
        };
        if !start_like {
            continue;
        }
        total_nodes += 1;
        depth += 1;
        if depth > max_depth {
            max_depth = depth;
        }
        if depth == 1
            && let Some(e) = &name_attrs
        {
            let name_bytes = e.name().as_ref().to_vec();
            let name = String::from_utf8_lossy(&name_bytes).into_owned();
            if xml_root.is_none() {
                xml_root = Some(name);
            }
            for attr in e.attributes().with_checks(false).flatten() {
                let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                if key == "xmlns" || key.starts_with("xmlns:") {
                    let val = attr
                        .unescape_value()
                        .map(|v| v.into_owned())
                        .unwrap_or_default();
                    let entry = if key == "xmlns" {
                        val
                    } else {
                        format!("{}={}", &key[6..], val)
                    };
                    if !xml_namespaces.contains(&entry) {
                        xml_namespaces.push(entry);
                    }
                }
            }
        }
        if depth == 2 {
            top_level_count += 1;
        }
        if empty {
            depth = depth.saturating_sub(1);
        }
    }

    Some(StructuredStats {
        top_level_kind: TopLevelKind::Document,
        top_level_count,
        max_depth,
        total_nodes,
        xml_root,
        xml_namespaces,
    })
}

// ---------------------------------------------------------------------------
// Binary
// ---------------------------------------------------------------------------

fn binary_extras(magic_mime: Option<&str>) -> FileExtras {
    FileExtras::Binary {
        format: magic_mime.map(format_label_for_mime),
    }
}

fn format_label_for_mime(mime: &str) -> String {
    // Map a few common mimes to short, friendly format names; otherwise echo.
    match mime {
        "application/zip" => "ZIP archive".to_string(),
        "application/gzip" => "gzip".to_string(),
        "application/x-tar" => "tar archive".to_string(),
        "application/x-bzip2" => "bzip2".to_string(),
        "application/x-7z-compressed" => "7z archive".to_string(),
        "application/x-xz" => "xz".to_string(),
        "application/x-rar-compressed" => "RAR archive".to_string(),
        "application/x-executable" => "executable".to_string(),
        "application/x-mach-binary" => "Mach-O binary".to_string(),
        "application/x-msdownload" => "PE executable".to_string(),
        "application/vnd.sqlite3" | "application/x-sqlite3" => "SQLite database".to_string(),
        "application/pdf" => "PDF document".to_string(),
        m if m.starts_with("video/") => format!("video ({m})"),
        m if m.starts_with("audio/") => format!("audio ({m})"),
        m => m.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Text stats — single streaming pass collecting all metrics
// ---------------------------------------------------------------------------

fn gather_text_stats_streaming(source: &InputSource) -> Option<TextStats> {
    let bs = source.open_byte_source().ok()?;
    let total = bs.len();
    if total == 0 {
        return Some(TextStats {
            line_count: 0,
            word_count: 0,
            char_count: 0,
            blank_lines: 0,
            longest_line_chars: 0,
            line_endings: LineEndings::None,
            indent_style: None,
            encoding: Encoding::Utf8,
            shebang: None,
        });
    }

    // Detect BOM up-front; advance past it for the rest of the scan.
    let head = bs.read_range(0, 4).ok()?;
    let (encoding, offset) = detect_bom(&head);
    if let Some(stats) = decode_utf16_stats(bs.as_ref(), encoding, offset, total) {
        return Some(stats);
    }
    let mut offset = offset;

    let mut buf: Vec<u8> = Vec::with_capacity(TEXT_SCAN_CHUNK);
    let mut chars = 0usize;
    let mut words = 0usize;
    let mut lf_count = 0usize;
    let mut crlf_count = 0usize;
    let mut cr_count = 0usize;
    let mut blank_lines = 0usize;
    let mut longest = 0usize;
    let mut current_line_chars = 0usize;
    let mut current_line_blank = true;
    // Indent tracking
    let mut tab_indents = 0usize;
    let mut space_indents = 0usize;
    let mut at_line_start = true;
    let mut counting_indent = false;
    let mut current_indent_spaces: u8 = 0;
    let mut space_widths: [usize; 9] = [0; 9];
    // Word tracking across chunk boundaries
    let mut in_word = false;
    let mut last_char: Option<char> = None;
    // Shebang line (only if file starts with `#!`)
    let mut shebang_bytes: Option<Vec<u8>> = None;
    let mut at_file_start = offset == 0;

    while offset < total {
        let want = ((total - offset) as usize).min(TEXT_SCAN_CHUNK);
        let read = bs.read_range(offset, want).ok()?;
        if read.is_empty() {
            break;
        }
        offset += read.len() as u64;
        buf.extend_from_slice(&read);

        let valid_up_to = match std::str::from_utf8(&buf) {
            Ok(_) => buf.len(),
            Err(e) => {
                if e.error_len().is_some() {
                    return None;
                }
                e.valid_up_to()
            }
        };

        let s = std::str::from_utf8(&buf[..valid_up_to])
            .expect("valid_up_to slice is valid UTF-8");

        // Shebang capture (first line) — buffer raw bytes until we see `\n`.
        if at_file_start && s.starts_with("#!") {
            let line_end = s.find('\n').unwrap_or(s.len());
            let mut line: Vec<u8> = s.as_bytes()[..line_end].to_vec();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            shebang_bytes = Some(line);
        }
        if at_file_start {
            at_file_start = false;
        }

        let mut prev_was_cr = matches!(last_char, Some('\r'));
        for ch in s.chars() {
            chars += 1;

            // Line-ending detection
            if ch == '\n' {
                if prev_was_cr {
                    crlf_count += 1;
                    // Already accounted for the line on \r — ignore here.
                } else {
                    lf_count += 1;
                    finalize_line(
                        current_line_chars,
                        current_line_blank,
                        &mut longest,
                        &mut blank_lines,
                    );
                }
                current_line_chars = 0;
                current_line_blank = true;
                at_line_start = true;
                counting_indent = false;
                current_indent_spaces = 0;
            } else if ch == '\r' {
                // Defer: might be \r\n
                finalize_line(
                    current_line_chars,
                    current_line_blank,
                    &mut longest,
                    &mut blank_lines,
                );
                current_line_chars = 0;
                current_line_blank = true;
                at_line_start = true;
                counting_indent = false;
                current_indent_spaces = 0;
            } else {
                if prev_was_cr {
                    cr_count += 1;
                }
                current_line_chars += 1;
                if !ch.is_whitespace() {
                    current_line_blank = false;
                }

                // Indent classification on first non-newline of each line
                if at_line_start {
                    counting_indent = true;
                    at_line_start = false;
                }
                if counting_indent {
                    if ch == '\t' {
                        tab_indents += 1;
                        counting_indent = false;
                    } else if ch == ' ' {
                        current_indent_spaces = current_indent_spaces.saturating_add(1);
                    } else {
                        if current_indent_spaces > 0 {
                            space_indents += 1;
                            // Bucket the run length by smallest prime-ish: most code
                            // uses 2/4/8 — group widths up to 8.
                            let idx = current_indent_spaces.min(8) as usize;
                            space_widths[idx] += 1;
                        }
                        counting_indent = false;
                    }
                }
            }

            // Word counting (whitespace boundaries)
            let is_ws = ch.is_whitespace();
            if !is_ws && !in_word {
                words += 1;
                in_word = true;
            } else if is_ws {
                in_word = false;
            }

            prev_was_cr = ch == '\r';
            last_char = Some(ch);
        }

        buf.drain(..valid_up_to);
    }

    if !buf.is_empty() {
        return None;
    }

    // Trailing CR (no following LF, no further chars) — count it.
    if matches!(last_char, Some('\r')) {
        cr_count += 1;
        finalize_line(
            current_line_chars,
            current_line_blank,
            &mut longest,
            &mut blank_lines,
        );
        current_line_chars = 0;
        current_line_blank = true;
    }

    // Final unterminated line counts as a line in `str::lines()` semantics.
    let last_was_terminator = matches!(last_char, Some('\n') | Some('\r') | None);
    if !last_was_terminator {
        finalize_line(
            current_line_chars,
            current_line_blank,
            &mut longest,
            &mut blank_lines,
        );
    }

    let line_count = match last_char {
        None => 0,
        Some('\n') | Some('\r') => lf_count + crlf_count + cr_count,
        Some(_) => lf_count + crlf_count + cr_count + 1,
    };

    let line_endings = classify_line_endings(lf_count, crlf_count, cr_count);
    let indent_style = classify_indent(tab_indents, space_indents, &space_widths);
    let shebang = shebang_bytes
        .and_then(|b| String::from_utf8(b).ok())
        .map(|s| s.trim_start_matches("#!").trim().to_string());

    Some(TextStats {
        line_count,
        word_count: words,
        char_count: chars,
        blank_lines,
        longest_line_chars: longest,
        line_endings,
        indent_style,
        encoding,
        shebang,
    })
}

fn finalize_line(line_chars: usize, blank: bool, longest: &mut usize, blanks: &mut usize) {
    if line_chars > *longest {
        *longest = line_chars;
    }
    if blank {
        *blanks += 1;
    }
}

fn classify_line_endings(lf: usize, crlf: usize, cr: usize) -> LineEndings {
    let kinds = [(lf, LineEndings::Lf), (crlf, LineEndings::Crlf), (cr, LineEndings::Cr)];
    let nonzero: Vec<_> = kinds.iter().filter(|(n, _)| *n > 0).collect();
    match nonzero.len() {
        0 => LineEndings::None,
        1 => nonzero[0].1,
        _ => LineEndings::Mixed,
    }
}

fn classify_indent(tabs: usize, spaces: usize, widths: &[usize; 9]) -> Option<IndentStyle> {
    if tabs == 0 && spaces == 0 {
        return None;
    }
    if tabs > 0 && spaces > 0 {
        return Some(IndentStyle::Mixed);
    }
    if tabs > 0 {
        return Some(IndentStyle::Tabs);
    }
    // Pick most common space width (1..=8). Prefer common code values when tied.
    let (mut best_w, mut best_n) = (4u8, 0usize);
    for (w, &n) in widths.iter().enumerate().skip(1) {
        if n > best_n {
            best_n = n;
            best_w = w as u8;
        }
    }
    Some(IndentStyle::Spaces(best_w))
}

fn detect_bom(head: &[u8]) -> (Encoding, u64) {
    if head.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (Encoding::Utf8Bom, 3);
    }
    if head.starts_with(&[0xFF, 0xFE]) {
        return (Encoding::Utf16Le, 2);
    }
    if head.starts_with(&[0xFE, 0xFF]) {
        return (Encoding::Utf16Be, 2);
    }
    (Encoding::Utf8, 0)
}

fn decode_utf16_stats(
    bs: &dyn crate::input::ByteSource,
    encoding: Encoding,
    offset: u64,
    total: u64,
) -> Option<TextStats> {
    if !matches!(encoding, Encoding::Utf16Le | Encoding::Utf16Be) {
        return None;
    }
    let body = bs.read_range(offset, (total - offset) as usize).ok()?;
    let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|c| match encoding {
            Encoding::Utf16Le => u16::from_le_bytes([c[0], c[1]]),
            Encoding::Utf16Be => u16::from_be_bytes([c[0], c[1]]),
            _ => 0,
        })
        .collect();
    let s = String::from_utf16_lossy(&units);
    let stats = analyse_text(&s, encoding);
    Some(stats)
}

/// Synchronous text analysis on a fully-loaded string. Used for UTF-16
/// content that we transcoded after reading the whole body — UTF-16 files in
/// the wild are essentially always small config / script files, so in-memory
/// is fine for that codepath.
fn analyse_text(s: &str, encoding: Encoding) -> TextStats {
    let mut lf_count = 0usize;
    let mut crlf_count = 0usize;
    let mut cr_count = 0usize;
    let mut blank_lines = 0usize;
    let mut longest = 0usize;
    let mut current = 0usize;
    let mut current_blank = true;
    let mut chars = 0usize;
    let mut words = 0usize;
    let mut in_word = false;
    let mut tab_indents = 0usize;
    let mut space_indents = 0usize;
    let mut space_widths: [usize; 9] = [0; 9];
    let mut at_line_start = true;
    let mut counting_indent = false;
    let mut current_indent: u8 = 0;

    let mut prev_cr = false;
    for ch in s.chars() {
        chars += 1;
        if ch == '\n' {
            if prev_cr {
                crlf_count += 1;
            } else {
                lf_count += 1;
                finalize_line(current, current_blank, &mut longest, &mut blank_lines);
            }
            current = 0;
            current_blank = true;
            at_line_start = true;
            counting_indent = false;
            current_indent = 0;
        } else if ch == '\r' {
            finalize_line(current, current_blank, &mut longest, &mut blank_lines);
            current = 0;
            current_blank = true;
            at_line_start = true;
            counting_indent = false;
            current_indent = 0;
        } else {
            if prev_cr {
                cr_count += 1;
            }
            current += 1;
            if !ch.is_whitespace() {
                current_blank = false;
            }
            if at_line_start {
                counting_indent = true;
                at_line_start = false;
            }
            if counting_indent {
                if ch == '\t' {
                    tab_indents += 1;
                    counting_indent = false;
                } else if ch == ' ' {
                    current_indent = current_indent.saturating_add(1);
                } else {
                    if current_indent > 0 {
                        space_indents += 1;
                        let idx = current_indent.min(8) as usize;
                        space_widths[idx] += 1;
                    }
                    counting_indent = false;
                }
            }
        }
        let is_ws = ch.is_whitespace();
        if !is_ws && !in_word {
            words += 1;
            in_word = true;
        } else if is_ws {
            in_word = false;
        }
        prev_cr = ch == '\r';
    }

    if prev_cr {
        cr_count += 1;
        finalize_line(current, current_blank, &mut longest, &mut blank_lines);
        current = 0;
        current_blank = true;
    }
    let last = s.chars().last();
    let last_was_term = matches!(last, Some('\n') | Some('\r') | None);
    if !last_was_term {
        finalize_line(current, current_blank, &mut longest, &mut blank_lines);
    }

    let line_count = match last {
        None => 0,
        Some('\n') | Some('\r') => lf_count + crlf_count + cr_count,
        Some(_) => lf_count + crlf_count + cr_count + 1,
    };

    TextStats {
        line_count,
        word_count: words,
        char_count: chars,
        blank_lines,
        longest_line_chars: longest,
        line_endings: classify_line_endings(lf_count, crlf_count, cr_count),
        indent_style: classify_indent(tab_indents, space_indents, &space_widths),
        encoding,
        shebang: s.strip_prefix("#!").and_then(|rest| {
            rest.lines().next().map(|l| l.trim().to_string())
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdin_source(text: &str) -> InputSource {
        InputSource::Stdin {
            data: Arc::from(text.as_bytes().to_vec().into_boxed_slice()),
        }
    }

    fn text_stats(s: &str) -> TextStats {
        gather_text_stats_streaming(&stdin_source(s)).expect("expected text stats")
    }

    fn extras_text(s: &str) -> (usize, usize, usize) {
        let st = text_stats(s);
        (st.line_count, st.word_count, st.char_count)
    }

    fn assert_matches_std(s: &str) {
        let (lines, words, chars) = extras_text(s);
        assert_eq!(lines, s.lines().count(), "lines for {s:?}");
        assert_eq!(words, s.split_whitespace().count(), "words for {s:?}");
        assert_eq!(chars, s.chars().count(), "chars for {s:?}");
    }

    #[test]
    fn empty_input() {
        let stats = text_stats("");
        assert_eq!(stats.line_count, 0);
        assert_eq!(stats.word_count, 0);
        assert_eq!(stats.char_count, 0);
        assert!(matches!(stats.line_endings, LineEndings::None));
    }

    #[test]
    fn unterminated_final_line() {
        assert_matches_std("alpha\nbeta\ngamma");
    }

    #[test]
    fn terminated_final_line() {
        assert_matches_std("alpha\nbeta\ngamma\n");
    }

    #[test]
    fn blank_lines() {
        assert_matches_std("\n\n\n");
        assert_matches_std("a\n\nb\n");
    }

    #[test]
    fn crlf_lines() {
        assert_matches_std("a\r\nb\r\nc\r\n");
        let stats = text_stats("a\r\nb\r\nc\r\n");
        assert!(matches!(stats.line_endings, LineEndings::Crlf));
    }

    #[test]
    fn lf_classification() {
        let stats = text_stats("a\nb\n");
        assert!(matches!(stats.line_endings, LineEndings::Lf));
    }

    #[test]
    fn mixed_line_endings() {
        let stats = text_stats("a\nb\r\nc\n");
        assert!(matches!(stats.line_endings, LineEndings::Mixed));
    }

    #[test]
    fn longest_line_tracked() {
        let stats = text_stats("ab\nabcdef\nabc\n");
        assert_eq!(stats.longest_line_chars, 6);
    }

    #[test]
    fn blank_line_count() {
        let stats = text_stats("a\n\n\nb\n");
        assert_eq!(stats.blank_lines, 2);
    }

    #[test]
    fn indent_tabs() {
        let stats = text_stats("a\n\tb\n\tc\n");
        assert!(matches!(stats.indent_style, Some(IndentStyle::Tabs)));
    }

    #[test]
    fn indent_spaces() {
        let stats = text_stats("a\n    b\n    c\n");
        assert!(matches!(stats.indent_style, Some(IndentStyle::Spaces(4))));
    }

    #[test]
    fn unicode_words_and_chars() {
        assert_matches_std("héllo wörld\nαβγ δεζ\n你好 世界\n");
    }

    #[test]
    fn shebang_detected() {
        let stats = text_stats("#!/usr/bin/env python3\nprint('hi')\n");
        assert_eq!(stats.shebang.as_deref(), Some("/usr/bin/env python3"));
    }

    #[test]
    fn utf8_bom_detected() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"hello\n");
        let src = InputSource::Stdin {
            data: Arc::from(bytes.into_boxed_slice()),
        };
        let stats = gather_text_stats_streaming(&src).unwrap();
        assert!(matches!(stats.encoding, Encoding::Utf8Bom));
    }

    #[test]
    fn invalid_utf8_returns_none() {
        let bad = vec![0xff, 0xfe, 0x05]; // BOM-ish but truncated → not valid UTF-16 either, but UTF-16 LE will still 'work'
        let src = InputSource::Stdin {
            data: Arc::from(bad.into_boxed_slice()),
        };
        // First two bytes are FF FE → UTF-16 LE; we accept that.
        // Use a clearly invalid byte instead.
        let bad2 = vec![0x80, 0x80, 0x80];
        let src2 = InputSource::Stdin {
            data: Arc::from(bad2.into_boxed_slice()),
        };
        // 0x80 alone is invalid UTF-8 (continuation byte without lead).
        assert!(gather_text_stats_streaming(&src2).is_none());
        let _ = src;
    }

    #[test]
    fn truncated_utf8_returns_none() {
        let truncated = vec![0xe4, 0xbd];
        let src = InputSource::Stdin {
            data: Arc::from(truncated.into_boxed_slice()),
        };
        assert!(gather_text_stats_streaming(&src).is_none());
    }
}
