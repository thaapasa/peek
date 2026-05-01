//! Structured-data stats (JSON / YAML / TOML / XML). Each parser walks
//! the document once to collect top-level kind + count, max depth, and
//! total node count. XML additionally records the root element name and
//! any namespaces declared on the root.

use super::super::{FileExtras, StructuredStats, TopLevelKind};
use crate::input::detect::StructuredFormat;

pub(super) fn format_name(fmt: StructuredFormat) -> &'static str {
    match fmt {
        StructuredFormat::Json => "JSON",
        StructuredFormat::Yaml => "YAML",
        StructuredFormat::Toml => "TOML",
        StructuredFormat::Xml => "XML",
    }
}

pub(super) fn structured_extras(fmt: StructuredFormat, bytes: &[u8]) -> FileExtras {
    let format_name = format_name(fmt);
    let stats = match std::str::from_utf8(bytes) {
        Ok(s) => match fmt {
            StructuredFormat::Json => json_stats(s),
            StructuredFormat::Yaml => yaml_stats(s),
            StructuredFormat::Toml => toml_stats(s),
            StructuredFormat::Xml => xml_stats(s),
        },
        Err(_) => None,
    };
    FileExtras::Structured { format_name, stats }
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
