//! Structured-data stats (JSON / YAML / TOML / XML) plus the Format
//! info section. Each parser walks the document once to collect
//! top-level kind + count, max depth, and total node count. XML
//! additionally records the root element name and any namespaces
//! declared on the root.

use peek_detect::StructuredFormat;
use peek_theme::PeekTheme;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::info::{Accent, Extras, InfoNode, paint_count};

pub struct StructuredInfo {
    pub format_name: &'static str,
    pub stats: Option<StructuredStats>,
}

pub struct StructuredStats {
    pub top_level_kind: TopLevelKind,
    pub top_level_count: usize,
    pub max_depth: usize,
    pub total_nodes: usize,
    pub xml_root: Option<String>,
    pub xml_namespaces: Vec<String>,
}

pub enum TopLevelKind {
    Object,
    Array,
    Scalar,
    Table,
    MultiDoc(usize),
    Document,
}

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

pub fn gather_extras(fmt: StructuredFormat, bytes: &[u8]) -> Extras {
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
    Box::new(StructuredInfo { format_name, stats })
}

crate::info_section!(StructuredInfo, StructuredView, "structured");

#[derive(Serialize, crate::info::InfoView)]
#[info(title = "Format")]
struct StructuredView {
    #[info(label = "Type")]
    #[serde(rename = "format")]
    format: Accent,
    #[info(nest)]
    #[serde(flatten)]
    stats: Option<StructuredStatsView>,
}

impl From<&StructuredInfo> for StructuredView {
    fn from(info: &StructuredInfo) -> Self {
        StructuredView {
            format: Accent(info.format_name.to_string()),
            stats: info.stats.as_ref().map(StructuredStatsView::from),
        }
    }
}

/// The parsed-document stats, inlined into the Format block. Top-level kind
/// fixes both the displayed kind text and the (dynamic) count-row label.
struct StructuredStatsView {
    kind_token: &'static str,
    kind_text: String,
    document_count: Option<usize>,
    count_label: &'static str,
    top_level_count: usize,
    max_depth: usize,
    total_nodes: usize,
    xml_root: Option<String>,
    xml_namespaces: Vec<String>,
}

impl From<&StructuredStats> for StructuredStatsView {
    fn from(s: &StructuredStats) -> Self {
        let (kind_label, count_label) = match &s.top_level_kind {
            TopLevelKind::Object => ("Object", "Keys"),
            TopLevelKind::Array => ("Array", "Items"),
            TopLevelKind::Scalar => ("Scalar", "Items"),
            TopLevelKind::Table => ("Table", "Keys"),
            TopLevelKind::MultiDoc(_) => ("Multi-doc", "Top-level"),
            TopLevelKind::Document => ("Document", "Top-level"),
        };
        let kind_text = match &s.top_level_kind {
            TopLevelKind::MultiDoc(n) => format!("Multi-doc ({n})"),
            _ => kind_label.to_string(),
        };
        let document_count = match &s.top_level_kind {
            TopLevelKind::MultiDoc(n) => Some(*n),
            _ => None,
        };
        StructuredStatsView {
            kind_token: top_level_token(&s.top_level_kind),
            kind_text,
            document_count,
            count_label,
            top_level_count: s.top_level_count,
            max_depth: s.max_depth,
            total_nodes: s.total_nodes,
            xml_root: s.xml_root.clone(),
            xml_namespaces: s.xml_namespaces.clone(),
        }
    }
}

impl crate::info::InfoView for StructuredStatsView {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        let mut nodes = vec![InfoNode::Row {
            label: "Top-level".into(),
            value: theme.paint_value(&self.kind_text),
        }];
        if self.top_level_count > 0 {
            nodes.push(InfoNode::Row {
                label: self.count_label.into(),
                value: paint_count(self.top_level_count, theme),
            });
        }
        if self.max_depth > 0 {
            nodes.push(InfoNode::Row {
                label: "Max Depth".into(),
                value: paint_count(self.max_depth, theme),
            });
        }
        if self.total_nodes > 0 {
            nodes.push(InfoNode::Row {
                label: "Total Nodes".into(),
                value: paint_count(self.total_nodes, theme),
            });
        }
        if let Some(root) = &self.xml_root {
            nodes.push(InfoNode::Row {
                label: "Root Element".into(),
                value: theme.paint_accent(root),
            });
        }
        for (i, ns) in self.xml_namespaces.iter().enumerate() {
            nodes.push(InfoNode::Row {
                label: if i == 0 { "Namespaces" } else { "" }.into(),
                value: theme.paint_muted(ns),
            });
        }
        nodes
    }
}

impl Serialize for StructuredStatsView {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut len = 4; // kind, count, depth, nodes
        if self.document_count.is_some() {
            len += 1;
        }
        if self.xml_root.is_some() {
            len += 1;
        }
        if !self.xml_namespaces.is_empty() {
            len += 1;
        }
        let mut st = ser.serialize_struct("structured_stats", len)?;
        st.serialize_field("top_level_kind", self.kind_token)?;
        if let Some(n) = self.document_count {
            st.serialize_field("document_count", &n)?;
        }
        st.serialize_field("top_level_count", &self.top_level_count)?;
        st.serialize_field("max_depth", &self.max_depth)?;
        st.serialize_field("total_nodes", &self.total_nodes)?;
        if let Some(root) = &self.xml_root {
            st.serialize_field("xml_root", root)?;
        }
        if !self.xml_namespaces.is_empty() {
            st.serialize_field("xml_namespaces", &self.xml_namespaces)?;
        }
        st.end()
    }
}

fn top_level_token(kind: &TopLevelKind) -> &'static str {
    match kind {
        TopLevelKind::Object => "object",
        TopLevelKind::Array => "array",
        TopLevelKind::Scalar => "scalar",
        TopLevelKind::Table => "table",
        TopLevelKind::MultiDoc(_) => "multi-doc",
        TopLevelKind::Document => "document",
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

/// Structural-nesting ceiling peek will hand to the JSON5 parser. The
/// `json5` crate recurses once per `[`/`{` with no internal bound (unlike
/// `serde_json`, which caps at 128), so a deeply-nested document overflows
/// the thread stack and *aborts the process* (`SIGABRT`, uncatchable). We
/// pre-scan and refuse past this bound — matched to serde_json's limit so
/// JSON and JSON5 reject the same depth bomb identically.
pub const JSON5_MAX_DEPTH: usize = 128;

/// Whether `src` nests `[`/`{` deeper than [`JSON5_MAX_DEPTH`]. Scans
/// outside string literals (single- or double-quoted, JSON5-style) and
/// `//` / `/* … */` comments so brackets inside those don't inflate the
/// count. The stack-overflow guard every `json5::from_str` call runs first.
pub fn json5_nesting_too_deep(src: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    let mut in_str: Option<u8> = None; // Some(quote byte) while inside a string
    let mut esc = false;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' => in_str = Some(b),
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            b'[' | b'{' => {
                depth += 1;
                if depth > JSON5_MAX_DEPTH {
                    return true;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    false
}

fn json5_stats(s: &str) -> Option<StructuredStats> {
    // Guard the process before json5's unbounded recursion (see
    // `json5_nesting_too_deep`); over-deep input degrades to "no stats".
    if json5_nesting_too_deep(s) {
        return None;
    }
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
                    let val = crate::xml::unescape_attr_value(&attr).unwrap_or_default();
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

#[cfg(test)]
mod json_tests {
    use super::*;

    #[test]
    fn multidoc_token_and_document_count() {
        let info = StructuredInfo {
            format_name: "YAML",
            stats: Some(StructuredStats {
                top_level_kind: TopLevelKind::MultiDoc(3),
                top_level_count: 3,
                max_depth: 2,
                total_nodes: 9,
                xml_root: None,
                xml_namespaces: Vec::new(),
            }),
        };
        let (key, v) = json_section(&info);
        assert_eq!(key, "structured");
        assert_eq!(v["format"], serde_json::json!("YAML"));
        assert_eq!(v["top_level_kind"], serde_json::json!("multi-doc"));
        assert_eq!(v["document_count"], serde_json::json!(3));
        // XML-only fields stay absent for non-XML input.
        assert!(v.get("xml_root").is_none());
        assert!(v.get("xml_namespaces").is_none());
    }

    #[test]
    fn unparsed_document_yields_only_format() {
        let info = StructuredInfo {
            format_name: "JSON",
            stats: None,
        };
        let (_, v) = json_section(&info);
        assert_eq!(v["format"], serde_json::json!("JSON"));
        assert!(v.get("top_level_kind").is_none());
    }
}
