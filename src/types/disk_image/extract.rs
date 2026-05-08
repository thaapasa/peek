//! Extract a single ISO 9660 entry as a standalone [`InputSource`].
//!
//! For file-backed ISOs we map the entry directly onto the underlying
//! file via `InputSource::FileRange` — no decompression, no buffering,
//! no copy. Stdin-piped ISOs already have the full bytes in memory, so
//! the entry is exposed via `Bytes::slice` (also zero-copy).
//!
//! DMG is intentionally unsupported: the UDIF format chunks payload
//! into compressed blocks that would need a separate decoder.

use std::path::PathBuf;

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
    }
}

fn extract_iso(source: &InputSource, key: &str) -> Result<Extracted, ExtractError> {
    let target = sanitize_entry_path(key)?;
    let (offset, len) = super::iso_listing::lookup_file_range(source, &target)
        .map_err(ExtractError::Other)?
        .ok_or_else(|| ExtractError::NotFound(key.to_string()))?;

    let suggested_name = suggested_name(&target);
    let extracted_source = build_extracted_source(source, offset, len, &suggested_name);
    Ok(Extracted {
        suggested_name,
        source: extracted_source,
    })
}

fn build_extracted_source(
    source: &InputSource,
    offset: u64,
    len: u64,
    suggested_name: &str,
) -> InputSource {
    match source {
        InputSource::File(path) => {
            InputSource::file_range(path.clone(), offset, len, suggested_name.to_string())
        }
        InputSource::Memory { bytes, .. } => {
            // Stdin-piped or already-extracted ISO: zero-copy slice over
            // the existing Bytes buffer.
            let start = offset as usize;
            let end = (start + len as usize).min(bytes.len());
            let sliced = bytes.slice(start..end);
            InputSource::memory(sliced, suggested_name.to_string())
        }
        InputSource::FileRange {
            base,
            offset: base_off,
            len: base_len,
            ..
        } => {
            // Recursive case: the ISO is itself a range inside another
            // file. Collapse offsets so we don't nest ranges.
            let abs_off = base_off.saturating_add(offset);
            let max = base_off.saturating_add(*base_len);
            let clamped_len = len.min(max.saturating_sub(abs_off));
            InputSource::file_range(
                base.clone(),
                abs_off,
                clamped_len,
                suggested_name.to_string(),
            )
        }
    }
}

fn suggested_name(target: &PathBuf) -> String {
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
        assert_eq!(bytes, b"primary\n");
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
        assert_eq!(bytes, b"deep\n");
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
