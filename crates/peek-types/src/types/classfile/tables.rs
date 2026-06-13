//! Classfile Fields / Methods tables. Layout and painting live in the
//! shared `viewer::table::TableMode`; this builds the structured data.

use anyhow::{Result, anyhow};
use cafebabe::{
    ClassFile, FieldAccessFlags, MethodAccessFlags, ParseOptions, parse_class_with_options,
};

use super::descriptor;
use crate::input::InputSource;
use crate::viewer::table::{Align, Cell, CellRole, Table, cell, cell_spans, fit_columns};

/// Both rendered tables for one classfile.
pub struct ClassfileTables {
    pub fields: Table,
    pub methods: Table,
}

/// Parse `source` and build its Fields and Methods tables.
pub fn build(source: &InputSource) -> Result<ClassfileTables> {
    let bytes = source.read_bytes(crate::input::limits::Budget::Sidecar("class file"))?;
    let mut opts = ParseOptions::default();
    opts.parse_bytecode(false);
    let class = parse_class_with_options(&bytes, &opts)
        .map_err(|e| anyhow!("not a valid classfile: {e}"))?;
    Ok(ClassfileTables {
        fields: build_fields(&class),
        methods: build_methods(&class),
    })
}

fn build_fields(class: &ClassFile<'_>) -> Table {
    let rows: Vec<Vec<Cell>> = class
        .fields
        .iter()
        .map(|f| {
            vec![
                cell(field_modifiers(f.access_flags), CellRole::Tag),
                cell_spans(descriptor::field(&f.descriptor)),
                cell(f.name.to_string(), CellRole::Name),
            ]
        })
        .collect();
    let columns = fit_columns(
        &[
            ("Modifiers", Align::Left),
            ("Type", Align::Left),
            ("Name", Align::Left),
        ],
        &rows,
    );
    let notice = rows.is_empty().then(|| "(no fields)".to_string());
    Table {
        columns,
        rows,
        notice,
    }
}

fn build_methods(class: &ClassFile<'_>) -> Table {
    let rows: Vec<Vec<Cell>> = class
        .methods
        .iter()
        .map(|m| {
            vec![
                cell(method_modifiers(m.access_flags), CellRole::Tag),
                cell(m.name.to_string(), CellRole::Primary),
                cell_spans(descriptor::method(&m.descriptor)),
            ]
        })
        .collect();
    let columns = fit_columns(
        &[
            ("Modifiers", Align::Left),
            ("Method", Align::Left),
            ("Signature", Align::Left),
        ],
        &rows,
    );
    let notice = rows.is_empty().then(|| "(no methods)".to_string());
    Table {
        columns,
        rows,
        notice,
    }
}

fn field_modifiers(f: FieldAccessFlags) -> String {
    let mut v: Vec<&str> = Vec::new();
    if f.contains(FieldAccessFlags::PUBLIC) {
        v.push("public");
    }
    if f.contains(FieldAccessFlags::PRIVATE) {
        v.push("private");
    }
    if f.contains(FieldAccessFlags::PROTECTED) {
        v.push("protected");
    }
    if f.contains(FieldAccessFlags::STATIC) {
        v.push("static");
    }
    if f.contains(FieldAccessFlags::FINAL) {
        v.push("final");
    }
    if f.contains(FieldAccessFlags::VOLATILE) {
        v.push("volatile");
    }
    if f.contains(FieldAccessFlags::TRANSIENT) {
        v.push("transient");
    }
    v.join(" ")
}

fn method_modifiers(f: MethodAccessFlags) -> String {
    let mut v: Vec<&str> = Vec::new();
    if f.contains(MethodAccessFlags::PUBLIC) {
        v.push("public");
    }
    if f.contains(MethodAccessFlags::PRIVATE) {
        v.push("private");
    }
    if f.contains(MethodAccessFlags::PROTECTED) {
        v.push("protected");
    }
    if f.contains(MethodAccessFlags::STATIC) {
        v.push("static");
    }
    if f.contains(MethodAccessFlags::FINAL) {
        v.push("final");
    }
    if f.contains(MethodAccessFlags::ABSTRACT) {
        v.push("abstract");
    }
    if f.contains(MethodAccessFlags::SYNCHRONIZED) {
        v.push("synchronized");
    }
    if f.contains(MethodAccessFlags::NATIVE) {
        v.push("native");
    }
    v.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample() -> InputSource {
        let mut p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        p.push("test-data/Sample.class");
        InputSource::File(p)
    }

    /// A method signature reaches the Methods table as a multi-colour
    /// span cell — punctuation, primitives and class names tagged with
    /// distinct roles — not one flat block.
    #[test]
    fn method_signature_cell_is_multi_span() {
        let tables = build(&sample()).unwrap();
        let spans = tables
            .methods
            .rows
            .iter()
            .filter_map(|r| r.get(2)) // Signature is the third column.
            .find_map(|c| c.spans.as_ref())
            .expect("a method signature renders as spans");
        assert!(
            spans.iter().any(|(t, _)| t == "("),
            "signature opens with ("
        );
        let roles: std::collections::HashSet<_> = spans
            .iter()
            .map(|(_, r)| std::mem::discriminant(r))
            .collect();
        assert!(roles.len() > 1, "signature uses multiple colour roles");
    }
}
