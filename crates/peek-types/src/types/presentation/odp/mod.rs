//! ODP (OpenDocument Presentation).
//!
//! [`package`] opens the ZIP and walks `content.xml`, segmenting on
//! `<draw:page>` into per-slide [`Doc`](crate::types::document::ast::Doc)
//! values. The shared [`compose`](super::compose) wires the deck into the
//! slide read mode + ZIP TOC.

pub mod package;
