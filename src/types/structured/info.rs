//! Structured-data stats (JSON / YAML / TOML / XML) plus the Format
//! info section. Each parser walks the document once to collect
//! top-level kind + count, max depth, and total node count. XML
//! additionally records the root element name and any namespaces
//! declared on the root.

use crate::info::{
    FileExtras, StructuredStats, TopLevelKind, paint_count, push_field, push_section_header,
};
use crate::input::detect::StructuredFormat;
use crate::theme::PeekTheme;

pub fn format_name(fmt: StructuredFormat) -> &'static str {
    match fmt {
        StructuredFormat::Json => "JSON",
        StructuredFormat::Jsonc => "JSONC",
        StructuredFormat::Json5 => "JSON5",
        StructuredFormat::Jsonl => "JSON Lines",
        StructuredFormat::Yaml => "YAML",
        StructuredFormat::Toml => "TOML",
        StructuredFormat::Xml => "XML",
    }
}

pub fn gather_extras(fmt: StructuredFormat, bytes: &[u8]) -> FileExtras {
    let format_name = format_name(fmt);
    let stats = match std::str::from_utf8(bytes) {
        Ok(s) => match fmt {
            StructuredFormat::Json => json_stats(s),
            StructuredFormat::Jsonc => jsonc_stats(s),
            StructuredFormat::Json5 => json5_stats(s),
            StructuredFormat::Jsonl => jsonl_stats(s),
            StructuredFormat::Yaml => yaml_stats(s),
            StructuredFormat::Toml => toml_stats(s),
            StructuredFormat::Xml => xml_stats(s),
        },
        Err(_) => None,
    };
    FileExtras::Structured { format_name, stats }
}

pub fn render_section(
    lines: &mut Vec<String>,
    format_name: &str,
    stats: Option<&StructuredStats>,
    theme: &PeekTheme,
) {
    lines.push(String::new());
    push_section_header(lines, "Format", theme);
    push_field(lines, "Type", &theme.paint_accent(format_name), theme);
    if let Some(stats) = stats {
        push_structured_stats(lines, stats, theme);
    }
}

fn push_structured_stats(lines: &mut Vec<String>, stats: &StructuredStats, theme: &PeekTheme) {
    let (kind_label, count_label) = match &stats.top_level_kind {
        TopLevelKind::Object => ("Object", "Keys"),
        TopLevelKind::Array => ("Array", "Items"),
        TopLevelKind::Scalar => ("Scalar", "Items"),
        TopLevelKind::Table => ("Table", "Keys"),
        TopLevelKind::MultiDoc(_) => ("Multi-doc", "Top-level"),
        TopLevelKind::Document => ("Document", "Top-level"),
    };
    let kind_text = match &stats.top_level_kind {
        TopLevelKind::MultiDoc(n) => format!("Multi-doc ({n})"),
        _ => kind_label.to_string(),
    };
    push_field(lines, "Top-level", &theme.paint_value(&kind_text), theme);
    if stats.top_level_count > 0 {
        push_field(
            lines,
            count_label,
            &paint_count(stats.top_level_count, theme),
            theme,
        );
    }
    if stats.max_depth > 0 {
        push_field(
            lines,
            "Max Depth",
            &paint_count(stats.max_depth, theme),
            theme,
        );
    }
    if stats.total_nodes > 0 {
        push_field(
            lines,
            "Total Nodes",
            &paint_count(stats.total_nodes, theme),
            theme,
        );
    }
    if let Some(root) = &stats.xml_root {
        push_field(lines, "Root Element", &theme.paint_accent(root), theme);
    }
    if !stats.xml_namespaces.is_empty() {
        for (i, ns) in stats.xml_namespaces.iter().enumerate() {
            let label = if i == 0 { "Namespaces" } else { "" };
            push_field(lines, label, &theme.paint_muted(ns), theme);
        }
    }
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

fn json_stats(s: &str) -> Option<StructuredStats> {
    let value: serde_json::Value = serde_json::from_str(s).ok()?;
    let (kind, count) = match &value {
        serde_json::Value::Object(o) => (TopLevelKind::Object, o.len()),
        serde_json::Value::Array(a) => (TopLevelKind::Array, a.len()),
        _ => (TopLevelKind::Scalar, 0),
    };
    let mut max_depth = 0;
    let mut total_nodes = 0;
    walk_json(&value, 1, &mut max_depth, &mut total_nodes);
    Some(StructuredStats {
        top_level_kind: kind,
        top_level_count: count,
        max_depth,
        total_nodes,
        xml_root: None,
        xml_namespaces: Vec::new(),
    })
}

fn walk_json(v: &serde_json::Value, depth: usize, max_depth: &mut usize, total: &mut usize) {
    *total += 1;
    if depth > *max_depth {
        *max_depth = depth;
    }
    match v {
        serde_json::Value::Object(o) => {
            for (_, val) in o {
                walk_json(val, depth + 1, max_depth, total);
            }
        }
        serde_json::Value::Array(a) => {
            for val in a {
                walk_json(val, depth + 1, max_depth, total);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// JSONC (JSON with `//` + `/* … */` comments)
// ---------------------------------------------------------------------------

fn jsonc_stats(s: &str) -> Option<StructuredStats> {
    let stripped = strip_json_comments(s);
    json_stats(&stripped)
}

/// Remove `//` line comments and `/* … */` block comments outside of
/// string literals. Preserves all other bytes verbatim so JSON parsing
/// after the strip doesn't shift offsets in user-visible ways.
pub fn strip_json_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    let mut in_str = false;
    let mut esc = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            out.push(b as char);
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            in_str = true;
            out.push('"');
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'/' => {
                    i += 2;
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                    continue;
                }
                b'*' => {
                    i += 2;
                    while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                        // Preserve newlines so line-based stats below stay sensible.
                        if bytes[i] == b'\n' {
                            out.push('\n');
                        }
                        i += 1;
                    }
                    if i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i = bytes.len();
                    }
                    continue;
                }
                _ => {}
            }
        }
        out.push(b as char);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// JSON5
// ---------------------------------------------------------------------------

fn json5_stats(s: &str) -> Option<StructuredStats> {
    let value: serde_json::Value = json5::from_str(s).ok()?;
    let (kind, count) = match &value {
        serde_json::Value::Object(o) => (TopLevelKind::Object, o.len()),
        serde_json::Value::Array(a) => (TopLevelKind::Array, a.len()),
        _ => (TopLevelKind::Scalar, 0),
    };
    let mut max_depth = 0;
    let mut total_nodes = 0;
    walk_json(&value, 1, &mut max_depth, &mut total_nodes);
    Some(StructuredStats {
        top_level_kind: kind,
        top_level_count: count,
        max_depth,
        total_nodes,
        xml_root: None,
        xml_namespaces: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// JSON Lines / NDJSON
// ---------------------------------------------------------------------------

fn jsonl_stats(s: &str) -> Option<StructuredStats> {
    let mut docs = 0usize;
    let mut max_depth = 0usize;
    let mut total_nodes = 0usize;
    for line in s.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            docs += 1;
            walk_json(&v, 1, &mut max_depth, &mut total_nodes);
        }
    }
    if docs == 0 {
        return None;
    }
    Some(StructuredStats {
        top_level_kind: TopLevelKind::MultiDoc(docs),
        top_level_count: docs,
        max_depth,
        total_nodes,
        xml_root: None,
        xml_namespaces: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// YAML
// ---------------------------------------------------------------------------

fn yaml_stats(s: &str) -> Option<StructuredStats> {
    use serde::de::Deserialize;
    use serde_yaml::Value;

    // Multi-document support: count `---` separated docs.
    let docs: Vec<Value> = serde_yaml::Deserializer::from_str(s)
        .map(Value::deserialize)
        .filter_map(|r| r.ok())
        .collect();
    if docs.is_empty() {
        return None;
    }
    let (kind, count) = match &docs[0] {
        Value::Mapping(m) => (TopLevelKind::Object, m.len()),
        Value::Sequence(seq) => (TopLevelKind::Array, seq.len()),
        Value::Null => (TopLevelKind::Scalar, 0),
        _ => (TopLevelKind::Scalar, 0),
    };
    let kind = if docs.len() > 1 {
        TopLevelKind::MultiDoc(docs.len())
    } else {
        kind
    };
    let mut max_depth = 0;
    let mut total_nodes = 0;
    for doc in &docs {
        walk_yaml(doc, 1, &mut max_depth, &mut total_nodes);
    }
    Some(StructuredStats {
        top_level_kind: kind,
        top_level_count: count,
        max_depth,
        total_nodes,
        xml_root: None,
        xml_namespaces: Vec::new(),
    })
}

fn walk_yaml(v: &serde_yaml::Value, depth: usize, max_depth: &mut usize, total: &mut usize) {
    use serde_yaml::Value;
    *total += 1;
    if depth > *max_depth {
        *max_depth = depth;
    }
    match v {
        Value::Mapping(m) => {
            for (_, val) in m {
                walk_yaml(val, depth + 1, max_depth, total);
            }
        }
        Value::Sequence(seq) => {
            for val in seq {
                walk_yaml(val, depth + 1, max_depth, total);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// TOML
// ---------------------------------------------------------------------------

fn toml_stats(s: &str) -> Option<StructuredStats> {
    let value: toml::Value = toml::from_str(s).ok()?;
    let (kind, count) = match &value {
        toml::Value::Table(t) => (TopLevelKind::Table, t.len()),
        _ => (TopLevelKind::Scalar, 0),
    };
    let mut max_depth = 0;
    let mut total_nodes = 0;
    walk_toml(&value, 1, &mut max_depth, &mut total_nodes);
    Some(StructuredStats {
        top_level_kind: kind,
        top_level_count: count,
        max_depth,
        total_nodes,
        xml_root: None,
        xml_namespaces: Vec::new(),
    })
}

fn walk_toml(v: &toml::Value, depth: usize, max_depth: &mut usize, total: &mut usize) {
    *total += 1;
    if depth > *max_depth {
        *max_depth = depth;
    }
    match v {
        toml::Value::Table(t) => {
            for (_, val) in t {
                walk_toml(val, depth + 1, max_depth, total);
            }
        }
        toml::Value::Array(a) => {
            for val in a {
                walk_toml(val, depth + 1, max_depth, total);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// XML — lenient parser that tolerates malformed HTML
// ---------------------------------------------------------------------------

fn xml_stats(s: &str) -> Option<StructuredStats> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(s);
    let mut depth: usize = 0;
    let mut max_depth: usize = 0;
    let mut total_nodes: usize = 0;
    let mut top_level_count: usize = 0;
    let mut xml_root: Option<String> = None;
    let mut xml_namespaces: Vec<String> = Vec::new();

    let mut error_count = 0usize;
    loop {
        let event = match reader.read_event() {
            Err(_) => {
                error_count += 1;
                if error_count > 64 {
                    break;
                }
                continue;
            }
            Ok(ev) => ev,
        };
        let (start_like, empty, name_attrs): (bool, bool, Option<_>) = match &event {
            Event::Eof => break,
            Event::Start(e) => (true, false, Some(e.clone())),
            Event::Empty(e) => (true, true, Some(e.clone())),
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ => continue,
        };
        if !start_like {
            continue;
        }
        total_nodes += 1;
        depth += 1;
        if depth > max_depth {
            max_depth = depth;
        }
        if depth == 1
            && let Some(e) = &name_attrs
        {
            let name_bytes = e.name().as_ref().to_vec();
            let name = String::from_utf8_lossy(&name_bytes).into_owned();
            if xml_root.is_none() {
                xml_root = Some(name);
            }
            for attr in e.attributes().with_checks(false).flatten() {
                let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                if key == "xmlns" || key.starts_with("xmlns:") {
                    let val = attr
                        .unescape_value()
                        .map(|v| v.into_owned())
                        .unwrap_or_default();
                    let entry = if key == "xmlns" {
                        val
                    } else {
                        format!("{}={}", &key[6..], val)
                    };
                    if !xml_namespaces.contains(&entry) {
                        xml_namespaces.push(entry);
                    }
                }
            }
        }
        if depth == 2 {
            top_level_count += 1;
        }
        if empty {
            depth = depth.saturating_sub(1);
        }
    }

    Some(StructuredStats {
        top_level_kind: TopLevelKind::Document,
        top_level_count,
        max_depth,
        total_nodes,
        xml_root,
        xml_namespaces,
    })
}
