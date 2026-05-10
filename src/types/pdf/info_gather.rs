//! Populate [`PdfStats`] for the info section.
//!
//! Surfaces the open error as `error` text on the stats so the info
//! view can show why pdfium couldn't read the file (corrupt header,
//! encrypted, missing library) without crashing.

use crate::info::FileExtras;
use crate::input::InputSource;

use super::info::PdfStats;
use super::package;

pub fn gather_extras(source: &InputSource) -> FileExtras {
    match package::open_doc(source) {
        Ok(doc) => {
            let embeds = doc.list_embeds();
            let attachment_count = embeds
                .iter()
                .filter(|e| e.path.starts_with("attachments/"))
                .count();
            let image_count = embeds
                .iter()
                .filter(|e| e.path.starts_with("pages/"))
                .count();
            let stats = PdfStats {
                metadata: doc.metadata(),
                page_count: doc.page_count(),
                attachment_count,
                image_count,
                encrypted: doc.is_encrypted(),
                pdf_version: doc.pdf_version().to_string(),
                error: None,
            };
            FileExtras::Pdf(stats)
        }
        Err(e) => {
            let mut stats = PdfStats::empty();
            stats.error = Some(format!("{e:#}"));
            FileExtras::Pdf(stats)
        }
    }
}
