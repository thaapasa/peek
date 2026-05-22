//! CSS info: rule / selector / at-rule counts, an `@import` inventory,
//! and a deduped colour palette. Used as a sidecar to the standard text
//! stats — the source is still rendered as syntax-highlighted CSS in
//! `ContentMode`.

pub mod info;
pub mod info_gather;
pub mod info_render;
