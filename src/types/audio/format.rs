//! Audio container format enum + display label.

/// Sound-file container. Encompasses the common consumer audio formats;
/// the symphonia probe resolves codec details (e.g. ALAC inside an
/// M4a container) on top of this container-level classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    Flac,
    /// Ogg container — usually Vorbis, sometimes FLAC.
    Ogg,
    /// Ogg container carrying an Opus stream (`.opus`).
    Opus,
    Wav,
    /// MPEG-4 audio container (`.m4a` / `.m4b` / `.mp4` audio-only /
    /// `.aac` in ADTS).
    M4a,
    /// Raw AAC ADTS stream (`.aac`).
    Aac,
    /// Audio Interchange File Format (`.aiff` / `.aif`).
    Aiff,
    /// Apple Core Audio Format (`.caf`).
    Caf,
    /// Matroska audio (`.mka`).
    Mka,
    /// Windows Media Audio (`.wma`).
    Wma,
}

impl AudioFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mp3 => "MP3",
            Self::Flac => "FLAC",
            Self::Ogg => "Ogg",
            Self::Opus => "Opus",
            Self::Wav => "WAV",
            Self::M4a => "MPEG-4 audio",
            Self::Aac => "AAC",
            Self::Aiff => "AIFF",
            Self::Caf => "CAF",
            Self::Mka => "Matroska audio",
            Self::Wma => "WMA",
        }
    }
}
