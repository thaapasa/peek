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

// Extension- and MIME-based audio container detection. The symphonia
// probe handles codec-level classification on top of these container
// formats; this layer only needs to route the file to the audio
// viewer.

/// Map a single file extension to an audio container format.
pub fn format_from_ext(ext: &str) -> Option<AudioFormat> {
    match ext {
        "mp3" => Some(AudioFormat::Mp3),
        "flac" => Some(AudioFormat::Flac),
        "ogg" | "oga" => Some(AudioFormat::Ogg),
        "opus" => Some(AudioFormat::Opus),
        "wav" | "wave" => Some(AudioFormat::Wav),
        "m4a" | "m4b" | "m4p" => Some(AudioFormat::M4a),
        "aac" => Some(AudioFormat::Aac),
        "aiff" | "aif" | "aifc" => Some(AudioFormat::Aiff),
        "caf" => Some(AudioFormat::Caf),
        "mka" => Some(AudioFormat::Mka),
        "wma" => Some(AudioFormat::Wma),
        _ => None,
    }
}

/// Map an `infer` magic-byte MIME to an audio container format.
pub fn format_from_mime(mime: &str) -> Option<AudioFormat> {
    match mime {
        "audio/mpeg" | "audio/mp3" => Some(AudioFormat::Mp3),
        "audio/flac" | "audio/x-flac" => Some(AudioFormat::Flac),
        "audio/ogg" | "application/ogg" => Some(AudioFormat::Ogg),
        "audio/opus" => Some(AudioFormat::Opus),
        "audio/wav" | "audio/wave" | "audio/x-wav" => Some(AudioFormat::Wav),
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => Some(AudioFormat::M4a),
        "audio/aac" => Some(AudioFormat::Aac),
        "audio/aiff" | "audio/x-aiff" => Some(AudioFormat::Aiff),
        "audio/x-caf" => Some(AudioFormat::Caf),
        "audio/x-matroska" => Some(AudioFormat::Mka),
        "audio/x-ms-wma" => Some(AudioFormat::Wma),
        _ => None,
    }
}
