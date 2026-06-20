//! The audio info section: an Audio block plus a Tags block, driven by one
//! [`AudioView`] that derives both `serde::Serialize` (JSON) and
//! [`InfoView`](crate::info::InfoView) (themed print). [`AudioStats`] stays
//! the gather struct; the view projects it.
//!
//! On a probe error only the `Error` row shows (but JSON still carries the
//! `format` token). Durations / rates / bitrates print human-formatted but
//! serialize as raw numbers. `has_lyrics` / `has_album_art` print inside the
//! Tags block but are top-level JSON bools.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::json;

use crate::info::{InfoNode, InfoValue, Role, Value, Warn, thousands_sep};
use peek_theme::PeekTheme;

use super::info::{AudioMetadata, AudioStats};
use peek_detect::AudioFormat;

crate::info_section!(AudioStats, AudioView, "audio");

#[derive(Serialize, crate::info::InfoView)]
struct AudioView {
    #[info(nest)]
    #[serde(flatten)]
    audio: AudioBlock,
    #[info(nest)]
    #[serde(rename = "tags", skip_serializing_if = "Tags::no_json")]
    tags: Tags,
    // Printed inside the Tags block, but top-level JSON bools.
    #[info(skip)]
    has_lyrics: bool,
    #[info(skip)]
    has_album_art: bool,
}

#[derive(Serialize, crate::info::InfoView)]
#[info(title = "Audio")]
struct AudioBlock {
    #[info(label = "Error")]
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Warn>,
    // Print row only when the probe succeeded…
    #[info(label = "Format")]
    #[serde(skip)]
    format_print: Option<String>,
    // …JSON token always.
    #[info(skip)]
    #[serde(rename = "format")]
    format: &'static str,
    #[info(label = "Codec")]
    #[serde(skip_serializing_if = "Option::is_none")]
    codec: Option<String>,
    #[info(label = "Duration")]
    #[serde(rename = "duration_secs", skip_serializing_if = "Option::is_none")]
    duration: Option<Value>,
    #[info(label = "Channels")]
    #[serde(flatten)]
    channels: Option<Channels>,
    #[info(label = "Sample rate")]
    #[serde(rename = "sample_rate", skip_serializing_if = "Option::is_none")]
    sample_rate: Option<Value>,
    #[info(label = "Bit depth")]
    #[serde(rename = "bits_per_sample", skip_serializing_if = "Option::is_none")]
    bits_per_sample: Option<Value>,
    #[info(label = "Bitrate")]
    #[serde(rename = "bitrate", skip_serializing_if = "Option::is_none")]
    bitrate: Option<Value>,
}

impl From<&AudioStats> for AudioView {
    fn from(s: &AudioStats) -> Self {
        let ok = s.error.is_none();
        AudioView {
            audio: AudioBlock {
                error: s.error.clone().map(Warn),
                format_print: ok.then(|| s.format.label().to_string()),
                format: audio_format_token(s.format),
                codec: if ok { s.codec.clone() } else { None },
                duration: s
                    .duration_secs
                    .map(|d| Value::split(format_duration(d), Role::Value, json!(d))),
                channels: s.channels.map(|ch| Channels {
                    channels: ch,
                    layout: s.channel_layout.clone(),
                }),
                sample_rate: s.sample_rate.map(|r| {
                    Value::split(
                        format!("{} Hz", thousands_sep(r as u64)),
                        Role::Value,
                        json!(r),
                    )
                }),
                bits_per_sample: s
                    .bits_per_sample
                    .map(|b| Value::split(format!("{b}-bit"), Role::Value, json!(b))),
                bitrate: s
                    .bitrate
                    .map(|b| Value::split(format_bitrate(b), Role::Value, json!(b))),
            },
            tags: Tags {
                meta: s.metadata.clone(),
                has_lyrics: s.has_lyrics,
                has_album_art: s.has_album_art,
            },
            has_lyrics: s.has_lyrics,
            has_album_art: s.has_album_art,
        }
    }
}

/// Channel count + optional layout. Print: `N (layout)` / `N`. JSON:
/// `channels` + optional `channel_layout`.
struct Channels {
    channels: u16,
    layout: Option<String>,
}
impl InfoValue for Channels {
    fn render_value(&self, theme: &PeekTheme) -> String {
        let label = match &self.layout {
            Some(layout) => format!("{} ({layout})", self.channels),
            None => self.channels.to_string(),
        };
        theme.paint_value(&label)
    }
}
impl Serialize for Channels {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let len = 1 + self.layout.is_some() as usize;
        let mut st = ser.serialize_struct("channels", len)?;
        st.serialize_field("channels", &self.channels)?;
        if let Some(layout) = &self.layout {
            st.serialize_field("channel_layout", layout)?;
        }
        st.end()
    }
}

/// Tag block. Print: a `Tags` section (album-artist hidden when it equals the
/// artist; lyrics / album-art rows). JSON: a `tags` object of the textual tags
/// only (lyrics / art are top-level bools on the parent).
struct Tags {
    meta: AudioMetadata,
    has_lyrics: bool,
    has_album_art: bool,
}

impl Tags {
    fn no_json(&self) -> bool {
        let m = &self.meta;
        m.title.is_none()
            && m.artist.is_none()
            && m.album.is_none()
            && m.album_artist.is_none()
            && m.track_number.is_none()
            && m.disc_number.is_none()
            && m.date.is_none()
            && m.genre.is_none()
            && m.composer.is_none()
            && m.comment.is_none()
    }
}

impl crate::info::InfoView for Tags {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        let m = &self.meta;
        let has_tag = !self.no_json() || self.has_lyrics || self.has_album_art;
        if !has_tag {
            return Vec::new();
        }
        let mut body = Vec::new();
        let row = |label: &'static str, v: &str| InfoNode::Row {
            label: label.into(),
            value: theme.paint_value(v),
        };
        let muted = |label: &'static str, v: &str| InfoNode::Row {
            label: label.into(),
            value: theme.paint_muted(v),
        };
        if let Some(v) = &m.title {
            body.push(row("Title", v));
        }
        if let Some(v) = &m.artist {
            body.push(row("Artist", v));
        }
        if let Some(v) = &m.album_artist
            && Some(v) != m.artist.as_ref()
        {
            body.push(row("Album artist", v));
        }
        if let Some(v) = &m.album {
            body.push(row("Album", v));
        }
        if let Some(v) = &m.track_number {
            body.push(muted("Track", v));
        }
        if let Some(v) = &m.disc_number {
            body.push(muted("Disc", v));
        }
        if let Some(v) = &m.date {
            body.push(muted("Date", v));
        }
        if let Some(v) = &m.genre {
            body.push(muted("Genre", v));
        }
        if let Some(v) = &m.composer {
            body.push(muted("Composer", v));
        }
        if let Some(v) = &m.comment {
            body.push(muted("Comment", v));
        }
        if self.has_lyrics {
            body.push(row("Lyrics", "embedded"));
        }
        if self.has_album_art {
            body.push(row("Album art", "embedded"));
        }
        vec![InfoNode::Block {
            title: "Tags".to_string(),
            body,
        }]
    }
}

impl Serialize for Tags {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let m = &self.meta;
        let entries: [(&str, &Option<String>); 10] = [
            ("title", &m.title),
            ("artist", &m.artist),
            ("album", &m.album),
            ("album_artist", &m.album_artist),
            ("track_number", &m.track_number),
            ("disc_number", &m.disc_number),
            ("date", &m.date),
            ("genre", &m.genre),
            ("composer", &m.composer),
            ("comment", &m.comment),
        ];
        let present = entries.iter().filter(|(_, v)| v.is_some()).count();
        let mut map = ser.serialize_map(Some(present))?;
        for (k, v) in entries {
            if let Some(v) = v {
                map.serialize_entry(k, v)?;
            }
        }
        map.end()
    }
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

/// `H:MM:SS` for ≥1h tracks, `M:SS` otherwise.
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

fn format_bitrate(bps: u64) -> String {
    if bps >= 1_000_000 {
        format!("{:.1} Mbps", bps as f64 / 1_000_000.0)
    } else {
        format!("{} kbps", (bps + 500) / 1000)
    }
}
