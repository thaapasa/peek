//! Object-file info section rendering. On a parse error only the error
//! row is shown; otherwise the ELF / Mach-O / PE / COFF header summary.

use super::info::ObjectInfo;
use crate::info::{push_field, push_section_header, thousands_sep};
use crate::theme::PeekTheme;

pub fn render_section(lines: &mut Vec<String>, info: &ObjectInfo, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Object File", theme);

    if let Some(err) = &info.error {
        push_field(lines, "Status", &theme.paint_warning(err), theme);
        return;
    }

    push_field(lines, "Format", &theme.paint_value(info.format), theme);
    push_field(
        lines,
        "Architecture",
        &theme.paint_value(&info.architecture),
        theme,
    );
    if !info.universal.is_empty() {
        let selected = info
            .universal
            .get(info.universal_selected)
            .map(String::as_str)
            .unwrap_or("?");
        push_field(
            lines,
            "Universal",
            &theme.paint_value(&format!(
                "{} (showing {selected})",
                info.universal.join(", ")
            )),
            theme,
        );
    }
    push_field(lines, "Type", &theme.paint_value(info.kind), theme);
    push_field(
        lines,
        "Class",
        &theme.paint_value(if info.is_64 { "64-bit" } else { "32-bit" }),
        theme,
    );
    push_field(
        lines,
        "Endianness",
        &theme.paint_value(info.endianness),
        theme,
    );
    if let Some(entry) = info.entry {
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
        &theme.paint_value(&thousands_sep(info.section_count as u64)),
        theme,
    );
    push_field(
        lines,
        "Symbols",
        &theme.paint_value(&symbol_summary(info)),
        theme,
    );
    push_field(
        lines,
        "Debug info",
        &theme.paint_value(if info.has_debug_info {
            "present"
        } else {
            "none"
        }),
        theme,
    );
}

/// `.symtab` count with the dynamic-symbol count appended when present;
/// "none (stripped)" when the file carries neither table.
fn symbol_summary(info: &ObjectInfo) -> String {
    if info.symbol_count == 0 && info.dynamic_symbol_count == 0 {
        return "none (stripped)".to_string();
    }
    let mut s = thousands_sep(info.symbol_count as u64);
    if info.dynamic_symbol_count > 0 {
        s.push_str(&format!(
            " (+{} dynamic)",
            thousands_sep(info.dynamic_symbol_count as u64)
        ));
    }
    s
}
