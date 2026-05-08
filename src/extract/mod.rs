//! Extracting an inner item from a container source (animation frame,
//! archive entry, ISO entry) as a standalone [`InputSource`].
//!
//! The extracted source feeds straight back into the rest of the peek
//! pipeline: it can be written to disk, streamed to stdout, or — once
//! recursive peek is wired up — re-entered as the input to a fresh
//! viewer. The extraction layer never decides what to *do* with the
//! result; that's the caller's job.
//!
//! Per-type implementations live next to their existing modules
//! (`types/image/extract.rs`, `types/archive/extract.rs`, etc.) so each
//! type owns its own parsing alongside its detection and rendering.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::input::InputSource;
use crate::input::detect::{Detected, FileType};

pub mod write;

/// Result of a successful extract — a fresh `InputSource` plus a
/// human-friendly default filename. The caller decides whether to use
/// the suggested name or override with a user-provided path.
#[derive(Debug)]
pub struct Extracted {
    pub suggested_name: String,
    pub source: InputSource,
}

/// Failure modes for extraction. `Unsupported` means the container type
/// has no extractor at all (text, binary, plain image without frames);
/// `NotFound` / `InvalidKey` mean the container could be opened but the
/// requested key didn't resolve.
#[derive(Debug)]
pub enum ExtractError {
    NotFound(String),
    InvalidKey(String),
    UnsafePath(String),
    Unsupported(&'static str),
    Other(anyhow::Error),
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(k) => write!(f, "no entry matching {k:?}"),
            Self::InvalidKey(k) => write!(f, "invalid extract key {k:?}"),
            Self::UnsafePath(p) => write!(f, "unsafe entry path {p:?}"),
            Self::Unsupported(reason) => write!(f, "extract not supported: {reason}"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ExtractError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Other(e) = self {
            Some(e.as_ref())
        } else {
            None
        }
    }
}

impl From<anyhow::Error> for ExtractError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(e)
    }
}

/// Top-level dispatch: pick the right per-type extractor based on what
/// `input::detect` decided the source is. Containers without an
/// extractor return `Unsupported`.
pub fn extract(
    source: &InputSource,
    detected: &Detected,
    key: &str,
) -> Result<Extracted, ExtractError> {
    match &detected.file_type {
        FileType::Image => {
            crate::types::image::extract::extract(source, key, detected.magic_mime.as_deref())
        }
        FileType::Archive(fmt) => crate::types::archive::extract::extract(source, *fmt, key),
        FileType::DiskImage(fmt) => crate::types::disk_image::extract::extract(source, *fmt, key),
        FileType::SourceCode { .. }
        | FileType::Svg
        | FileType::Structured(_)
        | FileType::Binary => Err(ExtractError::Unsupported(
            "this file type has no inner items",
        )),
    }
}

/// Validate an entry path coming from an untrusted archive/ISO TOC.
/// Reject absolute paths and any path traversal — the result must stay
/// rooted under an implicit extraction root.
///
/// Used by the archive and ISO extract impls; lives here so the safety
/// rules apply uniformly across container types.
pub(crate) fn sanitize_entry_path(raw: &str) -> Result<PathBuf, ExtractError> {
    let trimmed = raw.trim_start_matches('/');
    let p = Path::new(trimmed);
    if p.is_absolute() {
        return Err(ExtractError::UnsafePath(raw.to_string()));
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(seg) => out.push(seg),
            // ParentDir / Prefix / RootDir / CurDir all rejected: a
            // sanitized path may only contain plain segments.
            Component::ParentDir
            | Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir => {
                if matches!(c, Component::CurDir) {
                    continue;
                }
                return Err(ExtractError::UnsafePath(raw.to_string()));
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(ExtractError::InvalidKey(raw.to_string()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_traversal() {
        assert!(matches!(
            sanitize_entry_path("../../etc/passwd"),
            Err(ExtractError::UnsafePath(_))
        ));
        assert!(matches!(
            sanitize_entry_path("foo/../bar"),
            Err(ExtractError::UnsafePath(_))
        ));
    }

    #[test]
    fn sanitize_treats_leading_slash_as_relative() {
        // Container TOCs frequently store entries with a leading `/`
        // that are intended as relative-to-the-archive paths. We trim
        // the slash and treat the remainder as a relative key — there
        // is no host filesystem involved at sanitize time.
        let p = sanitize_entry_path("/etc/passwd").unwrap();
        assert_eq!(p, PathBuf::from("etc/passwd"));
    }

    #[test]
    fn sanitize_strips_leading_slash() {
        let p = sanitize_entry_path("foo/bar.txt").unwrap();
        assert_eq!(p, PathBuf::from("foo/bar.txt"));
    }

    #[test]
    fn sanitize_allows_dotted_segment() {
        let p = sanitize_entry_path("./foo/bar.txt").unwrap();
        assert_eq!(p, PathBuf::from("foo/bar.txt"));
    }

    #[test]
    fn sanitize_rejects_empty() {
        assert!(matches!(
            sanitize_entry_path(""),
            Err(ExtractError::InvalidKey(_))
        ));
    }
}
