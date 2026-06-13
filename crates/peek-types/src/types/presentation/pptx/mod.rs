//! PPTX / PPTM / PPSX (Office Open XML presentation).
//!
//! [`package`] opens the ZIP, resolves slide order from
//! `ppt/presentation.xml`, and walks each `ppt/slides/slideN.xml` into a
//! per-slide [`Doc`](crate::types::document::ast::Doc). The shared
//! [`compose`](super::compose) wires the resulting deck into the slide
//! read mode + ZIP TOC; [`info_gather`](super::info_gather) reuses
//! [`package::open`] for the Info counts.

pub mod package;
