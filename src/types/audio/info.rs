//! Shared audio info shape. One struct covers every container peek
//! recognises — tag field set is the union over ID3v1/v2, Vorbis
//! comments, MP4 atoms, and APE; per-format quirks collapse to the
//! same display rows.

use crate::input::detect::AudioFormat;

#[derive(Debug, Clone, Default)]
pub struct AudioMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_number: Option<String>,
    pub disc_number: Option<String>,
    /// Release year / date. Often just the year ("1997"); sometimes a
    /// full ISO date ("1997-04-21"). Surfaced as-is from the tag.
    pub date: Option<String>,
    pub genre: Option<String>,
    pub composer: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AudioStats {
    pub format: AudioFormat,
    /// Codec long-form label (e.g. "MP3 (MPEG-1 Audio Layer 3)",
    /// "FLAC", "AAC (Advanced Audio Coding)"). Empty for containers
    /// where the codec is implicit in the format label and symphonia
    /// returned nothing extra.
    pub codec: Option<String>,
    /// Total play time in seconds. `None` when the track header doesn't
    /// carry a frame count / duration (some streamable formats).
    pub duration_secs: Option<f64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    /// Channel layout label when symphonia can name it (e.g. "stereo",
    /// "5.1 surround"); falls back to None and the renderer shows just
    /// the channel count.
    pub channel_layout: Option<String>,
    /// Codec bit depth (16 / 24 / 32). `None` for compressed codecs
    /// where bit depth is set per-frame (MP3 / AAC / Vorbis).
    pub bits_per_sample: Option<u32>,
    /// Average bitrate in bits per second. Derived from file size /
    /// duration when the container doesn't carry an explicit field.
    pub bitrate: Option<u64>,
    pub metadata: AudioMetadata,
    /// True when the file carries embedded lyrics (USLT/SYLT for ID3v2,
    /// `LYRICS=` Vorbis comment, `\xa9lyr` MP4 atom).
    pub has_lyrics: bool,
    /// True when the file carries an embedded picture (cover art).
    pub has_album_art: bool,
    /// User-facing reason the probe failed. Surfaced in place of the
    /// stat rows when present.
    pub error: Option<String>,
}

impl AudioStats {
    pub fn empty(format: AudioFormat) -> Self {
        Self {
            format,
            codec: None,
            duration_secs: None,
            sample_rate: None,
            channels: None,
            channel_layout: None,
            bits_per_sample: None,
            bitrate: None,
            metadata: AudioMetadata::default(),
            has_lyrics: false,
            has_album_art: false,
            error: None,
        }
    }
}
