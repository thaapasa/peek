//! Structured-data format enum.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredFormat {
    Json,
    /// JSON with comments (VS Code flavor): `//` and `/* … */` allowed.
    Jsonc,
    /// JSON5: comments, unquoted keys, trailing commas, single quotes, hex.
    Json5,
    /// JSON Lines / NDJSON: one JSON value per line.
    Jsonl,
    Yaml,
    Toml,
    Xml,
}
