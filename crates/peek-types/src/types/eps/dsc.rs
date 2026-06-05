//! Document Structuring Convention (DSC) comment parser.
//!
//! PostScript / EPS files carry metadata in `%%`-prefixed header
//! comments (`%%Title:`, `%%Creator:`, `%%BoundingBox:`, …) up to the
//! `%%EndComments` marker. This is a line-prefix scan over the header
//! region — no PostScript execution — surfacing the fields the Info
//! view shows.

/// Fields lifted from the DSC header. Every field is optional — files
/// vary in what they declare.
#[derive(Debug, Clone, Default)]
pub struct DscInfo {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub creation_date: Option<String>,
    pub for_whom: Option<String>,
    pub bounding_box: Option<String>,
    pub language_level: Option<String>,
    pub pages: Option<String>,
}

/// Parse DSC header comments from the start of a PostScript program.
/// Stops at `%%EndComments` or the first body (non-`%`) line, whichever
/// comes first, so the scan stays bounded to the header.
pub fn parse(text: &str) -> DscInfo {
    let mut info = DscInfo::default();
    // Split on any CR / LF: Adobe Illustrator EPS mixes `\r\n` and bare
    // `\r` (old-Mac) line endings within the same header, and
    // `str::lines()` doesn't break on a lone `\r` — it would merge
    // consecutive DSC comments and silently drop fields. A `\r\n` yields
    // an empty fragment here, which the empty-line skip below handles.
    for line in text.split(['\r', '\n']) {
        let line = line.trim_end_matches('\u{0}');
        if line == "%%EndComments" {
            break;
        }
        // DSC keywords are `%%`-prefixed. A bare `%` is a regular
        // comment, anything else is the program body — stop there so a
        // body line like `%!PS` continuation or code doesn't get
        // mis-scanned for the whole file.
        let Some(rest) = line.strip_prefix("%%") else {
            if !line.starts_with('%') && !line.is_empty() {
                break;
            }
            continue;
        };
        let Some((key, value)) = rest.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let slot = match key {
            "Title" => &mut info.title,
            "Creator" => &mut info.creator,
            "CreationDate" => &mut info.creation_date,
            "For" => &mut info.for_whom,
            "BoundingBox" => &mut info.bounding_box,
            "LanguageLevel" => &mut info.language_level,
            "Pages" => &mut info.pages,
            _ => continue,
        };
        // First declaration wins — `%%Pages: (atend)` early plus a real
        // count later is rare; the header value is what DSC intends.
        if slot.is_none() {
            *slot = Some(value.to_string());
        }
    }
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mixed_cr_and_crlf_line_endings() {
        // Adobe Illustrator EPS mixes `\r\n` and bare `\r`. A naive
        // `lines()` would merge the bare-`\r` run and drop `%%For` /
        // anything after it on the same physical line.
        let text = "%!PS-Adobe-3.0 EPSF-3.0\r\n\
                    %%Title: art.eps\r\n\
                    %%Creator: Illustrator\r\
                    %AI_PrivateComment\r\
                    %%For: Someone\r\n\
                    %%BoundingBox: 0 0 100 200\r\n\
                    %%EndComments\r\n\
                    showpage\r\n";
        let d = parse(text);
        assert_eq!(d.title.as_deref(), Some("art.eps"));
        assert_eq!(d.creator.as_deref(), Some("Illustrator"));
        assert_eq!(d.for_whom.as_deref(), Some("Someone"));
        assert_eq!(d.bounding_box.as_deref(), Some("0 0 100 200"));
    }

    #[test]
    fn stops_at_end_comments() {
        let text = "%%Title: a\n%%EndComments\n%%Creator: ignored\n";
        let d = parse(text);
        assert_eq!(d.title.as_deref(), Some("a"));
        assert_eq!(d.creator, None);
    }
}
