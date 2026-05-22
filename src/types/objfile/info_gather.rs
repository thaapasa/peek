//! Object-file info gathering: parse the container header (fat-aware)
//! and project it onto [`ObjectInfo`]. Parse failures land in the
//! `error` field rather than bubbling up — the Info view always renders.

use object::Object;

use super::info::ObjectInfo;
use super::load;
use crate::info::FileExtras;
use crate::input::InputSource;

pub fn gather_extras(source: &InputSource) -> FileExtras {
    FileExtras::ObjectFile(gather(source))
}

fn gather(source: &InputSource) -> ObjectInfo {
    // `object` parses over a byte slice, so the whole file is read.
    // Object files are rarely multi-GB; streaming isn't an option here
    // because symbol / section tables need random access.
    let bytes = match source.read_bytes() {
        Ok(b) => b,
        Err(e) => return ObjectInfo::error(format!("read failed: {e}")),
    };
    let loaded = match load::load(&bytes) {
        Ok(l) => l,
        Err(e) => return ObjectInfo::error(format!("{e}")),
    };
    let file = &loaded.file;
    let (universal, universal_selected) = match &loaded.fat {
        Some(fat) => (fat.architectures.clone(), fat.selected),
        None => (Vec::new(), 0),
    };
    ObjectInfo {
        error: None,
        format: format_label(file.format()),
        architecture: load::arch_label(file.architecture()),
        kind: kind_label(file.kind()),
        endianness: if file.is_little_endian() {
            "little-endian"
        } else {
            "big-endian"
        },
        is_64: file.is_64(),
        entry: entry_point(file),
        section_count: file.sections().count(),
        symbol_count: file.symbols().count(),
        dynamic_symbol_count: file.dynamic_symbols().count(),
        has_debug_info: file.has_debug_symbols(),
        universal,
        universal_selected,
    }
}

/// Entry point, or `None` for relocatable objects (`.o` — no entry).
fn entry_point(file: &object::File<'_>) -> Option<u64> {
    match file.kind() {
        object::ObjectKind::Relocatable => None,
        _ => Some(file.entry()),
    }
}

fn format_label(f: object::BinaryFormat) -> &'static str {
    use object::BinaryFormat;
    match f {
        BinaryFormat::Coff => "COFF",
        BinaryFormat::Elf => "ELF",
        BinaryFormat::MachO => "Mach-O",
        BinaryFormat::Pe => "PE",
        BinaryFormat::Wasm => "WebAssembly",
        BinaryFormat::Xcoff => "XCOFF",
        _ => "unknown",
    }
}

fn kind_label(k: object::ObjectKind) -> &'static str {
    use object::ObjectKind;
    match k {
        ObjectKind::Relocatable => "relocatable object",
        ObjectKind::Executable => "executable",
        ObjectKind::Dynamic => "dynamic library",
        ObjectKind::Core => "core dump",
        _ => "unknown",
    }
}
