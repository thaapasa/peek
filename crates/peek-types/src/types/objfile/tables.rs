//! Object-file Sections table: walk the `object` view into structured
//! `readelf -S`-style table data. Layout and painting live in the shared
//! `viewer::table::TableMode`. (Symbols are a listing, not a table — see
//! [`super::symbol_list`].)

use object::{Object, ObjectSection, SectionKind};

use crate::viewer::table::{Align, Cell, CellRole, Table, cell, fit_columns};

/// Build the sections table from a parsed object file.
pub fn build_sections(file: &object::File<'_>) -> Table {
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

fn section_kind_label(k: SectionKind) -> &'static str {
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
