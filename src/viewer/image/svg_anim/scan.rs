//! Single-pass quick-xml walk: collects byte spans + class/id/style
//! attributes for every potentially-animated element + the
//! concatenated `<style>` text content for the CSS parsers.
//!
//! Resolution of element → animation spec happens in
//! [`super::parse_text`], which combines selector-matched rule decls
//! (from [`super::selectors`]) with each element's inline `style=`
//! attribute. Scanning therefore captures any element that *could* be
//! a target — has a `class`, an `id`, or inline `animation*` style —
//! and lets the resolver weed out non-matches.

pub(super) struct XmlScan {
    pub elements: Vec<RawElement>,
    pub style_text: String,
}

pub(super) struct RawElement {
    /// Byte position of the `<` that opens the element. Outer marker
    /// for transform wrapping is inserted here.
    pub open_at: usize,
    /// Byte position right after the element's closing tag (or `/>`
    /// for self-closing elements). Outer close marker is inserted here.
    pub close_at: usize,
    /// Byte position right after the `>` (or `/>`) that closes the
    /// element's opening tag — `text[open_at..open_tag_end]` is the
    /// opening-tag slice. Marker phase rewrites this slice when the
    /// target carries property animations.
    pub open_tag_end: usize,
    pub tag: String,
    pub classes: Vec<String>,
    pub id: Option<String>,
    pub inline_style: Option<String>,
}

/// Walk the SVG once with quick-xml, collecting:
/// - Candidate elements: any `Start` or `Empty` event with a `class`,
///   `id`, or inline `style="…animation…"` attribute. Records open/close
///   byte spans tracking nesting depth so the matching `</tag>` is
///   found correctly.
/// - `<style>` block text content (used by the CSS keyframe + rule
///   parsers).
pub(super) fn scan_svg(text: &str) -> XmlScan {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);

    struct Pending {
        depth: i32,
        open_at: usize,
        open_tag_end: usize,
        tag: String,
        classes: Vec<String>,
        id: Option<String>,
        inline_style: Option<String>,
    }

    let mut depth: i32 = 0;
    let mut pending: Vec<Pending> = Vec::new();
    let mut elements: Vec<RawElement> = Vec::new();
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
                let tag = std::str::from_utf8(e.local_name().as_ref())
                    .unwrap_or("")
                    .to_string();
                if tag.eq_ignore_ascii_case("style") {
                    in_style_depth = depth;
                }
                let attrs = read_target_attrs(&e);
                if attrs.is_candidate() {
                    pending.push(Pending {
                        depth,
                        open_at: pos_before,
                        open_tag_end: pos_after,
                        tag,
                        classes: attrs.classes,
                        id: attrs.id,
                        inline_style: attrs.style,
                    });
                }
            }
            Event::Empty(e) => {
                let tag = std::str::from_utf8(e.local_name().as_ref())
                    .unwrap_or("")
                    .to_string();
                let attrs = read_target_attrs(&e);
                if attrs.is_candidate() {
                    elements.push(RawElement {
                        open_at: pos_before,
                        close_at: pos_after,
                        open_tag_end: pos_after,
                        tag,
                        classes: attrs.classes,
                        id: attrs.id,
                        inline_style: attrs.style,
                    });
                }
            }
            Event::End(e) => {
                while let Some(top) = pending.last() {
                    if top.depth == depth {
                        let p = pending.pop().unwrap();
                        elements.push(RawElement {
                            open_at: p.open_at,
                            close_at: pos_after,
                            open_tag_end: p.open_tag_end,
                            tag: p.tag,
                            classes: p.classes,
                            id: p.id,
                            inline_style: p.inline_style,
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
        elements,
        style_text,
    }
}

struct TargetAttrs {
    classes: Vec<String>,
    id: Option<String>,
    style: Option<String>,
}

impl TargetAttrs {
    fn is_candidate(&self) -> bool {
        !self.classes.is_empty()
            || self.id.is_some()
            || self.style.as_ref().is_some_and(|s| s.contains("animation"))
    }
}

fn read_target_attrs(e: &quick_xml::events::BytesStart<'_>) -> TargetAttrs {
    let mut classes = Vec::new();
    let mut id = None;
    let mut style = None;
    for attr in e.attributes().with_checks(false).flatten() {
        let key = attr.key.local_name();
        let key_bytes = key.as_ref();
        let Ok(value) = std::str::from_utf8(&attr.value) else {
            continue;
        };
        if key_bytes.eq_ignore_ascii_case(b"class") {
            classes = value.split_whitespace().map(|s| s.to_string()).collect();
        } else if key_bytes.eq_ignore_ascii_case(b"id") {
            id = Some(value.to_string());
        } else if key_bytes.eq_ignore_ascii_case(b"style") {
            style = Some(value.to_string());
        }
    }
    TargetAttrs { classes, id, style }
}
