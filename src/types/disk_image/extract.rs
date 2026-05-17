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
            let start = offset as usize;
            let end = (start + len as usize).min(bytes.len());
            InputSource::memory(bytes.slice(start..end), suggested_name.to_string())
        }
        InputSource::FileRange {
            base,
            offset: base_off,
            len: base_len,
            ..
        } => {
            // Recursive: ISO inside another file's range. Collapse to
            // a single range — never nest.
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
