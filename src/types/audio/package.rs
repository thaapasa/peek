//! Symphonia probe shared between info-gather, listing, and extract.
//! `probe` does the read+parse work once; downstream callers transform
//! the resulting [`Probed`] into [`AudioStats`], [`FlatEntry`] lists,
//! or raw byte payloads for extract.
//!
//! Re-probing per call is intentional: symphonia probe only walks
//! container headers + the metadata block, so cost is bounded even for
//! files that embed multi-MB cover art. Caching across modes would
//! require an [`std::sync::Arc<Probed>`] threaded through
//! `compose_modes` — defer until profiling shows a hot path.

use std::io::Cursor;

use anyhow::Result;
use symphonia::core::codecs::CodecType;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{
    MetadataOptions, MetadataRevision, StandardTagKey, StandardVisualKey, Tag, Value,
    Visual as SymVisual,
};
use symphonia::core::probe::Hint;
use symphonia::default::get_probe;

use crate::input::InputSource;
use crate::input::detect::AudioFormat;
use crate::types::audio::info::{AudioMetadata, AudioStats};
use crate::types::listing::FlatEntry;

/// Full probe result. Everything info-gather, listing, and extract need
/// flows from this single struct.
pub struct Probed {
    pub format: AudioFormat,
    pub codec: Option<&'static str>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub channel_layout: Option<String>,
    pub bits_per_sample: Option<u32>,
    pub duration_secs: Option<f64>,
    pub bitrate: Option<u64>,
    pub metadata: AudioMetadata,
    pub visuals: Vec<EmbedVisual>,
    /// Joined lyrics text from every `USLT` / `SYLT` / `LYRICS=` source
    /// in the file. `None` when none present.
    pub lyrics: Option<String>,
}

/// One picture embedded in the container (ID3v2 `APIC`, FLAC PICTURE
/// block, Vorbis `METADATA_BLOCK_PICTURE`, MP4 `covr` atom, APE binary
/// tag). `usage_root` is the canonical filename root the listing path
/// is built from — front_cover / back_cover / artist / etc.
pub struct EmbedVisual {
    pub media_type: String,
    pub usage_root: &'static str,
    pub data: Vec<u8>,
}

/// Listing entry kind we synthesised from a [`Probed`]. Carries enough
/// to materialise an [`InputSource::Memory`] in `read_embed` without
/// re-walking the visual list by index.
enum EmbedKind {
    Picture(usize),
    Lyrics,
}

pub fn probe(source: &InputSource, format: AudioFormat) -> Result<Probed> {
    let bytes = source.read_bytes()?;
    let file_size = bytes.len() as u64;
    let cursor = Cursor::new(bytes);
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = extension_hint(format) {
        hint.with_extension(ext);
    }
    if let Some(mime) = mime_hint(format) {
        hint.mime_type(mime);
    }

    let mut probed = get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;

    let mut out = Probed {
        format,
        codec: None,
        sample_rate: None,
        channels: None,
        channel_layout: None,
        bits_per_sample: None,
        duration_secs: None,
        bitrate: None,
        metadata: AudioMetadata::default(),
        visuals: Vec::new(),
        lyrics: None,
    };

    if let Some(track) = probed.format.default_track() {
        let params = &track.codec_params;
        out.codec = codec_label(params.codec);
        out.sample_rate = params.sample_rate;
        if let Some(channels) = params.channels {
            out.channels = Some(channels.count() as u16);
            out.channel_layout = Some(channel_layout_label(channels.count()));
        }
        out.bits_per_sample = params.bits_per_sample;
        if let (Some(n_frames), Some(tb)) = (params.n_frames, params.time_base) {
            let secs = n_frames as f64 * tb.numer as f64 / tb.denom as f64;
            out.duration_secs = Some(secs);
        }
        out.bitrate = derive_bitrate(file_size, out.duration_secs);
    }

    // Walk every metadata revision the probe surfaced. Vorbis containers
    // (Ogg / FLAC) carry tags on `probed.format.metadata()`; ID3v2-only
    // files (MP3 / AIFF) land them on `probed.metadata` instead. Some
    // files split tags + visuals across both — read both.
    let fmt_md = probed.format.metadata();
    if let Some(rev) = fmt_md.current() {
        ingest_revision(rev, &mut out);
    }
    if let Some(md) = probed.metadata.get().as_ref()
        && let Some(rev) = md.current()
    {
        ingest_revision(rev, &mut out);
    }

    Ok(out)
}

/// Project a [`Probed`] onto the public [`AudioStats`] shape used by
/// the info view. Drops the visual / lyric bodies — those are only
/// needed by the listing + extract paths.
pub fn to_stats(probed: &Probed) -> AudioStats {
    AudioStats {
        format: probed.format,
        codec: probed.codec.map(str::to_string),
        duration_secs: probed.duration_secs,
        sample_rate: probed.sample_rate,
        channels: probed.channels,
        channel_layout: probed.channel_layout.clone(),
        bits_per_sample: probed.bits_per_sample,
        bitrate: probed.bitrate,
        metadata: probed.metadata.clone(),
        has_lyrics: probed.lyrics.is_some(),
        has_album_art: !probed.visuals.is_empty(),
        error: None,
    }
}

/// Build the listing tree for a probed file. Pictures land under
/// `pictures/<usage>.<ext>`, with `_N` suffix when multiple share a
/// usage root. Lyrics (any source) collapse to a single
/// `lyrics/lyrics.txt`. Empty result → caller should skip pushing a
/// ListingMode entirely.
pub fn build_listing(probed: &Probed) -> Vec<FlatEntry> {
    build_listing_keyed(probed)
        .into_iter()
        .map(|(path, _kind, size)| FlatEntry {
            path,
            size,
            mtime: None,
            mode: None,
            is_dir: false,
        })
        .collect()
}

/// List + extract share the same path-derivation logic. Returns
/// `(path, kind, size)` tuples; `build_listing` drops the kind for the
/// public surface, `read_embed` keeps it to look up the right payload
/// without a second walk.
fn build_listing_keyed(probed: &Probed) -> Vec<(String, EmbedKind, u64)> {
    let mut out: Vec<(String, EmbedKind, u64)> = Vec::new();
    let mut seen: std::collections::HashMap<&'static str, usize> = std::collections::HashMap::new();
    for (idx, visual) in probed.visuals.iter().enumerate() {
        let root = visual.usage_root;
        let count = seen.entry(root).or_insert(0);
        *count += 1;
        let stem = if *count > 1 {
            format!("{root}_{}", *count)
        } else {
            root.to_string()
        };
        let ext = extension_for_mime(&visual.media_type);
        let path = format!("pictures/{stem}.{ext}");
        out.push((path, EmbedKind::Picture(idx), visual.data.len() as u64));
    }
    if let Some(lyrics) = &probed.lyrics {
        out.push((
            "lyrics/lyrics.txt".to_string(),
            EmbedKind::Lyrics,
            lyrics.len() as u64,
        ));
    }
    out
}

/// Resolve a listing key (e.g. `pictures/front_cover.jpg`) to raw
/// payload bytes plus a suggested extract filename. Returns `None` for
/// unknown keys — caller maps that to `ExtractError::NotFound`.
pub fn read_embed(probed: &Probed, key: &str) -> Option<(Vec<u8>, String)> {
    let entries = build_listing_keyed(probed);
    let (path, kind, _) = entries.into_iter().find(|(p, _, _)| p == key)?;
    let suggested = path.rsplit('/').next().unwrap_or(&path).to_string();
    let bytes = match kind {
        EmbedKind::Picture(idx) => probed.visuals.get(idx)?.data.clone(),
        EmbedKind::Lyrics => probed.lyrics.as_ref()?.as_bytes().to_vec(),
    };
    Some((bytes, suggested))
}

fn ingest_revision(rev: &MetadataRevision, out: &mut Probed) {
    for tag in rev.tags() {
        ingest_tag(tag, out);
    }
    for visual in rev.visuals() {
        out.visuals.push(convert_visual(visual));
    }
}

fn convert_visual(v: &SymVisual) -> EmbedVisual {
    EmbedVisual {
        media_type: v.media_type.clone(),
        usage_root: v.usage.map(visual_usage_root).unwrap_or("picture"),
        data: v.data.to_vec(),
    }
}

/// Canonical filename root per documented `StandardVisualKey` value.
/// Used by `build_listing` to pick `pictures/<root>.<ext>`. Unknown /
/// `Other` collapse to a generic `picture` root; duplicates get
/// `_N`-suffixed downstream.
fn visual_usage_root(key: StandardVisualKey) -> &'static str {
    match key {
        StandardVisualKey::FrontCover => "front_cover",
        StandardVisualKey::BackCover => "back_cover",
        StandardVisualKey::Leaflet => "leaflet",
        StandardVisualKey::Media => "media",
        StandardVisualKey::LeadArtistPerformerSoloist => "lead_artist",
        StandardVisualKey::ArtistPerformer => "artist",
        StandardVisualKey::Conductor => "conductor",
        StandardVisualKey::BandOrchestra => "band",
        StandardVisualKey::Composer => "composer",
        StandardVisualKey::Lyricist => "lyricist",
        StandardVisualKey::RecordingLocation => "recording_location",
        StandardVisualKey::RecordingSession => "recording_session",
        StandardVisualKey::Performance => "performance",
        StandardVisualKey::ScreenCapture => "screen_capture",
        StandardVisualKey::Illustration => "illustration",
        StandardVisualKey::BandArtistLogo => "band_logo",
        StandardVisualKey::PublisherStudioLogo => "publisher_logo",
        StandardVisualKey::FileIcon => "file_icon",
        StandardVisualKey::OtherIcon => "icon",
    }
}

/// Pick a filename extension for a picture from its declared media
/// type. Stays narrow on purpose — the listing extension drives the
/// re-detect path on extract, and an over-eager mapping would mis-route
/// (e.g. mapping a malformed `image/x-foo` to `bin` is safer than
/// guessing `jpg`).
fn extension_for_mime(mime: &str) -> &'static str {
    match mime.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" | "image/x-bmp" => "bmp",
        "image/tiff" => "tif",
        _ => "bin",
    }
}

fn ingest_tag(tag: &Tag, out: &mut Probed) {
    let v = tag_value_string(&tag.value);
    let m = &mut out.metadata;

    if let Some(key) = tag.std_key {
        if matches!(key, StandardTagKey::Lyrics) {
            if !v.is_empty() {
                append_lyrics(&mut out.lyrics, &v);
            }
            return;
        }
        if v.is_empty() {
            return;
        }
        match key {
            StandardTagKey::TrackTitle => set_once(&mut m.title, v),
            StandardTagKey::Artist => set_once(&mut m.artist, v),
            StandardTagKey::Album => set_once(&mut m.album, v),
            StandardTagKey::AlbumArtist => set_once(&mut m.album_artist, v),
            StandardTagKey::TrackNumber => set_once(&mut m.track_number, v),
            StandardTagKey::DiscNumber => set_once(&mut m.disc_number, v),
            StandardTagKey::Date | StandardTagKey::ReleaseDate => set_once(&mut m.date, v),
            StandardTagKey::Genre => set_once(&mut m.genre, v),
            StandardTagKey::Composer => set_once(&mut m.composer, v),
            StandardTagKey::Comment => set_once(&mut m.comment, v),
            _ => {}
        }
        return;
    }

    // Non-standard tag. Catch `LYRICS=` Vorbis comments (which
    // symphonia doesn't always map to StandardTagKey::Lyrics) on the
    // raw key name so they still feed the lyrics body.
    if !v.is_empty() && tag.key.eq_ignore_ascii_case("lyrics") {
        append_lyrics(&mut out.lyrics, &v);
    }
}

fn append_lyrics(slot: &mut Option<String>, text: &str) {
    match slot {
        Some(existing) => {
            existing.push_str("\n\n");
            existing.push_str(text);
        }
        None => *slot = Some(text.to_string()),
    }
}

fn set_once(slot: &mut Option<String>, value: String) {
    if slot.is_none() {
        *slot = Some(value);
    }
}

fn tag_value_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.trim().to_string(),
        Value::Binary(_) => String::new(),
        Value::Boolean(b) => b.to_string(),
        Value::Flag => String::new(),
        Value::Float(f) => f.to_string(),
        Value::SignedInt(i) => i.to_string(),
        Value::UnsignedInt(u) => u.to_string(),
    }
}

fn channel_layout_label(channel_count: usize) -> String {
    match channel_count {
        1 => "mono".to_string(),
        2 => "stereo".to_string(),
        3 => "2.1".to_string(),
        4 => "quad".to_string(),
        6 => "5.1".to_string(),
        8 => "7.1".to_string(),
        n => format!("{n} channels"),
    }
}

/// Bit-rate estimate from file size + duration (bits per second). Falls
/// back to `None` when duration is missing — without a denominator the
/// number is meaningless. We don't surface symphonia's per-frame
/// `bit_rate` because it's only populated by a couple of codecs and
/// the size/duration derivation matches user expectations ("what would
/// a player display in the info row").
fn derive_bitrate(file_size: u64, duration_secs: Option<f64>) -> Option<u64> {
    let secs = duration_secs?;
    if secs <= 0.0 || file_size == 0 {
        return None;
    }
    Some(((file_size as f64 * 8.0) / secs).round() as u64)
}

fn extension_hint(format: AudioFormat) -> Option<&'static str> {
    Some(match format {
        AudioFormat::Mp3 => "mp3",
        AudioFormat::Flac => "flac",
        AudioFormat::Ogg => "ogg",
        AudioFormat::Opus => "opus",
        AudioFormat::Wav => "wav",
        AudioFormat::M4a => "m4a",
        AudioFormat::Aac => "aac",
        AudioFormat::Aiff => "aiff",
        AudioFormat::Caf => "caf",
        AudioFormat::Mka => "mka",
        AudioFormat::Wma => return None,
    })
}

fn mime_hint(format: AudioFormat) -> Option<&'static str> {
    Some(match format {
        AudioFormat::Mp3 => "audio/mpeg",
        AudioFormat::Flac => "audio/flac",
        AudioFormat::Ogg => "audio/ogg",
        AudioFormat::Opus => "audio/opus",
        AudioFormat::Wav => "audio/wav",
        AudioFormat::M4a => "audio/mp4",
        AudioFormat::Aac => "audio/aac",
        AudioFormat::Aiff => "audio/aiff",
        AudioFormat::Caf => "audio/x-caf",
        AudioFormat::Mka => "audio/x-matroska",
        AudioFormat::Wma => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_probed(visuals: Vec<EmbedVisual>, lyrics: Option<String>) -> Probed {
        Probed {
            format: AudioFormat::Mp3,
            codec: None,
            sample_rate: None,
            channels: None,
            channel_layout: None,
            bits_per_sample: None,
            duration_secs: None,
            bitrate: None,
            metadata: AudioMetadata::default(),
            visuals,
            lyrics,
        }
    }

    fn pic(usage: &'static str, mime: &str, body: &[u8]) -> EmbedVisual {
        EmbedVisual {
            media_type: mime.to_string(),
            usage_root: usage,
            data: body.to_vec(),
        }
    }

    #[test]
    fn empty_probed_has_no_listing() {
        let probed = synthetic_probed(vec![], None);
        assert!(build_listing(&probed).is_empty());
    }

    #[test]
    fn front_back_artist_route_under_pictures() {
        let probed = synthetic_probed(
            vec![
                pic("front_cover", "image/jpeg", b"frontbytes"),
                pic("back_cover", "image/png", b"backbytes"),
                pic("artist", "image/jpeg", b"artistbytes"),
            ],
            None,
        );
        let entries = build_listing(&probed);
        let names: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(names.contains(&"pictures/front_cover.jpg"), "{names:?}");
        assert!(names.contains(&"pictures/back_cover.png"), "{names:?}");
        assert!(names.contains(&"pictures/artist.jpg"), "{names:?}");
    }

    #[test]
    fn duplicate_usage_gets_index_suffix() {
        let probed = synthetic_probed(
            vec![
                pic("front_cover", "image/jpeg", b"a"),
                pic("front_cover", "image/jpeg", b"bb"),
                pic("front_cover", "image/png", b"ccc"),
            ],
            None,
        );
        let names: Vec<String> = build_listing(&probed)
            .iter()
            .map(|e| e.path.clone())
            .collect();
        assert_eq!(
            names,
            vec![
                "pictures/front_cover.jpg".to_string(),
                "pictures/front_cover_2.jpg".to_string(),
                "pictures/front_cover_3.png".to_string(),
            ]
        );
    }

    #[test]
    fn lyrics_listed_when_present() {
        let probed = synthetic_probed(vec![], Some("la la la".to_string()));
        let names: Vec<String> = build_listing(&probed)
            .iter()
            .map(|e| e.path.clone())
            .collect();
        assert_eq!(names, vec!["lyrics/lyrics.txt".to_string()]);
    }

    #[test]
    fn read_embed_returns_picture_bytes() {
        let probed = synthetic_probed(vec![pic("front_cover", "image/jpeg", b"FRONT")], None);
        let (bytes, name) = read_embed(&probed, "pictures/front_cover.jpg").unwrap();
        assert_eq!(bytes, b"FRONT");
        assert_eq!(name, "front_cover.jpg");
    }

    #[test]
    fn read_embed_returns_lyrics_bytes() {
        let probed = synthetic_probed(vec![], Some("hello world".to_string()));
        let (bytes, name) = read_embed(&probed, "lyrics/lyrics.txt").unwrap();
        assert_eq!(bytes, b"hello world");
        assert_eq!(name, "lyrics.txt");
    }

    #[test]
    fn read_embed_unknown_key_is_none() {
        let probed = synthetic_probed(vec![], None);
        assert!(read_embed(&probed, "pictures/nope.jpg").is_none());
    }

    #[test]
    fn extension_for_mime_known_and_unknown() {
        assert_eq!(extension_for_mime("image/jpeg"), "jpg");
        assert_eq!(extension_for_mime("IMAGE/PNG"), "png");
        assert_eq!(extension_for_mime("image/x-foo"), "bin");
        assert_eq!(extension_for_mime("application/octet-stream"), "bin");
    }
}

fn codec_label(codec: CodecType) -> Option<&'static str> {
    // Symphonia exposes a registry-based descriptor lookup, but the
    // long-form names are clearer if we pin them here. Unknown codec
    // (WMA, anything symphonia doesn't bundle) → None and the
    // renderer skips the row.
    use symphonia::core::codecs as c;
    Some(match codec {
        c::CODEC_TYPE_MP1 => "MPEG-1 Audio Layer 1",
        c::CODEC_TYPE_MP2 => "MPEG-1 Audio Layer 2",
        c::CODEC_TYPE_MP3 => "MP3 (MPEG-1 Audio Layer 3)",
        c::CODEC_TYPE_AAC => "AAC (Advanced Audio Coding)",
        c::CODEC_TYPE_FLAC => "FLAC (Free Lossless Audio Codec)",
        c::CODEC_TYPE_ALAC => "ALAC (Apple Lossless Audio Codec)",
        c::CODEC_TYPE_VORBIS => "Vorbis",
        c::CODEC_TYPE_OPUS => "Opus",
        c::CODEC_TYPE_PCM_S16LE
        | c::CODEC_TYPE_PCM_S16BE
        | c::CODEC_TYPE_PCM_S24LE
        | c::CODEC_TYPE_PCM_S24BE
        | c::CODEC_TYPE_PCM_S32LE
        | c::CODEC_TYPE_PCM_S32BE
        | c::CODEC_TYPE_PCM_F32LE
        | c::CODEC_TYPE_PCM_F32BE
        | c::CODEC_TYPE_PCM_F64LE
        | c::CODEC_TYPE_PCM_F64BE
        | c::CODEC_TYPE_PCM_U8
        | c::CODEC_TYPE_PCM_S8 => "PCM",
        c::CODEC_TYPE_ADPCM_MS | c::CODEC_TYPE_ADPCM_IMA_WAV => "ADPCM",
        _ => return None,
    })
}
