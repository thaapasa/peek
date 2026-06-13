//! Presentation format (PPTX / ODP / Keynote) — the missing leg of the
//! office trio (Word = `document`, Excel = `spreadsheet`, slides here).
//!
//! Detection mirrors the other OOXML / ODF containers: PPTX and ODP
//! magic-detect as `application/zip`, so the extension is what routes
//! them; a magic-only zip with no presentation extension falls through
//! to the archive viewer.
//!
//! Keynote `.key` is the exception. Its extension collides with PEM
//! private keys (`cert`), so the bare extension can't route it — the
//! orchestrator ([`crate::detect`]) only claims a `.key` for Keynote
//! when the head also carries zip magic, disambiguating the Apple iWork
//! package from a text key. See [`is_keynote_ext`].

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationFormat {
    /// Office Open XML presentation (PowerPoint).
    Pptx,
    /// Macro-enabled OOXML presentation (same on-disk format).
    Pptm,
    /// OOXML slideshow (`.ppsx` — opens straight into the deck).
    Ppsx,
    /// OpenDocument Presentation.
    Odp,
    /// Apple Keynote (`.key` iWork package). Slide text is undocumented
    /// snappy-protobuf; peek surfaces the embedded preview + metadata.
    Key,
}

impl PresentationFormat {
    /// Human label for the Info section header.
    pub fn label(self) -> &'static str {
        match self {
            PresentationFormat::Pptx => "PowerPoint",
            PresentationFormat::Pptm => "PowerPoint (macro-enabled)",
            PresentationFormat::Ppsx => "PowerPoint slideshow",
            PresentationFormat::Odp => "OpenDocument Presentation",
            PresentationFormat::Key => "Keynote",
        }
    }

    /// Whether this is an OOXML (PowerPoint) container — `ppt/slides/*`
    /// XML vs ODP's `content.xml`.
    pub fn is_ooxml(self) -> bool {
        matches!(
            self,
            PresentationFormat::Pptx | PresentationFormat::Pptm | PresentationFormat::Ppsx
        )
    }
}

/// Map a lowercase extension to a presentation format. Does **not**
/// claim `.key` — that extension collides with PEM keys and is routed
/// by [`is_keynote_ext`] only when the head is zip-shaped.
pub fn format_from_ext(ext: &str) -> Option<PresentationFormat> {
    match ext {
        "pptx" => Some(PresentationFormat::Pptx),
        "pptm" => Some(PresentationFormat::Pptm),
        "ppsx" => Some(PresentationFormat::Ppsx),
        "odp" => Some(PresentationFormat::Odp),
        _ => None,
    }
}

/// Whether an extension is a Keynote candidate. Kept separate from
/// [`format_from_ext`] because `.key` is ambiguous with PEM private
/// keys: the orchestrator only treats it as Keynote when the head also
/// carries zip magic (the iWork package is a zip).
pub fn is_keynote_ext(ext: &str) -> bool {
    matches!(ext, "key" | "keynote")
}

/// Map an IANA MIME to a presentation format. Used for the
/// extension-mismatch allow-list and any future content sniff.
pub fn format_from_mime(mime: &str) -> Option<PresentationFormat> {
    match mime {
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
            Some(PresentationFormat::Pptx)
        }
        "application/vnd.ms-powerpoint.presentation.macroenabled.12" => {
            Some(PresentationFormat::Pptm)
        }
        "application/vnd.openxmlformats-officedocument.presentationml.slideshow" => {
            Some(PresentationFormat::Ppsx)
        }
        "application/vnd.oasis.opendocument.presentation" => Some(PresentationFormat::Odp),
        "application/vnd.apple.keynote" => Some(PresentationFormat::Key),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_routes_ooxml_and_odf_but_not_key() {
        assert_eq!(format_from_ext("pptx"), Some(PresentationFormat::Pptx));
        assert_eq!(format_from_ext("pptm"), Some(PresentationFormat::Pptm));
        assert_eq!(format_from_ext("ppsx"), Some(PresentationFormat::Ppsx));
        assert_eq!(format_from_ext("odp"), Some(PresentationFormat::Odp));
        // `.key` is claimed by the head-aware Keynote path, not here.
        assert_eq!(format_from_ext("key"), None);
        assert_eq!(format_from_ext("txt"), None);
    }

    #[test]
    fn keynote_ext_recognised() {
        assert!(is_keynote_ext("key"));
        assert!(is_keynote_ext("keynote"));
        assert!(!is_keynote_ext("pptx"));
    }

    #[test]
    fn mime_round_trips() {
        assert_eq!(
            format_from_mime("application/vnd.apple.keynote"),
            Some(PresentationFormat::Key)
        );
        assert_eq!(
            format_from_mime("application/vnd.oasis.opendocument.presentation"),
            Some(PresentationFormat::Odp)
        );
    }
}
