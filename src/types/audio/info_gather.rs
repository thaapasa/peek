//! Populate [`AudioStats`] via [`super::package::probe`]. Probe-only —
//! no audio decoding. Track 0 supplies the codec params (channels /
//! sample rate / bit depth / duration); tag walking + visual + lyric
//! extraction live in [`super::package`].

use crate::info::FileExtras;
use crate::input::InputSource;
use crate::input::detect::AudioFormat;
use crate::types::audio::info::AudioStats;

use super::package;

pub fn gather_extras(source: &InputSource, format: AudioFormat) -> FileExtras {
    match package::probe(source, format) {
        Ok(probed) => FileExtras::Audio(package::to_stats(&probed)),
        Err(e) => {
            let mut stats = AudioStats::empty(format);
            stats.error = Some(format!("{e:#}"));
            FileExtras::Audio(stats)
        }
    }
}
