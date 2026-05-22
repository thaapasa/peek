//! Object-file Sections / Symbols tables: parse via `object` and build
//! structured `readelf -S` / `nm`-style table data. Painting and layout
//! are deferred to `ObjectTableMode`, which repaints from the live theme
//! on every frame — so a runtime theme cycle recolours the tables.
//! These tables are display-only — there is no per-row extract.

use anyhow::Result;
use object::{Object, ObjectSection, ObjectSymbol};

use super::load;
use crate::input::InputSource;

/// Both rendered tables for one object file.
pub struct ObjectTables {
    pub sections: ObjectTable,
    pub symbols: ObjectTable,
}

/// One structured table: fixed column layout + rows of typed cells, plus
/// an optional one-line notice shown above the body.
pub struct ObjectTable {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Cell>>,
    pub notice: Option<String>,
}

/// One column's static layout. A `width` of 0 marks a flexible last
/// column — rendered at its natural length, never padded or truncated.
pub struct Column {
    pub header: &'static str,
    pub width: usize,
    pub align: Align,
}

#[derive(Clone, Copy)]
pub enum Align {
    Left,
    Right,
}

/// One cell: its text plus the colour role the renderer paints it with.
pub struct Cell {
    pub text: String,
    pub role: CellRole,
}

/// Colour role for a cell — resolved against the live theme at render
/// time so a theme cycle recolours every table.
#[derive(Clone, Copy)]
pub enum CellRole {
    /// Section index / symbol bind — theme `label`.
    Tag,
    /// Section name / symbol type — theme `accent` (keyword colour).
    Primary,
    /// Hex address — `0x` prefix dimmed, digits in the value colour.
    Address,
    /// Byte sizes — an accent/value blend.
    Numeric,
    /// Section kind — theme `muted`, plainer than the rest.
    Muted,
    /// Symbol name — plain foreground.
    Name,
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

fn build_sections(file: &object::File<'_>) -> ObjectTable {
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
    ObjectTable {
        columns,
        rows,
        notice,
    }
}

fn build_symbols(file: &object::File<'_>) -> ObjectTable {
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
    ObjectTable {
        columns,
        rows,
        notice,
    }
}

fn cell(text: String, role: CellRole) -> Cell {
    Cell { text, role }
}

/// Hard ceiling on a fixed column's width — keeps one pathological cell
/// from pushing the table off-screen; longer cells truncate with `…`.
const MAX_FIXED_WIDTH: usize = 40;

/// Build columns whose fixed widths are fitted to the actual cell
/// content (header included). The last column is left flexible
/// (`width == 0`) — rendered at its natural length, panned via the
/// view's horizontal scroll.
fn fit_columns(specs: &[(&'static str, Align)], rows: &[Vec<Cell>]) -> Vec<Column> {
    let last = specs.len().saturating_sub(1);
    specs
        .iter()
        .enumerate()
        .map(|(i, &(header, align))| {
            let width = if i == last {
                0
            } else {
                let widest = rows
                    .iter()
                    .filter_map(|r| r.get(i))
                    .map(|c| c.text.chars().count())
                    .max()
                    .unwrap_or(0);
                widest.max(header.chars().count()).min(MAX_FIXED_WIDTH)
            };
            Column {
                header,
                width,
                align,
            }
        })
        .collect()
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
