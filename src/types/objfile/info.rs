//! Object-file info shape: parsed header metadata for ELF / Mach-O /
//! PE / COFF, or a parse-error surface. Mirrors the `DiskImageInfo`
//! shape — `meta` is `Some` exactly when `error` is `None`.
//!
//! Metadata fields keep the semantic `object` enum values; mapping them
//! to display labels is `info_render`'s job.

use object::{Architecture, BinaryFormat, Endianness, ObjectKind};

/// Object-file metadata, or the reason parsing failed.
pub struct ObjectInfo {
    /// Header metadata. `None` when parsing failed.
    pub meta: Option<ObjectMeta>,
    /// User-facing parse-failure reason. `None` on success.
    pub error: Option<String>,
}

impl ObjectInfo {
    pub fn ok(meta: ObjectMeta) -> Self {
        Self {
            meta: Some(meta),
            error: None,
        }
    }

    pub fn err(msg: String) -> Self {
        Self {
            meta: None,
            error: Some(msg),
        }
    }
}

/// Header-level metadata for one parsed object file.
pub struct ObjectMeta {
    pub format: BinaryFormat,
    pub architecture: Architecture,
    pub kind: ObjectKind,
    pub endianness: Endianness,
    /// 64-bit vs 32-bit container.
    pub is_64: bool,
    /// Program entry point. `None` for relocatable objects — they have
    /// no entry point.
    pub entry: Option<u64>,
    pub section_count: usize,
    /// `.symtab` entry count (debug / link symbols; stripped → 0).
    pub symbol_count: usize,
    /// `.dynsym` entry count (symbols resolved at load time).
    pub dynamic_symbol_count: usize,
    /// True when the file carries DWARF / debug sections.
    pub has_debug_info: bool,
    /// Universal (fat) Mach-O slice architectures, in container order.
    /// Empty for a plain single-architecture file.
    pub universal: Vec<Architecture>,
    /// Index into `universal` of the slice the views describe.
    pub universal_selected: usize,
}
