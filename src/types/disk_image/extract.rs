//! Extract a single ISO 9660 entry. File-backed ISOs map the entry to
//! a zero-copy `InputSource::FileRange`; stdin-piped ISOs use
//! `Bytes::slice` over the in-memory buffer (also zero-copy). DMG is
//! unsupported — UDIF block decompression is a separate decoder.

use std::path::Path;

use crate::extract::{ExtractError, Extracted, sanitize_entry_path};
use crate::input::InputSource;
use crate::input::detect::DiskImageFormat;

pub fn extract(
    source: &InputSource,
    format: DiskImageFormat,
    key: &str,
) -> Result<Extracted, ExtractError> {
    match format {
        DiskImageFormat::Iso => extract_iso(source, key),
        DiskImageFormat::Dmg => Err(ExtractError::Unsupported(
            "DMG extraction is not implemented (UDIF block decompression required)",
        )),
        DiskImageFormat::Raw => Err(ExtractError::Unsupported(
            "raw disk images expose no per-file structure to extract",
        )),
    }
}

fn extract_iso(source: &InputSource, key: &str) -> Result<Extracted, ExtractError> {
    let target = sanitize_entry_path(key)?;
    let (offset, len) = super::iso_listing::lookup_file_range(source, &target)
        .map_err(ExtractError::Other)?
        .ok_or_else(|| ExtractError::NotFound(key.to_string()))?;

    let suggested_name = suggested_name(&target);
    // Zero-copy view into the backing source. Tempfile-backed sources
    // (recursive ISO inside a spooled archive entry) yield a guarded
    // `FileRange` rather than buffering the range into memory.
    let extracted_source = source.subrange(offset, len, &suggested_name);
    Ok(Extracted {
        suggested_name,
        source: extracted_source,
    })
}

fn suggested_name(target: &Path) -> String {
    target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("extracted")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> InputSource {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("test-data");
        p.push(name);
        InputSource::File(p)
    }

    /// `sample.iso` (per iso_listing tests): README.txt = "primary\n",
    /// config.ini = "config\n", sub/inner.txt = "leaf\n",
    /// sub/deeper/deep.txt = "deep\n".
    #[test]
    fn extract_top_level_iso_file() {
        let extracted = extract(&fixture("sample.iso"), DiskImageFormat::Iso, "README.txt")
            .expect("ISO extract");
        assert_eq!(extracted.suggested_name, "README.txt");
        let bytes = extracted.source.read_bytes().unwrap();
        assert_eq!(bytes.as_ref(), b"primary\n");
    }

    #[test]
    fn extract_nested_iso_file() {
        let extracted = extract(
            &fixture("sample.iso"),
            DiskImageFormat::Iso,
            "sub/deeper/deep.txt",
        )
        .expect("nested ISO extract");
        assert_eq!(extracted.suggested_name, "deep.txt");
        let bytes = extracted.source.read_bytes().unwrap();
        assert_eq!(bytes.as_ref(), b"deep\n");
    }

    #[test]
    fn extract_iso_returns_file_range_for_file_source() {
        let extracted =
            extract(&fixture("sample.iso"), DiskImageFormat::Iso, "README.txt").unwrap();
        assert!(
            matches!(extracted.source, InputSource::FileRange { .. }),
            "file-backed ISO extract should produce a FileRange (zero-copy)"
        );
    }

    /// Recursive ISO sitting inside a spooled archive entry: the source
    /// is a `TempFile`. Pre-Arc-guard this buffered the range into memory;
    /// now it yields a guarded `FileRange` that outlives the source it was
    /// carved from.
    #[test]
    fn extract_iso_from_tempfile_source_returns_guarded_file_range() {
        let iso = std::fs::read(fixture("sample.iso").disk_path().unwrap()).unwrap();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut tmp, &iso).unwrap();
        std::io::Write::flush(&mut tmp).unwrap();
        let src = InputSource::temp_file(tmp, "sample.iso".to_string());

        let extracted = extract(&src, DiskImageFormat::Iso, "README.txt").unwrap();
        match &extracted.source {
            InputSource::FileRange { guard, .. } => {
                assert!(guard.is_some(), "range over a spooled ISO must be guarded");
            }
            other => panic!("expected guarded FileRange, got {other:?}"),
        }
        // Guard keeps the spool linked after the originating source drops.
        let view = extracted.source;
        drop(src);
        assert_eq!(view.read_bytes().unwrap().as_ref(), b"primary\n");
    }

    #[test]
    fn extract_iso_missing_file_errors() {
        let err = extract(&fixture("sample.iso"), DiskImageFormat::Iso, "no/such").unwrap_err();
        assert!(matches!(err, ExtractError::NotFound(_)));
    }

    #[test]
    fn extract_iso_directory_path_errors() {
        let err = extract(&fixture("sample.iso"), DiskImageFormat::Iso, "sub").unwrap_err();
        assert!(matches!(err, ExtractError::NotFound(_)));
    }

    #[test]
    fn extract_dmg_unsupported() {
        // Even with no DMG fixture, the format-level check fires first.
        let dummy = InputSource::memory(bytes::Bytes::new(), "x.dmg");
        let err = extract(&dummy, DiskImageFormat::Dmg, "anything").unwrap_err();
        assert!(matches!(err, ExtractError::Unsupported(_)));
    }
}
