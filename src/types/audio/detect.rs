//! Extension- and MIME-based audio container detection. The symphonia
//! probe handles codec-level classification on top of these container
//! formats; this layer only needs to route the file to the audio
//! viewer.

use super::format::AudioFormat;

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
