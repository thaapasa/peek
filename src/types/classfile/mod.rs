//! Java classfile support (`.class` — JVM bytecode container).
//!
//! Read-only introspection via the `cafebabe` crate. `compose` builds a
//! metadata Info view, Fields and Methods tables (the shared
//! `viewer::table::TableMode`), and a `javap -c`-style Bytecode
//! disassembly view. There is no extract path — fields and methods are
//! not files.

pub mod bytecode;
pub mod bytecode_mode;
pub mod compose;
pub mod descriptor;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod tables;
