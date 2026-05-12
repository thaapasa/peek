//! Render the audio info section. Layout follows the document /
//! ebook / PDF sections so the metadata block is visually consistent
//! across file types.

use crate::info::{push_field, push_section_header, thousands_sep};
use crate::theme::PeekTheme;

use super::info::AudioStats;

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
