//! Object-file info shape: parsed header metadata for ELF / Mach-O /
//! PE / COFF, or a parse-error surface. Mirrors the `DiskImageInfo`
//! shape — `meta` is `Some` exactly when `error` is `None`.
//!
//! Metadata fields keep the semantic `object` enum values; mapping them
//! to display labels is `info_render`'s job.

use object::{Architecture, BinaryFormat, Endianness, ObjectKind};

/// Which kind of build-identity blob a file carries. The bytes are
/// rendered differently per kind (continuous hex vs. canonical UUID).
#[derive(Debug)]
pub enum BuildIdKind {
    /// ELF `NT_GNU_BUILD_ID` note.
    GnuBuildId,
    /// Mach-O `LC_UUID` load command.
    MachUuid,
    /// PE CodeView PDB GUID.
    PdbGuid,
}

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
    /// Build-identity blob (ELF build ID, Mach-O UUID, PE PDB GUID) when
    /// present, tagged with its source so the renderer can label it.
    pub build_id: Option<(BuildIdKind, Vec<u8>)>,
    /// Shared libraries the file links against (ELF `DT_NEEDED`, Mach-O
    /// dylibs, PE imports), in file order. Empty when statically linked
    /// or for formats we don't walk.
    pub linked_libraries: Vec<String>,
    /// Universal (fat) Mach-O slice architectures, in container order.
    /// Empty for a plain single-architecture file.
    pub universal: Vec<Architecture>,
    /// Index into `universal` of the slice the views describe.
    pub universal_selected: usize,
}
