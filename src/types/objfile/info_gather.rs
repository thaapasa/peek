//! Object-file info gathering: parse the container header (fat-aware)
//! and capture it into [`ObjectMeta`] — semantic `object` enum values,
//! no display formatting. Parse failures land in `ObjectInfo::err`
//! rather than bubbling up, so the Info view always renders.

use object::Object;

use super::info::{ObjectInfo, ObjectMeta};
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
        Err(e) => return ObjectInfo::err(format!("read failed: {e}")),
    };
    let loaded = match load::load(&bytes) {
        Ok(l) => l,
        Err(e) => return ObjectInfo::err(format!("{e}")),
    };
    let file = &loaded.file;
    let (universal, universal_selected) = match &loaded.fat {
        Some(fat) => (fat.architectures.clone(), fat.selected),
        None => (Vec::new(), 0),
    };
    ObjectInfo::ok(ObjectMeta {
        format: file.format(),
        architecture: file.architecture(),
        kind: file.kind(),
        endianness: file.endianness(),
        is_64: file.is_64(),
        entry: entry_point(file),
        section_count: file.sections().count(),
        symbol_count: file.symbols().count(),
        dynamic_symbol_count: file.dynamic_symbols().count(),
        has_debug_info: file.has_debug_symbols(),
        linked_libraries: super::links::linked_libraries(loaded.data),
        universal,
        universal_selected,
    })
}

/// Entry point, or `None` for relocatable objects (`.o` — no entry).
fn entry_point(file: &object::File<'_>) -> Option<u64> {
    match file.kind() {
        object::ObjectKind::Relocatable => None,
        _ => Some(file.entry()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use object::BinaryFormat;
    use std::path::PathBuf;

    fn fixture(name: &str) -> InputSource {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("test-data");
        p.push(name);
        InputSource::File(p)
    }

    /// A `.wasm` module parses through the same object-file path as ELF /
    /// Mach-O / PE and reports the WebAssembly format. The exported
    /// function surfaces as a symbol.
    #[test]
    fn wasm_module_parses_as_webassembly() {
        let info = gather(&fixture("minimal.wasm"));
        let meta = info.meta.expect("wasm module parses");
        assert_eq!(meta.format, BinaryFormat::Wasm);
        assert!(meta.section_count > 0, "wasm sections are listed");
    }
}
