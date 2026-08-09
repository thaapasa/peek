//! Linked-library extraction for object files.
//!
//! The unified `object::Object::imports()` reports imported *symbols*,
//! not the shared-library dependency list (for ELF its `library` field
//! is only the symbol-version file). So each format is walked directly:
//! ELF `DT_NEEDED` entries, Mach-O dylib load commands, and — where the
//! unified API does carry library names — the PE/COFF import table.

use object::read::elf::{Dyn, FileHeader};
use object::read::macho::MachHeader;
use object::{Endianness, FileKind, Object, elf};

/// Shared libraries this object links against, in file order. Empty for
/// statically-linked or relocatable inputs, and for any container whose
/// dependency list we don't walk (e.g. WebAssembly).
pub fn linked_libraries(data: &[u8]) -> Vec<String> {
    match FileKind::parse(data) {
        Ok(FileKind::Elf32) => elf_needed::<elf::FileHeader32<Endianness>>(data),
        Ok(FileKind::Elf64) => elf_needed::<elf::FileHeader64<Endianness>>(data),
        Ok(FileKind::MachO32) => macho_dylibs::<object::macho::MachHeader32<Endianness>>(data),
        Ok(FileKind::MachO64) => macho_dylibs::<object::macho::MachHeader64<Endianness>>(data),
        Ok(FileKind::Pe32 | FileKind::Pe64 | FileKind::Coff) => pe_imports(data),
        _ => Vec::new(),
    }
}

/// ELF `DT_NEEDED` sonames, resolved against the dynamic string table.
fn elf_needed<Elf: FileHeader<Endian = Endianness>>(data: &[u8]) -> Vec<String> {
    let Ok(header) = Elf::parse(data) else {
        return Vec::new();
    };
    let Ok(endian) = header.endian() else {
        return Vec::new();
    };
    let Ok(sections) = header.sections(endian, data) else {
        return Vec::new();
    };
    let Ok(Some((entries, str_index))) = sections.dynamic(endian, data) else {
        return Vec::new();
    };
    let strings = sections
        .strings(endian, data, str_index)
        .unwrap_or_default();
    entries
        .iter()
        // object 0.39 widened the `DT_*` constants to `i64`; `tag32`
        // still yields `Option<i32>`, so narrow the constant to compare.
        .filter(|d| d.tag32(endian) == Some(elf::DT_NEEDED as i32))
        .filter_map(|d| d.string(endian, strings).ok())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect()
}

/// Mach-O dylib dependencies from the `LC_LOAD_DYLIB` family of load
/// commands (weak / reexport / lazy / upward included).
fn macho_dylibs<Mach: MachHeader<Endian = Endianness>>(data: &[u8]) -> Vec<String> {
    let Ok(header) = Mach::parse(data, 0) else {
        return Vec::new();
    };
    let Ok(endian) = header.endian() else {
        return Vec::new();
    };
    let Ok(mut commands) = header.load_commands(endian, data, 0) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    while let Ok(Some(command)) = commands.next() {
        if let Ok(Some(dylib)) = command.dylib()
            && let Ok(name) = command.string(endian, dylib.dylib.name)
        {
            out.push(String::from_utf8_lossy(name).into_owned());
        }
    }
    out
}

/// PE/COFF import-table DLL names, deduplicated in first-seen order. The
/// unified reader carries the library name on each `Import` for PE.
fn pe_imports(data: &[u8]) -> Vec<String> {
    let Ok(file) = object::File::parse(data) else {
        return Vec::new();
    };
    let Ok(imports) = file.imports() else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for import in imports {
        let lib = String::from_utf8_lossy(import.library()).into_owned();
        if !lib.is_empty() && !out.contains(&lib) {
            out.push(lib);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn read(name: &str) -> Vec<u8> {
        let mut p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        p.push("test-data");
        p.push(name);
        std::fs::read(p).unwrap()
    }

    /// A Mach-O dylib's `LC_LOAD_DYLIB` dependencies are listed; its own
    /// `LC_ID_DYLIB` install name is not mistaken for a dependency.
    #[test]
    fn macho_dylib_lists_dependencies_not_self() {
        let libs = linked_libraries(&read("tiny.dylib"));
        assert!(
            libs.iter().any(|l| l.ends_with("libSystem.B.dylib")),
            "links libSystem: {libs:?}"
        );
        assert!(
            !libs.iter().any(|l| l.ends_with("tiny.dylib")),
            "own install name excluded: {libs:?}"
        );
    }

    /// Formats without a walked dependency list (here WebAssembly) yield
    /// no libraries rather than erroring.
    #[test]
    fn unwalked_format_yields_none() {
        assert!(linked_libraries(&read("minimal.wasm")).is_empty());
    }
}
