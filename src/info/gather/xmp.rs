//! XMP packet scraping for raster images. We don't pull in a full XMP
//! parser — just locate the `<x:xmpmeta>` block in the leading bytes of
//! the file and substring-match a handful of well-known Dublin Core / xmp
//! tag names.

pub(super) fn xmp_fields_from_bytes(data: &[u8]) -> Vec<(String, String)> {
    // XMP packet is wrapped in `<?xpacket begin="..." ...?>` markers, with
    // `<x:xmpmeta>` as the root element. Find the packet boundaries; bail
    // if not present.
    let needle_start = b"<x:xmpmeta";
    let needle_end = b"</x:xmpmeta>";
    let start = match data.windows(needle_start.len()).position(|w| w == needle_start) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let end = match data[start..]
        .windows(needle_end.len())
        .position(|w| w == needle_end)
    {
        Some(p) => start + p + needle_end.len(),
        None => return Vec::new(),
    };
    let packet = match std::str::from_utf8(&data[start..end]) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    // Tags to pull out. `(label, [candidate XML element names])` — XMP uses
    // namespaced names; we scan for any of the candidates as substrings.
    const TAGS: &[(&str, &[&str])] = &[
        ("Title", &["dc:title"]),
        ("Subject", &["dc:subject"]),
        ("Description", &["dc:description"]),
        ("Creator", &["dc:creator"]),
        ("Rights", &["dc:rights"]),
        ("Rating", &["xmp:Rating", "MicrosoftPhoto:Rating"]),
        ("Label", &["xmp:Label"]),
        ("Keywords", &["dc:subject"]),
    ];

    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (label, candidates) in TAGS {
        if !seen.insert(*label) {
            continue;
        }
        for tag in *candidates {
            if let Some(value) = extract_tag(packet, tag) {
                let value = value.trim();
                if !value.is_empty() {
                    result.push((label.to_string(), value.to_string()));
                    break;
                }
            }
        }
    }
    result
}

/// Pull the inner text of an XMP element, joining `rdf:li` items with
/// commas (XMP often stores text in `<rdf:Alt>` or `<rdf:Bag>` containers).
fn extract_tag(packet: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = packet.find(&open)?;
    let after_open = &packet[start..];
    // Skip to end of opening tag
    let close_bracket = after_open.find('>')?;
    let body_start = start + close_bracket + 1;
    let close = format!("</{tag}>");
    let close_at = packet[body_start..].find(&close)?;
    let inner = &packet[body_start..body_start + close_at];

    // Collect rdf:li items if present, otherwise return inner text.
    let mut items = Vec::new();
    let mut cursor = inner;
    while let Some(li_start) = cursor.find("<rdf:li") {
        let after = &cursor[li_start..];
        let Some(open_end) = after.find('>') else {
            break;
        };
        // Self-closing `<rdf:li/>` — no content; skip past it.
        if after.as_bytes().get(open_end.saturating_sub(1)) == Some(&b'/') {
            cursor = &cursor[li_start + open_end + 1..];
            continue;
        }
        let item_start = li_start + open_end + 1;
        let Some(item_end) = cursor[item_start..].find("</rdf:li>") else {
            break;
        };
        let text = &cursor[item_start..item_start + item_end];
        if !text.trim().is_empty() {
            items.push(text.trim().to_string());
        }
        cursor = &cursor[item_start + item_end + "</rdf:li>".len()..];
    }
    if !items.is_empty() {
        return Some(items.join(", "));
    }
    // Inner is just an empty `<rdf:Alt>` / `<rdf:Bag>` / `<rdf:Seq>` shell
    // — treat as empty so the field gets dropped by the caller.
    if inner.contains("<rdf:") {
        return None;
    }
    Some(inner.trim().to_string())
}
