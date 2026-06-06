//! Render the audio info section. Layout follows the document /
//! ebook / PDF sections so the metadata block is visually consistent
//! across file types.

use crate::info::{push_field, push_section_header, thousands_sep};
use crate::theme::PeekTheme;

use super::info::AudioStats;
use crate::input::detect::AudioFormat;

pub fn render_section(lines: &mut Vec<String>, stats: &AudioStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Audio", theme);

    if let Some(err) = &stats.error {
        push_field(lines, "Error", &theme.paint_warning(err), theme);
        return;
    }

    push_field(
        lines,
        "Format",
        &theme.paint_value(stats.format.label()),
        theme,
    );
    if let Some(codec) = &stats.codec {
        push_field(lines, "Codec", &theme.paint_value(codec), theme);
    }
    if let Some(secs) = stats.duration_secs {
        push_field(
            lines,
            "Duration",
            &theme.paint_value(&format_duration(secs)),
            theme,
        );
    }
    if let Some(ch) = stats.channels {
        let label = match &stats.channel_layout {
            Some(layout) => format!("{ch} ({layout})"),
            None => ch.to_string(),
        };
        push_field(lines, "Channels", &theme.paint_value(&label), theme);
    }
    if let Some(rate) = stats.sample_rate {
        push_field(
            lines,
            "Sample rate",
            &theme.paint_value(&format!("{} Hz", thousands_sep(rate as u64))),
            theme,
        );
    }
    if let Some(bits) = stats.bits_per_sample {
        push_field(
            lines,
            "Bit depth",
            &theme.paint_value(&format!("{bits}-bit")),
            theme,
        );
    }
    if let Some(br) = stats.bitrate {
        push_field(
            lines,
            "Bitrate",
            &theme.paint_value(&format_bitrate(br)),
            theme,
        );
    }

    let m = &stats.metadata;
    let has_tag = m.title.is_some()
        || m.artist.is_some()
        || m.album.is_some()
        || m.album_artist.is_some()
        || m.track_number.is_some()
        || m.disc_number.is_some()
        || m.date.is_some()
        || m.genre.is_some()
        || m.composer.is_some()
        || m.comment.is_some()
        || stats.has_lyrics
        || stats.has_album_art;
    if !has_tag {
        return;
    }

    lines.push(String::new());
    push_section_header(lines, "Tags", theme);
    if let Some(v) = &m.title {
        push_field(lines, "Title", &theme.paint_value(v), theme);
    }
    if let Some(v) = &m.artist {
        push_field(lines, "Artist", &theme.paint_value(v), theme);
    }
    if let Some(v) = &m.album_artist
        && Some(v) != m.artist.as_ref()
    {
        push_field(lines, "Album artist", &theme.paint_value(v), theme);
    }
    if let Some(v) = &m.album {
        push_field(lines, "Album", &theme.paint_value(v), theme);
    }
    if let Some(v) = &m.track_number {
        push_field(lines, "Track", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &m.disc_number {
        push_field(lines, "Disc", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &m.date {
        push_field(lines, "Date", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &m.genre {
        push_field(lines, "Genre", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &m.composer {
        push_field(lines, "Composer", &theme.paint_muted(v), theme);
    }
    if let Some(v) = &m.comment {
        push_field(lines, "Comment", &theme.paint_muted(v), theme);
    }
    if stats.has_lyrics {
        push_field(lines, "Lyrics", &theme.paint_value("embedded"), theme);
    }
    if stats.has_album_art {
        push_field(lines, "Album art", &theme.paint_value("embedded"), theme);
    }
}

/// Typed `--info --json` encoding of the Audio section. Durations,
/// rates, and bitrates are raw numbers (seconds / Hz / bits-per-second),
/// never the human-formatted display strings. `error` is present only
/// when the probe failed.
pub fn json_section(stats: &AudioStats) -> (&'static str, serde_json::Value) {
    let mut obj = serde_json::json!({
        "format": audio_format_token(stats.format),
    });
    if let Some(ref err) = stats.error {
        obj["error"] = serde_json::json!(err);
        return ("audio", obj);
    }
    if let Some(ref codec) = stats.codec {
        obj["codec"] = serde_json::json!(codec);
    }
    if let Some(secs) = stats.duration_secs {
        obj["duration_secs"] = serde_json::json!(secs);
    }
    if let Some(rate) = stats.sample_rate {
        obj["sample_rate"] = serde_json::json!(rate);
    }
    if let Some(ch) = stats.channels {
        obj["channels"] = serde_json::json!(ch);
    }
    if let Some(ref layout) = stats.channel_layout {
        obj["channel_layout"] = serde_json::json!(layout);
    }
    if let Some(bits) = stats.bits_per_sample {
        obj["bits_per_sample"] = serde_json::json!(bits);
    }
    if let Some(br) = stats.bitrate {
        obj["bitrate"] = serde_json::json!(br);
    }

    let m = &stats.metadata;
    let mut tags = serde_json::Map::new();
    if let Some(ref v) = m.title {
        tags.insert("title".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.artist {
        tags.insert("artist".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.album {
        tags.insert("album".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.album_artist {
        tags.insert("album_artist".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.track_number {
        tags.insert("track_number".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.disc_number {
        tags.insert("disc_number".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.date {
        tags.insert("date".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.genre {
        tags.insert("genre".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.composer {
        tags.insert("composer".into(), serde_json::json!(v));
    }
    if let Some(ref v) = m.comment {
        tags.insert("comment".into(), serde_json::json!(v));
    }
    if !tags.is_empty() {
        obj["tags"] = serde_json::Value::Object(tags);
    }
    obj["has_lyrics"] = serde_json::json!(stats.has_lyrics);
    obj["has_album_art"] = serde_json::json!(stats.has_album_art);
    ("audio", obj)
}

fn audio_format_token(format: AudioFormat) -> &'static str {
    match format {
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
        AudioFormat::Wma => "wma",
    }
}

/// `H:MM:SS` for ≥1h tracks, `M:SS` otherwise. Sub-second tail is
/// dropped — bit-perfect duration isn't useful in a metadata view.
fn format_duration(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `N kbps` for the common range, `N.N Mbps` for very high (24-bit
/// FLAC, uncompressed PCM).
fn format_bitrate(bps: u64) -> String {
    if bps >= 1_000_000 {
        format!("{:.1} Mbps", bps as f64 / 1_000_000.0)
    } else {
        format!("{} kbps", (bps + 500) / 1000)
    }
}
