//! Single-pass quick-xml walk: collects byte spans of every animated
//! element + concatenated `<style>` text content for the CSS parser.

use super::spec::{AnimSpec, parse_anim_spec};

pub(super) struct XmlScan {
    pub targets: Vec<RawTarget>,
    pub style_text: String,
}

pub(super) struct RawTarget {
    /// Byte position right after the `>` closing the element's opening
    /// tag — the open marker (later replaced with `<g transform="...">`)
    /// is inserted here.
    pub open_at: usize,
    /// Byte position right before the matching `</tag>` closing tag —
    /// the close marker (replaced with `</g>`) is inserted here.
    pub close_at: usize,
    pub spec: AnimSpec,
}

/// Walk the SVG once with quick-xml, collecting:
/// - Animated elements: any `Start` event whose `style="..."` attribute
///   contains an `animation-*` reference. Records open/close byte spans
///   tracking nesting depth so the matching `</tag>` is found correctly.
/// - `<style>` block text content (used by the CSS keyframe parser).
pub(super) fn scan_svg(text: &str) -> XmlScan {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);

    struct Pending {
        depth: i32,
        open_at: usize,
        spec: AnimSpec,
    }

    let mut depth: i32 = 0;
    let mut pending: Vec<Pending> = Vec::new();
    let mut targets: Vec<RawTarget> = Vec::new();
    let mut style_text = String::new();
    let mut in_style_depth: i32 = 0;

    loop {
        let pos_before = reader.buffer_position() as usize;
        let event = match reader.read_event() {
            Ok(e) => e,
            Err(_) => break,
        };
        let pos_after = reader.buffer_position() as usize;

        match event {
            Event::Start(e) => {
                depth += 1;
                let local = e.local_name();
                if local.as_ref().eq_ignore_ascii_case(b"style") {
                    in_style_depth = depth;
                }
                if let Some(spec) = anim_spec_from_attrs(&e) {
                    pending.push(Pending {
                        depth,
                        open_at: pos_after,
                        spec,
                    });
                }
            }
            Event::End(e) => {
                while let Some(top) = pending.last() {
                    if top.depth == depth {
                        let p = pending.pop().unwrap();
                        targets.push(RawTarget {
                            open_at: p.open_at,
                            close_at: pos_before,
                            spec: p.spec,
                        });
                    } else {
                        break;
                    }
                }
                let local = e.local_name();
                if local.as_ref().eq_ignore_ascii_case(b"style") && depth == in_style_depth {
                    in_style_depth = 0;
                }
                depth -= 1;
            }
            Event::Text(t) if in_style_depth > 0 => {
                if let Ok(s) = t.decode() {
                    style_text.push_str(&s);
                    style_text.push('\n');
                }
            }
            Event::CData(t) if in_style_depth > 0 => {
                if let Ok(s) = std::str::from_utf8(t.as_ref()) {
                    style_text.push_str(s);
                    style_text.push('\n');
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    XmlScan {
        targets,
        style_text,
    }
}

fn anim_spec_from_attrs(e: &quick_xml::events::BytesStart<'_>) -> Option<AnimSpec> {
    for attr in e.attributes().with_checks(false) {
        let attr = attr.ok()?;
        if attr
            .key
            .local_name()
            .as_ref()
            .eq_ignore_ascii_case(b"style")
        {
            let val = std::str::from_utf8(&attr.value).ok()?;
            if let Some(spec) = parse_anim_spec(val) {
                return Some(spec);
            }
        }
    }
    None
}
