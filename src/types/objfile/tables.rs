//! Object-file Sections / Symbols tables: parse via `object` and build
//! structured `readelf -S` / `nm`-style table data. Layout and painting
//! live in the shared `viewer::table::TableMode`.

use anyhow::Result;
use object::{Object, ObjectSection, ObjectSymbol};

use super::load;
use crate::input::InputSource;
use crate::viewer::table::{Align, Cell, CellRole, Table, cell, fit_columns};

/// Both rendered tables for one object file.
pub struct ObjectTables {
    pub sections: Table,
    pub symbols: Table,
}

/// Parse `source` and build its section and symbol tables.
pub fn build(source: &InputSource) -> Result<ObjectTables> {
    let bytes = source.read_bytes()?;
    let loaded = load::load(&bytes)?;
    Ok(ObjectTables {
        sections: build_sections(&loaded.file),
        symbols: build_symbols(&loaded.file),
    })
}

fn build_sections(file: &object::File<'_>) -> Table {
    let rows: Vec<Vec<Cell>> = file
        .sections()
        .enumerate()
        .map(|(i, sec)| {
            vec![
                cell(i.to_string(), CellRole::Tag),
                cell(
                    sec.name().unwrap_or("<invalid>").to_string(),
                    CellRole::Primary,
                ),
                cell(format!("{:#x}", sec.address()), CellRole::Address),
                cell(sec.size().to_string(), CellRole::Numeric),
                cell(section_kind_label(sec.kind()).to_string(), CellRole::Muted),
            ]
        })
        .collect();
    let columns = fit_columns(
        &[
            ("Idx", Align::Right),
            ("Name", Align::Left),
            ("Address", Align::Right),
            ("Size", Align::Right),
            ("Kind", Align::Left),
        ],
        &rows,
    );
    let notice = rows.is_empty().then(|| "(no sections)".to_string());
    Table {
        columns,
        rows,
        notice,
    }
}

fn build_symbols(file: &object::File<'_>) -> Table {
    // Prefer the full `.symtab`; fall back to `.dynsym` when the file
    // has been stripped so a dynamically-linked binary still lists
    // something useful.
    let mut symbols: Vec<_> = file.symbols().collect();
    let mut from_dynamic = false;
    if symbols.is_empty() {
        symbols = file.dynamic_symbols().collect();
        from_dynamic = true;
    }

    let rows: Vec<Vec<Cell>> = symbols
        .iter()
        .map(|sym| {
            let bind = if sym.is_undefined() {
                "undef"
            } else if sym.is_weak() {
                "weak"
            } else if sym.is_global() {
                "global"
            } else {
                "local"
            };
            vec![
                cell(format!("{:#x}", sym.address()), CellRole::Address),
                cell(sym.size().to_string(), CellRole::Numeric),
                cell(symbol_kind_label(sym.kind()).to_string(), CellRole::Primary),
                cell(bind.to_string(), CellRole::Tag),
                cell(
                    sym.name().unwrap_or("<invalid>").to_string(),
                    CellRole::Name,
                ),
            ]
        })
        .collect();

    let columns = fit_columns(
        &[
            ("Address", Align::Right),
            ("Size", Align::Right),
            ("Type", Align::Left),
            ("Bind", Align::Left),
            ("Name", Align::Left),
        ],
        &rows,
    );
    let notice = if rows.is_empty() {
        Some("(no symbols — the file is fully stripped)".to_string())
    } else if from_dynamic {
        Some(".symtab stripped — showing the dynamic symbol table".to_string())
    } else {
        None
    };
    Table {
        columns,
        rows,
        notice,
    }
}

fn section_kind_label(k: object::SectionKind) -> &'static str {
    use object::SectionKind;
    match k {
        SectionKind::Text => "code",
        SectionKind::Data => "data",
        SectionKind::ReadOnlyData => "rodata",
        SectionKind::ReadOnlyString => "strings",
        SectionKind::UninitializedData => "bss",
        SectionKind::Common => "common",
        SectionKind::Tls | SectionKind::UninitializedTls | SectionKind::TlsVariables => "tls",
        SectionKind::Debug => "debug",
        SectionKind::Note => "note",
        SectionKind::Linker => "linker",
        SectionKind::Metadata => "metadata",
        _ => "other",
    }
}

fn symbol_kind_label(k: object::SymbolKind) -> &'static str {
    use object::SymbolKind;
    match k {
        SymbolKind::Text => "func",
        SymbolKind::Data => "data",
        SymbolKind::Section => "section",
        SymbolKind::File => "file",
        SymbolKind::Label => "label",
        SymbolKind::Tls => "tls",
        _ => "?",
    }
}
