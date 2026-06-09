//! `SymbolListSource`: an object file's symbol table as an interactive
//! listing. Columns mirror the old `nm`-style table (address / size / type
//! / bind / name), but selecting a row **jumps the Hex view to that
//! symbol's byte offset** instead of extracting anything — the first
//! `ListSource` to use the in-frame jump select-semantic.
//!
//! A symbol carries a *virtual* address; the file offset is recovered by
//! locating the section that contains the address and adding the symbol's
//! displacement to the section's file range. Symbols with no file backing
//! (undefined, or sitting in `.bss`) keep their row but can't jump.

use object::{Object, ObjectSection, ObjectSymbol};

use crate::theme::PeekTheme;
use crate::viewer::listing::{ListSource, NameCell, RowCells};
use crate::viewer::modes::{ExtractTarget, ModeId, Position, RenderCtx};

struct SymbolRow {
    address: u64,
    size: u64,
    kind: &'static str,
    bind: &'static str,
    name: String,
    /// Byte offset in the file, when the symbol maps to on-disk bytes.
    file_offset: Option<u64>,
}

pub struct SymbolListSource {
    rows: Vec<SymbolRow>,
    addr_width: usize,
    size_width: usize,
    kind_width: usize,
    bind_width: usize,
}

/// Build the symbol listing from a parsed object file, plus any notice
/// (stripped / dynamic-fallback) to surface through Info.
pub fn build_from_file(file: &object::File<'_>) -> (SymbolListSource, Vec<String>) {
    let mut symbols: Vec<_> = file.symbols().collect();
    let mut from_dynamic = false;
    if symbols.is_empty() {
        symbols = file.dynamic_symbols().collect();
        from_dynamic = true;
    }

    let rows: Vec<SymbolRow> = symbols
        .iter()
        .map(|sym| SymbolRow {
            address: sym.address(),
            size: sym.size(),
            kind: symbol_kind_label(sym.kind()),
            bind: bind_label(sym),
            name: sym.name().unwrap_or("<invalid>").to_string(),
            file_offset: file_offset_of(file, sym),
        })
        .collect();

    let warnings = if rows.is_empty() {
        vec!["No symbols — the file is fully stripped.".to_string()]
    } else if from_dynamic {
        vec![".symtab stripped — showing the dynamic symbol table.".to_string()]
    } else {
        Vec::new()
    };

    (SymbolListSource::new(rows), warnings)
}

impl SymbolListSource {
    fn new(rows: Vec<SymbolRow>) -> Self {
        let addr_width = rows
            .iter()
            .map(|r| format!("{:#x}", r.address).len())
            .max()
            .unwrap_or(0);
        let size_width = rows
            .iter()
            .map(|r| r.size.to_string().len())
            .max()
            .unwrap_or(0);
        let kind_width = rows.iter().map(|r| r.kind.len()).max().unwrap_or(0);
        let bind_width = rows.iter().map(|r| r.bind.len()).max().unwrap_or(0);
        Self {
            rows,
            addr_width,
            size_width,
            kind_width,
            bind_width,
        }
    }

    /// The address / size / type / bind columns, pre-painted and padded.
    fn columns(&self, idx: usize, theme: &PeekTheme) -> Vec<String> {
        let r = &self.rows[idx];
        let addr = format!("{:>w$}", format!("{:#x}", r.address), w = self.addr_width);
        let size = format!("{:>w$}", r.size, w = self.size_width);
        let kind = format!("{:<w$}", r.kind, w = self.kind_width);
        let bind = format!("{:<w$}", r.bind, w = self.bind_width);
        // Address muted when the symbol can't be located in the file (no
        // jump target), so the actionable rows read brighter.
        let addr_color = if r.file_offset.is_some() {
            theme.value
        } else {
            theme.muted
        };
        vec![
            theme.paint(&addr, addr_color),
            theme.paint(&size, theme.muted),
            theme.paint(&kind, theme.accent),
            theme.paint(&bind, theme.muted),
        ]
    }
}

impl ListSource for SymbolListSource {
    fn len(&self) -> usize {
        self.rows.len()
    }

    fn parent(&self, _idx: usize) -> Option<usize> {
        None
    }

    fn selectable(&self, _idx: usize) -> bool {
        true
    }

    fn name(&self, idx: usize) -> &str {
        &self.rows[idx].name
    }

    fn source_label(&self) -> &str {
        "symbols"
    }

    /// Symbols aren't standalone files — there's nothing to extract. The
    /// row's action is the jump (see [`Self::jump_target`]).
    fn extract_target(&self, _idx: usize) -> Option<ExtractTarget> {
        None
    }

    fn jump_target(&self, idx: usize) -> Option<(ModeId, Position)> {
        self.rows[idx]
            .file_offset
            .map(|off| (ModeId::Hex, Position::Byte(off)))
    }

    fn row_cells(&self, idx: usize, ctx: &RenderCtx) -> RowCells {
        RowCells {
            prefix: String::new(),
            left: self.columns(idx, ctx.peek_theme),
            name: NameCell {
                text: self.rows[idx].name.clone(),
                is_dir: false,
            },
        }
    }

    fn flat_line(&self, idx: usize, theme: &PeekTheme) -> Option<String> {
        let cols = self.columns(idx, theme);
        let name = theme.paint(&self.rows[idx].name, theme.foreground);
        Some(format!("{}  {}", cols.join("  "), name))
    }
}

/// Recover a symbol's file offset from its virtual address: find the
/// section whose address range contains it and add the displacement to the
/// section's on-disk start. `None` for undefined symbols, sections with no
/// file backing (`.bss`), or addresses past the section's file bytes.
fn file_offset_of(file: &object::File<'_>, sym: &object::Symbol<'_, '_>) -> Option<u64> {
    let section_index = sym.section_index()?;
    let section = file.section_by_index(section_index).ok()?;
    let (file_start, file_size) = section.file_range()?;
    let delta = sym.address().checked_sub(section.address())?;
    if delta >= file_size {
        return None;
    }
    Some(file_start + delta)
}

fn bind_label(sym: &object::Symbol<'_, '_>) -> &'static str {
    if sym.is_undefined() {
        "undef"
    } else if sym.is_weak() {
        "weak"
    } else if sym.is_global() {
        "global"
    } else {
        "local"
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Vec<u8> {
        let mut p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        p.push("test-data");
        p.push(name);
        std::fs::read(p).expect("read fixture")
    }

    #[test]
    fn symbols_listed_with_in_bounds_jump_offsets() {
        let bytes = fixture("tiny.dylib");
        let loaded = super::super::load::load(&bytes).expect("parse object");
        let (src, _warnings) = build_from_file(&loaded.file);
        assert!(src.len() > 0, "fixture should carry symbols");
        // Every defined symbol that resolves to a jump target must point
        // inside the file, and the jump must be a Hex byte position.
        let mut jumpable = 0;
        for i in 0..src.len() {
            if let Some((mode, Position::Byte(off))) = src.jump_target(i) {
                assert_eq!(mode, ModeId::Hex);
                assert!(
                    (off as usize) < bytes.len(),
                    "offset {off:#x} past EOF {:#x}",
                    bytes.len()
                );
                jumpable += 1;
            }
        }
        assert!(jumpable > 0, "at least one symbol should be jumpable");
    }

    #[test]
    fn symbols_do_not_extract() {
        let bytes = fixture("tiny.dylib");
        let loaded = super::super::load::load(&bytes).expect("parse object");
        let (src, _) = build_from_file(&loaded.file);
        assert!((0..src.len()).all(|i| src.extract_target(i).is_none()));
    }
}
