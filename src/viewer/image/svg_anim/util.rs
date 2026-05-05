//! Small string + SVG-attribute helpers shared across `svg_anim`
//! submodules.

pub(super) fn skip_ws(s: &str, mut i: usize) -> usize {
    let bytes = s.as_bytes();
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    i
}

pub(super) fn find_substr(haystack: &str, from: usize, needle: &str) -> Option<usize> {
    haystack[from..].find(needle).map(|r| from + r)
}

pub(super) fn find_matching_brace(s: &str, body_start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 1i32;
    let mut i = body_start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

pub(super) fn parse_length(s: &str) -> Option<f64> {
    let t = s.trim();
    let num_end = t
        .find(|c: char| !(c == '-' || c == '+' || c == '.' || c.is_ascii_digit() || c == 'e'))
        .unwrap_or(t.len());
    let num = &t[..num_end];
    num.parse::<f64>().ok()
}

pub(super) fn root_svg_dimensions(text: &str) -> Option<(u32, u32)> {
    let open = text.find("<svg")?;
    let after = &text[open..];
    let close = after.find('>')?;
    let header = &after[..close];
    let w = attr_value(header, "width").and_then(|s| parse_length(&s).map(|f| f as u32));
    let h = attr_value(header, "height").and_then(|s| parse_length(&s).map(|f| f as u32));
    Some((w.unwrap_or(0).max(1), h.unwrap_or(0).max(1)))
}

fn attr_value(header: &str, name: &str) -> Option<String> {
    let needle = format!(" {name}=");
    let pos = header.find(&needle)?;
    let after = &header[pos + needle.len()..];
    let q = after.chars().next()?;
    if q != '"' && q != '\'' {
        return None;
    }
    let body = &after[1..];
    let close = body.find(q)?;
    Some(body[..close].to_string())
}
