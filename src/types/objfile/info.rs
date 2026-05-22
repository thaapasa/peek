//! Object-file info shape: header-level metadata for ELF / Mach-O /
//! PE / COFF, or a parse-error surface. Populated by `info_gather`,
//! rendered by `info_render`.

/// Header metadata for one object file. On a parse failure `error` is
/// set and the remaining fields stay at neutral defaults — the renderer
/// then shows only the error row.
pub struct ObjectInfo {
    /// User-facing parse-failure reason. `None` on success.
    pub error: Option<String>,
    /// Container format — "ELF", "Mach-O", "PE", "COFF", ...
    pub format: &'static str,
    /// Target architecture — "x86-64", "AArch64", ...
    pub architecture: String,
    /// File kind — "executable", "relocatable object", "dynamic library", ...
    pub kind: &'static str,
    /// "little-endian" / "big-endian".
    pub endianness: &'static str,
    /// 64-bit vs 32-bit container.
    pub is_64: bool,
    /// Program entry point (virtual address). `None` for relocatable
    /// objects — they have no entry point.
    pub entry: Option<u64>,
    pub section_count: usize,
    /// `.symtab` entry count (debug / link symbols; stripped → 0).
    pub symbol_count: usize,
    /// `.dynsym` entry count (symbols resolved at load time).
    pub dynamic_symbol_count: usize,
    /// True when the file carries DWARF / debug sections.
    pub has_debug_info: bool,
    /// Universal (fat) Mach-O slice labels, in container order. Empty
    /// for a plain single-architecture file.
    pub universal: Vec<String>,
    /// Index into `universal` of the slice the views describe.
    pub universal_selected: usize,
}

impl ObjectInfo {
    /// Build an error-only `ObjectInfo` — metadata fields left neutral.
    pub fn error(msg: String) -> Self {
        Self {
            error: Some(msg),
            format: "unknown",
            architecture: String::new(),
            kind: "unknown",
            endianness: "",
            is_64: false,
            entry: None,
            section_count: 0,
            symbol_count: 0,
            dynamic_symbol_count: 0,
            has_debug_info: false,
            universal: Vec::new(),
            universal_selected: 0,
        }
    }
}
