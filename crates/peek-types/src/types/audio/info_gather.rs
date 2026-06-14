//! Populate [`AudioStats`] via [`super::package::probe`]. Probe-only —
//! no audio decoding. Track 0 supplies the codec params (channels /
//! sample rate / bit depth / duration); tag walking + visual + lyric
//! extraction live in [`super::package`].

use crate::info::Extras;
use crate::types::audio::info::AudioStats;
use peek_detect::AudioFormat;
use peek_io::InputSource;

use super::package;

pub fn gather_extras(source: &InputSource, format: AudioFormat) -> Extras {
    match package::probe(source, format) {
        Ok(probed) => Box::new(package::to_stats(&probed)),
        Err(e) => {
            let mut stats = AudioStats::empty(format);
            stats.error = Some(format!("{e:#}"));
            Box::new(stats)
        }
    }
}
