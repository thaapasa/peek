//! The peek user manual, embedded in the binary (`peek --manual`).
//!
//! `build.rs` turns `manual/src/` into a static chapter table
//! ([`ENTRIES`]). The manual is a virtual container of type
//! [`FileType::Manual`]: its source is `SUMMARY.md` in memory, its listing
//! is the TOC ([`toc::ManualTocSource`]), and each chapter extracts as an
//! in-memory `.md` source. Normal detection then routes a chapter to the
//! Markdown viewer, so this crate needs nothing from `peek-types`.
//!
//! The bin's compose / gather / extract hubs call [`compose`],
//! [`gather_extras`] and [`extract`]; the CLI builds the session from
//! [`source`] and [`detected`].

mod info;
#[cfg(test)]
mod preprocess;
mod toc;

use peek_detect::{Detected, FileType};
use peek_foundation::extract::{ExtractError, Extracted};
use peek_foundation::info::Extras;
use peek_foundation::viewer::listing::ListingMode;
use peek_foundation::viewer::modes::Mode;
use peek_io::InputSource;
pub use toc::ManualTocSource;

/// Display name of the manual source: the breadcrumb root and Info name.
pub const SOURCE_NAME: &str = "peek manual";

/// One `SUMMARY.md` row.
pub(crate) enum TocEntry {
    /// Part heading (`File Types`).
    Part(&'static str),
    Chapter(Chapter),
}

pub(crate) struct Chapter {
    pub title: &'static str,
    /// Path relative to `manual/src/`; also the extract key.
    pub path: &'static str,
    /// List nesting level inside `SUMMARY.md` (0 = top-level item).
    pub depth: usize,
    /// Preprocessed Markdown.
    pub body: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/manual.rs"));

pub(crate) fn chapters() -> impl Iterator<Item = &'static Chapter> {
    ENTRIES.iter().filter_map(|e| match e {
        TocEntry::Chapter(c) => Some(c),
        TocEntry::Part(_) => None,
    })
}

/// Container source: `SUMMARY.md` in memory, so Hex and Info have bytes.
pub fn source() -> InputSource {
    InputSource::memory(bytes::Bytes::from_static(SUMMARY.as_bytes()), SOURCE_NAME)
}

pub fn detected() -> Detected {
    Detected {
        file_type: FileType::Manual,
        magic_mime: None,
        decompressed_from: None,
    }
}

pub fn compose(modes: &mut Vec<Box<dyn Mode>>) {
    modes.push(Box::new(ListingMode::from_source(
        Box::new(ManualTocSource::new()),
        "Contents",
        Vec::new(),
    )));
}

pub fn gather_extras() -> Extras {
    info::gather_extras()
}

/// Extract a chapter by its path (`viewer/themes.md`; a leading `./` is
/// accepted). The result is named by its path, so detection sees `.md`.
pub fn extract(key: &str) -> Result<Extracted, ExtractError> {
    let wanted = key.trim_start_matches("./").trim_start_matches('/');
    let chapter = chapters()
        .find(|c| c.path == wanted)
        .ok_or_else(|| ExtractError::NotFound(key.to_string()))?;
    let suggested_name = chapter
        .path
        .rsplit('/')
        .next()
        .unwrap_or(chapter.path)
        .to_string();
    Ok(Extracted {
        source: InputSource::memory(
            bytes::Bytes::from_static(chapter.body.as_bytes()),
            chapter.path,
        ),
        suggested_name,
    })
}

#[cfg(test)]
mod tests;
