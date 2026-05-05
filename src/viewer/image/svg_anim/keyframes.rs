//! CSS `@keyframes` rule parser. Produces per-rule stop lists with
//! optional transform values; understands `translateX`, `translateY`,
//! `translate`.

use std::collections::HashMap;

use super::util::{find_matching_brace, find_substr, parse_length, skip_ws};

#[derive(Clone)]
pub(super) struct KeyframeStop {
    pub percent: f64,
    pub transform: Option<TransformValue>,
}

#[derive(Clone, Copy)]
pub(super) struct TransformValue {
    pub tx: f64,
    pub ty: f64,
}

pub(super) fn parse_keyframes(css: &str) -> HashMap<String, Vec<KeyframeStop>> {
    let mut out = HashMap::new();
    let bytes = css.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let Some(at) = find_substr(css, i, "@keyframes") else {
            break;
        };
        let name_start = skip_ws(css, at + "@keyframes".len());
        let name_end = name_start
            + css[name_start..]
                .find(|c: char| c.is_whitespace() || c == '{')
                .unwrap_or(0);
        let name = css[name_start..name_end].trim().to_string();
        let Some(brace_rel) = css[name_end..].find('{') else {
            break;
        };
        let body_start = name_end + brace_rel + 1;
        let Some(body_end) = find_matching_brace(css, body_start) else {
            break;
        };
        let body = &css[body_start..body_end];
        let stops = parse_keyframe_stops(body);
        if !name.is_empty() && !stops.is_empty() {
            out.insert(name, stops);
        }
        i = body_end + 1;
    }
    out
}

fn parse_keyframe_stops(body: &str) -> Vec<KeyframeStop> {
    let mut out = Vec::new();
    let mut cursor = 0;
    let bytes = body.as_bytes();
    while cursor < bytes.len() {
        cursor = skip_ws(body, cursor);
        if cursor >= bytes.len() {
            break;
        }
        // Read percentage list (comma-separated): `0%`, `50%`, `from`, `to`.
        let mut percents: Vec<f64> = Vec::new();
        loop {
            cursor = skip_ws(body, cursor);
            let token_start = cursor;
            while cursor < bytes.len() {
                let c = bytes[cursor] as char;
                if c == '%' || c == '{' || c == ',' || c.is_whitespace() {
                    break;
                }
                cursor += 1;
            }
            let token = body[token_start..cursor].trim();
            let pct = parse_percent_token(token);
            if cursor < bytes.len() && bytes[cursor] == b'%' {
                cursor += 1;
            }
            if let Some(p) = pct {
                percents.push(p);
            }
            cursor = skip_ws(body, cursor);
            if cursor < bytes.len() && bytes[cursor] == b',' {
                cursor += 1;
                continue;
            }
            break;
        }
        cursor = skip_ws(body, cursor);
        if cursor >= bytes.len() || bytes[cursor] != b'{' {
            // Skip ahead to next `}` to recover; degenerate input.
            if let Some(rel) = body[cursor..].find('}') {
                cursor += rel + 1;
                continue;
            }
            break;
        }
        cursor += 1;
        let Some(close_rel) = body[cursor..].find('}') else {
            break;
        };
        let close = cursor + close_rel;
        let decls = &body[cursor..close];
        let transform = parse_transform_decl(decls);
        for p in percents {
            out.push(KeyframeStop {
                percent: p,
                transform,
            });
        }
        cursor = close + 1;
    }
    out.sort_by(|a, b| a.percent.partial_cmp(&b.percent).unwrap());
    out
}

fn parse_percent_token(tok: &str) -> Option<f64> {
    if tok.is_empty() {
        return None;
    }
    if tok.eq_ignore_ascii_case("from") {
        return Some(0.0);
    }
    if tok.eq_ignore_ascii_case("to") {
        return Some(100.0);
    }
    tok.parse::<f64>().ok()
}

fn parse_transform_decl(decls: &str) -> Option<TransformValue> {
    // Walk `prop:value;` pairs; only `transform:` is consumed.
    for chunk in decls.split(';') {
        let Some(colon) = chunk.find(':') else {
            continue;
        };
        let prop = chunk[..colon].trim();
        if !prop.eq_ignore_ascii_case("transform") {
            continue;
        }
        let value = chunk[colon + 1..].trim();
        return parse_transform_value(value);
    }
    None
}

fn parse_transform_value(value: &str) -> Option<TransformValue> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    if let Some(rest) = strip_fn(v, "translateX") {
        return Some(TransformValue {
            tx: parse_length(rest)?,
            ty: 0.0,
        });
    }
    if let Some(rest) = strip_fn(v, "translateY") {
        return Some(TransformValue {
            tx: 0.0,
            ty: parse_length(rest)?,
        });
    }
    if let Some(rest) = strip_fn(v, "translate") {
        let mut parts = rest.split(',').map(str::trim);
        let tx = parse_length(parts.next().unwrap_or(""))?;
        let ty = parts
            .next()
            .map(parse_length)
            .and_then(|o| o)
            .unwrap_or(0.0);
        return Some(TransformValue { tx, ty });
    }
    None
}

fn strip_fn<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let v = value.trim();
    if !v.starts_with(name) {
        return None;
    }
    let rest = v[name.len()..].trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let rest = &rest[1..];
    let close = rest.rfind(')')?;
    Some(rest[..close].trim())
}
