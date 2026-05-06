//! CSS `@keyframes` rule parser. Produces per-rule stop lists with an
//! optional `transform` value plus arbitrary CSS property changes
//! (`r`, `cx`, `opacity`, `stroke-width`, ...). Numeric values keep
//! their unit so the timeline sampler can interpolate; non-numeric
//! values are kept as raw strings and step through stops.

use std::collections::HashMap;

use super::util::{find_matching_brace, find_substr, parse_length, skip_ws};

#[derive(Clone)]
pub(super) struct KeyframeStop {
    pub percent: f64,
    pub transform: Option<TransformValue>,
    pub props: Vec<PropChange>,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct TransformValue {
    pub tx: f64,
    pub ty: f64,
}

#[derive(Clone)]
pub(super) struct PropChange {
    pub name: String,
    pub value: PropValue,
}

#[derive(Clone)]
pub(super) enum PropValue {
    Numeric { value: f64, unit: String },
    Raw(String),
}

impl PropValue {
    pub fn render(&self) -> String {
        match self {
            PropValue::Numeric { value, unit } => format!("{}{}", fmt_num(*value), unit),
            PropValue::Raw(s) => s.clone(),
        }
    }
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{:.0}", n)
    } else {
        format!("{:.4}", n)
    }
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
        let (transform, props) = parse_decls(decls);
        for p in percents {
            out.push(KeyframeStop {
                percent: p,
                transform,
                props: props.clone(),
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

/// Walk `prop:value;` pairs; pull `transform:` into a [`TransformValue`]
/// and capture every other declaration as a [`PropChange`]. `animation-*`
/// declarations (e.g. per-stop `animation-timing-function`) are ignored
/// — peek doesn't model per-stop timing curves.
fn parse_decls(decls: &str) -> (Option<TransformValue>, Vec<PropChange>) {
    let mut transform = None;
    let mut props: Vec<PropChange> = Vec::new();
    for chunk in decls.split(';') {
        let Some(colon) = chunk.find(':') else {
            continue;
        };
        let name = chunk[..colon].trim();
        let value = chunk[colon + 1..].trim();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        if name.eq_ignore_ascii_case("transform") {
            transform = parse_transform_value(value);
            continue;
        }
        if name.to_ascii_lowercase().starts_with("animation-") {
            continue;
        }
        props.push(PropChange {
            name: name.to_string(),
            value: parse_prop_value(value),
        });
    }
    (transform, props)
}

fn parse_prop_value(v: &str) -> PropValue {
    let v = v.trim();
    let split = v
        .find(|c: char| {
            !(c == '-' || c == '+' || c == '.' || c.is_ascii_digit() || c == 'e' || c == 'E')
        })
        .unwrap_or(v.len());
    if split > 0
        && let Ok(num) = v[..split].parse::<f64>()
    {
        let unit = v[split..].trim().to_string();
        return PropValue::Numeric { value: num, unit };
    }
    PropValue::Raw(v.to_string())
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
