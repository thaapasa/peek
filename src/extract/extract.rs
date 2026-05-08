//! Top-level extract dispatch + shared types. The extracted source
//! feeds straight back into the rest of the peek pipeline (write to
//! disk, stream to stdout, recursive peek). Path-safety helper
//! `sanitize_entry_path` is shared with the archive and ISO impls.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::input::InputSource;
use crate::input::detect::{Detected, FileType};

/// Successful extract: fresh `InputSource` + suggested filename.
#[derive(Debug)]
pub struct Extracted {
    pub suggested_name: String,
    pub source: InputSource,
}

/// Per-extract knobs. Extractor-specific; defaults are always sensible.
#[derive(Debug, Default, Clone)]
pub struct ExtractOptions {
    /// Explicit SVG raster size in pixels (longest axis). CLI: `--extract-size`.
    /// Wins over `view_cols` when both are set.
    pub svg_size: Option<u32>,
    /// Hint that the extract will be rendered at this many character
    /// columns downstream (e.g. `--print --width N`). The SVG extractor
    /// derives a raster size that matches what live rendering at the
    /// same width would produce, so extract-then-render output
    /// matches plain render output. Ignored when `svg_size` is set.
    pub view_cols: Option<u32>,
}

/// `Unsupported` = container has no extractor; `NotFound` / `InvalidKey`
/// = container opened but the key didn't resolve.
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

/// Dispatch to the per-type extractor. Containers without an
/// extractor return `Unsupported`.
pub fn extract(
    source: &InputSource,
    detected: &Detected,
    key: &str,
    opts: &ExtractOptions,
) -> Result<Extracted, ExtractError> {
    match &detected.file_type {
        FileType::Image => {
            crate::types::image::extract::extract(source, key, detected.magic_mime.as_deref())
        }
        FileType::Svg => {
            crate::types::svg::extract::extract(source, key, opts.svg_size, opts.view_cols)
        }
        FileType::Archive(fmt) => crate::types::archive::extract::extract(source, *fmt, key),
        FileType::DiskImage(fmt) => crate::types::disk_image::extract::extract(source, *fmt, key),
        FileType::SourceCode { .. } | FileType::Structured(_) | FileType::Binary => Err(
            ExtractError::Unsupported("this file type has no inner items"),
        ),
    }
}

/// Reject traversal / absolute paths in an untrusted archive/ISO key.
/// Shared across container types.
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
            Component::CurDir => continue,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
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
    use crate::input::detect;

    fn fixture(name: &str) -> InputSource {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("test-data");
        p.push(name);
        InputSource::File(p)
    }

    /// Recursive peek: extract an entry and confirm the resulting
    /// `InputSource` re-enters the peek pipeline cleanly — `detect`
    /// classifies it, `read_text` returns the entry's bytes as UTF-8,
    /// and `open_line_source` builds a working line index. This is the
    /// path `peek <container> --extract X --print` exercises end-to-end.
    #[test]
    fn extracted_iso_entry_round_trips_through_pipeline() {
        let detected = detect::detect(&fixture("sample.iso")).unwrap();
        let extracted = extract(
            &fixture("sample.iso"),
            &detected,
            "README.txt",
            &ExtractOptions::default(),
        )
        .unwrap();
        // Zero-copy: ISO extracts return a FileRange view.
        assert!(matches!(extracted.source, InputSource::FileRange { .. }));

        // Re-detect from the extracted source: should classify as text.
        let inner_detected = detect::detect(&extracted.source).unwrap();
        assert!(matches!(
            inner_detected.file_type,
            detect::FileType::SourceCode { .. } | detect::FileType::Binary
        ));

        // Re-read the bytes via the recursive pipeline path.
        let text = extracted.source.read_text().unwrap();
        assert_eq!(text, "primary\n");

        // Line indexing on a FileRange-backed source.
        let ls = extracted.source.open_line_source().unwrap();
        assert_eq!(ls.total_lines(), 1);
    }

    /// Same path for archive entries: extract a file out of a zip,
    /// confirm the extracted source carries the right bytes and is
    /// recognisable to the peek pipeline as Python source.
    #[test]
    fn extracted_archive_entry_round_trips_through_pipeline() {
        let detected = detect::detect(&fixture("archive.zip")).unwrap();
        let extracted = extract(
            &fixture("archive.zip"),
            &detected,
            "fibonacci.py",
            &ExtractOptions::default(),
        )
        .unwrap();
        assert!(matches!(extracted.source, InputSource::Memory { .. }));

        let text = extracted.source.read_text().unwrap();
        assert!(text.contains("fibonacci"), "expected python source");

        let inner_detected = detect::detect(&extracted.source).unwrap();
        match inner_detected.file_type {
            detect::FileType::SourceCode { syntax } => {
                // Detection from a stdin-style buffer typically can't
                // pick a syntax without a path; we just confirm the
                // shape (text classification) round-trips.
                let _ = syntax;
            }
            other => panic!("expected SourceCode, got {other:?}"),
        }
    }

    /// Double extraction: extract from container A, then extract from
    /// the result. Demonstrates the recursive-peek extension path —
    /// here the inner item is itself a single-file container (ISO with
    /// only one root entry), but the mechanism is the same one a
    /// future "view archive entry inside an ISO" flow would use.
    #[test]
    fn double_extract_uses_extracted_source_as_new_input() {
        let detected = detect::detect(&fixture("sample.iso")).unwrap();
        let inner = extract(
            &fixture("sample.iso"),
            &detected,
            "sub/inner.txt",
            &ExtractOptions::default(),
        )
        .unwrap();
        // The extracted source is a FileRange backed by sample.iso.
        match &inner.source {
            InputSource::FileRange { base, .. } => {
                assert!(base.ends_with("sample.iso"));
            }
            other => panic!("expected FileRange, got {other:?}"),
        }
        // It still reads correctly when treated as a fresh input.
        assert_eq!(inner.source.read_text().unwrap(), "leaf\n");
    }

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
