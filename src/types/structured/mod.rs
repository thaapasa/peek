//! Structured-data support: JSON / YAML / TOML / XML.
//!
//! `info` collects per-format stats (top-level kind, depth, node count,
//! XML root + namespaces) and renders the Format info section.
//! `pretty` reflows the raw source into pretty-printed form for
//! `ContentMode`'s pretty view.

pub mod detect;
pub mod format;
pub mod info;
pub mod pretty;
