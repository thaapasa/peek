//! Java classfile support (`.class` — JVM bytecode container).
//!
//! Read-only introspection via the `cafebabe` crate. `compose` builds a
//! metadata Info view plus Fields and Methods tables (the shared
//! `viewer::table::TableMode`). Bytecode disassembly is out of scope for
//! v1. There is no extract path — fields and methods are not files.

pub mod compose;
pub mod descriptor;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod tables;
