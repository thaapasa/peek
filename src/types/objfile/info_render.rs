//! Object-file info section rendering. Maps the semantic `object` enum
//! values held in [`ObjectMeta`] to display labels — the only place
//! object-file metadata becomes presentation text. On a parse error
//! only the error row is shown.

use object::{Architecture, BinaryFormat, Endianness, ObjectKind};

use super::info::{BuildIdKind, ObjectInfo, ObjectMeta};
use crate::info::{push_field, push_section_header, thousands_sep};
use crate::theme::PeekTheme;

pub fn render_section(lines: &mut Vec<String>, info: &ObjectInfo, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Object File", theme);

    let Some(meta) = &info.meta else {
        let msg = info
            .error
            .as_deref()
            .unwrap_or("could not parse object file");
        push_field(lines, "Status", &theme.paint_warning(msg), theme);
        return;
    };

    push_field(
        lines,
        "Format",
        &theme.paint_value(format_label(meta.format)),
        theme,
    );
    push_field(
        lines,
        "Architecture",
        &theme.paint_value(&arch_label(meta.architecture)),
        theme,
    );
    if !meta.universal.is_empty() {
        let list = meta
            .universal
            .iter()
            .map(|a| arch_label(*a))
            .collect::<Vec<_>>()
            .join(", ");
        let selected = meta
            .universal
            .get(meta.universal_selected)
            .map(|a| arch_label(*a))
            .unwrap_or_else(|| "?".to_string());
        push_field(
            lines,
            "Universal",
            &theme.paint_value(&format!("{list} (showing {selected})")),
            theme,
        );
    }
    push_field(
        lines,
        "Type",
        &theme.paint_value(kind_label(meta.kind)),
        theme,
    );
    push_field(
        lines,
        "Class",
        &theme.paint_value(if meta.is_64 { "64-bit" } else { "32-bit" }),
        theme,
    );
    push_field(
        lines,
        "Endianness",
        &theme.paint_value(endianness_label(meta.endianness)),
        theme,
    );
    if let Some(entry) = meta.entry {
        push_field(
            lines,
            "Entry point",
            &theme.paint_value(&format!("0x{entry:x}")),
            theme,
        );
    }
    push_field(
        lines,
        "Sections",
        &theme.paint_value(&thousands_sep(meta.section_count as u64)),
        theme,
    );
    push_field(
        lines,
        "Symbols",
        &theme.paint_value(&symbol_summary(meta)),
        theme,
    );
    push_field(
        lines,
        "Debug info",
        &theme.paint_value(if meta.has_debug_info {
            "present"
        } else {
            "none"
        }),
        theme,
    );
    if let Some((kind, bytes)) = &meta.build_id {
        let (label, value) = match kind {
            BuildIdKind::GnuBuildId => ("Build ID", hex(bytes)),
            BuildIdKind::MachUuid => ("UUID", uuid(bytes)),
            BuildIdKind::PdbGuid => ("PDB GUID", uuid(bytes)),
        };
        push_field(lines, label, &theme.paint_value(&value), theme);
    }
    // Only surfaced when present — a statically-linked or format-without-
    // deps file leaves the section out rather than printing "none".
    if !meta.linked_libraries.is_empty() {
        push_field(
            lines,
            "Linked libs",
            &theme.paint_value(&meta.linked_libraries.join(", ")),
            theme,
        );
    }
}

/// `.symtab` count with the dynamic-symbol count appended when present;
/// "none (stripped)" when the file carries neither table.
fn symbol_summary(meta: &ObjectMeta) -> String {
    if meta.symbol_count == 0 && meta.dynamic_symbol_count == 0 {
        return "none (stripped)".to_string();
    }
    let mut s = thousands_sep(meta.symbol_count as u64);
    if meta.dynamic_symbol_count > 0 {
        s.push_str(&format!(
            " (+{} dynamic)",
            thousands_sep(meta.dynamic_symbol_count as u64)
        ));
    }
    s
}

/// Continuous lowercase hex — for variable-length build IDs.
fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Canonical 8-4-4-4-12 UUID form. Falls back to plain hex if the blob
/// isn't 16 bytes (so a malformed record still renders something).
fn uuid(bytes: &[u8]) -> String {
    if bytes.len() != 16 {
        return hex(bytes);
    }
    let h = hex(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

fn format_label(f: BinaryFormat) -> &'static str {
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

fn kind_label(k: ObjectKind) -> &'static str {
    match k {
        ObjectKind::Relocatable => "relocatable object",
        ObjectKind::Executable => "executable",
        ObjectKind::Dynamic => "dynamic library",
        ObjectKind::Core => "core dump",
        _ => "unknown",
    }
}

fn endianness_label(e: Endianness) -> &'static str {
    match e {
        Endianness::Little => "little-endian",
        Endianness::Big => "big-endian",
    }
}

/// Friendly label for the common architectures; anything else falls
/// back to the `object` enum's debug name (still readable — `S390x` etc).
fn arch_label(a: Architecture) -> String {
    match a {
        Architecture::X86_64 => "x86-64".to_string(),
        Architecture::I386 => "x86 (i386)".to_string(),
        Architecture::Aarch64 => "AArch64".to_string(),
        Architecture::Arm => "ARM".to_string(),
        Architecture::Wasm32 => "WebAssembly (32-bit)".to_string(),
        Architecture::Wasm64 => "WebAssembly (64-bit)".to_string(),
        Architecture::Unknown => "unknown".to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_continuous_lowercase() {
        assert_eq!(hex(&[0x0a, 0xff, 0x00]), "0aff00");
    }

    #[test]
    fn uuid_uses_canonical_grouping() {
        let bytes: Vec<u8> = (0u8..16).collect();
        assert_eq!(uuid(&bytes), "00010203-0405-0607-0809-0a0b0c0d0e0f");
    }

    #[test]
    fn uuid_falls_back_to_hex_when_not_16_bytes() {
        assert_eq!(uuid(&[0xde, 0xad]), "dead");
    }
}
