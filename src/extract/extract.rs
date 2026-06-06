//! Top-level extract dispatch — the `FileType → types::<x>::extract` hub.
//! Session glue, so it lives in the binary; the value types it returns
//! ([`Extracted`] / [`ExtractError`] / [`ExtractOptions`]) and the
//! path-safety helpers come from `peek-foundation`. The extracted source
//! feeds straight back into the rest of the peek pipeline (write to disk,
//! stream to stdout, recursive peek).

use peek_detect::{ComicFormat, Detected, DocumentFormat, EbookFormat, FileType};
use peek_foundation::extract::{ExtractError, ExtractOptions, Extracted};
use peek_io::InputSource;
use peek_types::types;

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
            types::image::extract::extract(source, key, detected.magic_mime.as_deref())
        }
        FileType::Svg => types::svg::extract::extract(source, key, opts.svg_size, opts.view_cols),
        FileType::Archive(fmt) => types::archive::extract::extract(source, *fmt, key, opts),
        FileType::DiskImage(fmt) => types::disk_image::extract::extract(source, *fmt, key),
        FileType::Ebook(EbookFormat::Epub)
        | FileType::Comic(ComicFormat::Cbz)
        | FileType::Document(DocumentFormat::Docx | DocumentFormat::Odt) => {
            types::archive::extract::extract(source, peek_detect::ArchiveFormat::Zip, key, opts)
        }
        FileType::Document(DocumentFormat::Rtf) => {
            types::document::rtf::extract::extract(source, key)
        }
        FileType::Pdf(_) => types::pdf::extract::extract(source, key),
        FileType::Spreadsheet(fmt) => types::spreadsheet::extract::extract(source, key, *fmt, opts),
        FileType::Directory => types::directory::extract::extract(source, key),
        FileType::Audio(fmt) => types::audio::extract::extract(source, *fmt, key),
        FileType::Sqlite(_) => types::sqlite::extract::extract(source, key),
        FileType::Notebook => types::notebook::extract::extract(source, key),
        FileType::Email(_) => types::email::extract::extract(source, key),
        FileType::SourceCode { .. }
        | FileType::Structured(_)
        | FileType::Html
        | FileType::Markdown
        | FileType::Compressed(_)
        | FileType::Csv(_)
        | FileType::Cert(_)
        | FileType::Font(_)
        | FileType::ObjectFile
        | FileType::Classfile
        | FileType::PostScript(_)
        | FileType::VObject(_)
        | FileType::Binary => Err(ExtractError::Unsupported(
            "this file type has no inner items",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use peek_detect as detect;
    use std::path::PathBuf;

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

    /// DOCX is treated as a ZIP container — extracting an inner part
    /// must round-trip through the pipeline and re-classify (e.g.
    /// `word/document.xml` as Structured XML).
    #[test]
    fn extracted_docx_part_round_trips() {
        let detected = detect::detect(&fixture("sample.docx")).unwrap();
        let extracted = extract(
            &fixture("sample.docx"),
            &detected,
            "word/document.xml",
            &ExtractOptions::default(),
        )
        .unwrap();
        let inner_detected = detect::detect(&extracted.source).unwrap();
        assert!(matches!(
            inner_detected.file_type,
            detect::FileType::Structured(detect::StructuredFormat::Xml)
                | detect::FileType::SourceCode { .. }
        ));
        let text = extracted.source.read_text().unwrap();
        assert!(text.contains("<w:document"), "expected DOCX XML body");
    }

    /// ODT extracts inner ZIP parts through the same archive path as
    /// DOCX. Asking for `content.xml` round-trips and re-classifies as
    /// structured XML.
    #[test]
    fn extracted_odt_part_round_trips() {
        let detected = detect::detect(&fixture("sample.odt")).unwrap();
        let extracted = extract(
            &fixture("sample.odt"),
            &detected,
            "content.xml",
            &ExtractOptions::default(),
        )
        .unwrap();
        let inner_detected = detect::detect(&extracted.source).unwrap();
        assert!(matches!(
            inner_detected.file_type,
            detect::FileType::Structured(detect::StructuredFormat::Xml)
                | detect::FileType::SourceCode { .. }
        ));
        let text = extracted.source.read_text().unwrap();
        assert!(
            text.contains("office:document-content"),
            "expected ODT content.xml body",
        );
    }

    /// RTF embeds (`\pict` / `\object` groups) extract by their
    /// auto-generated name. Asking for an embed that doesn't exist
    /// must surface `NotFound`, not `Unsupported` — RTF has its own
    /// extract path now.
    #[test]
    fn rtf_extract_unknown_embed_is_not_found() {
        let detected = detect::detect(&fixture("sample.rtf")).unwrap();
        let err = extract(
            &fixture("sample.rtf"),
            &detected,
            "image999.jpg",
            &ExtractOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(err, ExtractError::NotFound(_)));
    }
}
