//! Structured-data support: JSON / YAML / TOML / XML.
//!
//! `info` collects per-format stats (top-level kind, depth, node count,
//! XML root + namespaces) and renders the Format info section.
//! `pretty` reflows the raw source into pretty-printed form for
//! `ContentMode`'s pretty view.

pub mod info;
pub mod pretty;

use crate::input::detect::{FileType, StructuredFormat};
use crate::viewer::modes::PrettyView;

/// Build the pretty-print branch for a structured-ish source view
/// (`Structured(*)` or `Svg`-as-XML), or `None` when the type has no
/// pretty form or `plain_mode` is set. The returned `PrettyView` carries
/// a closure over this crate's pretty-printer, so the foundation
/// `text_content_mode` stays ignorant of `types::structured` — the
/// reader → foundation direction the crate split needs.
pub fn pretty_view_for(file_type: &FileType, plain_mode: bool) -> Option<PrettyView> {
    if plain_mode {
        return None;
    }
    let fmt = match file_type {
        FileType::Structured(f) => *f,
        FileType::Svg => StructuredFormat::Xml,
        _ => return None,
    };
    Some(PrettyView::new(
        move |raw: &str| pretty::pretty_print(raw, fmt),
        info::format_name(fmt),
    ))
}
