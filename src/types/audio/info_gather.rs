//! Populate [`AudioStats`] via symphonia. Probe-only — no audio
//! decoding. Track 0 supplies the codec params (channels / sample rate
//! / bit depth / duration). All metadata revisions are walked so tags
//! that live in the container header (Vorbis comments) and tags that
//! live in side blocks (ID3v2 inside FLAC, for instance) both surface.

use std::io::Cursor;

use symphonia::core::codecs::CodecType;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey, Tag, Value};
use symphonia::core::probe::Hint;
use symphonia::default::get_probe;

use crate::info::FileExtras;
use crate::input::InputSource;
use crate::input::detect::AudioFormat;
use crate::types::audio::info::{AudioMetadata, AudioStats};

pub fn gather_extras(source: &InputSource, format: AudioFormat) -> FileExtras {
    match probe(source, format) {
        Ok(stats) => FileExtras::Audio(stats),
        Err(e) => {
            let mut stats = AudioStats::empty(format);
            stats.error = Some(format!("{e:#}"));
            FileExtras::Audio(stats)
        }
    }
}

fn probe(source: &InputSource, format: AudioFormat) -> anyhow::Result<AudioStats> {
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

    let mut stats = AudioStats::empty(format);

    if let Some(track) = probed.format.default_track() {
        let params = &track.codec_params;
        stats.codec = codec_label(params.codec).map(str::to_string);
        stats.sample_rate = params.sample_rate;
        if let Some(channels) = params.channels {
            stats.channels = Some(channels.count() as u16);
            stats.channel_layout = Some(channel_layout_label(channels.count()));
        }
        stats.bits_per_sample = params.bits_per_sample;
        if let (Some(n_frames), Some(tb)) = (params.n_frames, params.time_base) {
            let secs = n_frames as f64 * tb.numer as f64 / tb.denom as f64;
            stats.duration_secs = Some(secs);
        }
        stats.bitrate = derive_bitrate(file_size, stats.duration_secs);
    }

    // Walk every metadata revision the probe surfaced. Vorbis containers
    // (Ogg / FLAC) carry tags on `probed.format.metadata()`; ID3v2-only
    // files (MP3 / AIFF) land them on `probed.metadata` instead. Some
    // files split tags across both — read both.
    let fmt_md = probed.format.metadata();
    if let Some(rev) = fmt_md.current() {
        ingest_revision(rev, &mut stats);
    }
    if let Some(md) = probed.metadata.get().as_ref()
        && let Some(rev) = md.current()
    {
        ingest_revision(rev, &mut stats);
    }

    Ok(stats)
}

fn ingest_revision(rev: &MetadataRevision, stats: &mut AudioStats) {
    for tag in rev.tags() {
        ingest_tag(tag, &mut stats.metadata, &mut stats.has_lyrics);
    }
    if !rev.visuals().is_empty() {
        stats.has_album_art = true;
    }
}

fn ingest_tag(tag: &Tag, m: &mut AudioMetadata, has_lyrics: &mut bool) {
    let Some(key) = tag.std_key else {
        // Non-standard tag: catch the obvious unsynced-lyrics name on
        // the raw key so `LYRICS=` Vorbis comments (which symphonia
        // doesn't always map to StandardTagKey::Lyrics) still surface.
        if tag.key.eq_ignore_ascii_case("lyrics") {
            *has_lyrics = true;
        }
        return;
    };
    let v = tag_value_string(&tag.value);
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
        StandardTagKey::Lyrics => {
            *has_lyrics = true;
        }
        _ => {}
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
