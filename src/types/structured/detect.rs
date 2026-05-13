//! Extension-based structured-data format detection. Content sniffing
//! (deciding "this is JSON" / "this is YAML" from the bytes themselves)
//! stays in the orchestrator because the JSON/YAML/XML/HTML/SVG
//! family share the same UTF-8 entry path and must be tried in a
//! specific order.

use super::format::StructuredFormat;

/// Map a single file extension to a structured-data format.
pub fn format_from_ext(ext: &str) -> Option<StructuredFormat> {
    match ext {
        "json" | "geojson" => Some(StructuredFormat::Json),
        "jsonc" => Some(StructuredFormat::Jsonc),
        "json5" => Some(StructuredFormat::Json5),
        "jsonl" | "ndjson" => Some(StructuredFormat::Jsonl),
        "yaml" | "yml" => Some(StructuredFormat::Yaml),
        "toml" => Some(StructuredFormat::Toml),
        "xml" | "plist" => Some(StructuredFormat::Xml),
        _ => None,
    }
}
