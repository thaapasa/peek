//! SQL info: statement counts (DDL/DML/DQL/TCL), created-object inventory,
//! comment ratio, and a heuristic dialect guess. Sidecar to plain text
//! stats — the source is still rendered as syntect-highlighted SQL.

pub mod info_gather;
pub mod info_render;
