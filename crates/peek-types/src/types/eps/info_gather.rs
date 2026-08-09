//! Populate [`EpsInfo`] for the info section: DSC header fields, the
//! embedded-preview descriptor (binary DOS-EPS only), and Ghostscript
//! availability.

use peek_io::InputSource;

use super::PostScriptFormat;
use super::dos_eps::{self, PreviewKind};
use super::dsc;
use super::info::{EpsInfo, PreviewMeta};
use super::{gs, postscript_text};
use crate::info::Extras;

pub fn gather_extras(source: &InputSource, format: PostScriptFormat) -> Extras {
    let bytes = match source.read_bytes(peek_io::limits::Budget::Sidecar("EPS file")) {
        Ok(b) => b,
        Err(_) => {
            return Box::new(EpsInfo {
                format,
                dsc: dsc::DscInfo::default(),
                preview: None,
                gs_available: gs::find().is_some(),
            });
        }
    };

    let header = dos_eps::parse(&bytes);
    let preview = header.as_ref().and_then(|h| {
        let (kind, section) = (h.preview_kind?, h.preview?);
        let raw = &bytes[section.offset..section.offset + section.len];
        // Only TIFF previews decode through the image crate; WMF dims
        // stay unknown.
        let dimensions = match kind {
            PreviewKind::Tiff => image::load_from_memory(raw)
                .ok()
                .map(|img| (img.width(), img.height())),
            PreviewKind::Wmf => None,
        };
        Some(PreviewMeta {
            kind,
            bytes: section.len,
            dimensions,
        })
    });

    // DSC lives in the PostScript section, which for a DOS-EPS is a
    // slice of the file rather than the whole thing.
    let ps = postscript_text(&bytes, header.as_ref());
    let dsc = dsc::parse(&ps);

    Box::new(EpsInfo {
        format,
        dsc,
        preview,
        gs_available: gs::find().is_some(),
    })
}
