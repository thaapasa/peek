//! Jupyter notebook (`.ipynb`) parse model.
//!
//! `.ipynb` is JSON on the wire. Rather than bind to a rigid serde
//! struct that rejects the long tail of real-world notebooks (nbformat
//! 3 vs 4, missing optional fields, vendor extensions), we walk a
//! `serde_json::Value` and pull out only the shape the viewer needs.
//! Everything not understood is ignored, never an error.

use crate::theme::strip_ansi;
use serde_json::Value;

/// A parsed notebook: kernel/language metadata plus the ordered cell
/// list. Built once per render / info gather from the raw JSON text.
pub(crate) struct Notebook {
    pub nbformat: (i64, i64),
    /// Kernel language (`metadata.kernelspec.language` or
    /// `metadata.language_info.name`), e.g. `python`. Used as the
    /// syntect token for code-cell highlighting.
    pub language: Option<String>,
    pub language_version: Option<String>,
    /// Human kernel name (`metadata.kernelspec.display_name`).
    pub kernel: Option<String>,
    pub cells: Vec<Cell>,
}

pub(crate) struct Cell {
    pub kind: CellKind,
    pub source: String,
    /// `execution_count` for code cells (`None` when never run).
    pub exec_count: Option<i64>,
    pub outputs: Vec<Output>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum CellKind {
    Markdown,
    Code,
    Raw,
}

/// One rendered cell output. The mime bundle on a `display_data` /
/// `execute_result` is collapsed to a single best representation
/// (image > text > html-note) at parse time so the renderer stays dumb.
#[derive(Debug)]
pub(crate) enum Output {
    /// `stream` output — stdout / stderr text.
    Stream { stderr: bool, text: String },
    /// `text/plain` representation of a result.
    Text(String),
    /// A rich image output kept as a note (inline ASCII rendering is a
    /// follow-up — see module docs on the renderer).
    Image { mime: String },
    /// A `text/html` result with no `text/plain` fallback.
    Html,
    /// `error` output — exception name, value, and the (ANSI-stripped)
    /// traceback joined into one block.
    Error {
        ename: String,
        evalue: String,
        traceback: String,
    },
}

impl Notebook {
    /// Parse notebook JSON. Returns `None` only when the bytes are not
    /// JSON at all; a JSON document that merely lacks notebook fields
    /// yields an empty-celled notebook rather than failing.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let root: Value = serde_json::from_str(text).ok()?;
        let meta = root.get("metadata");
        let kernelspec = meta.and_then(|m| m.get("kernelspec"));
        let language_info = meta.and_then(|m| m.get("language_info"));

        let language = kernelspec
            .and_then(|k| k.get("language"))
            .or_else(|| language_info.and_then(|l| l.get("name")))
            .and_then(Value::as_str)
            .map(str::to_string);
        let language_version = language_info
            .and_then(|l| l.get("version"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let kernel = kernelspec
            .and_then(|k| k.get("display_name"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let major = root.get("nbformat").and_then(Value::as_i64).unwrap_or(4);
        let minor = root
            .get("nbformat_minor")
            .and_then(Value::as_i64)
            .unwrap_or(0);

        Some(Self {
            nbformat: (major, minor),
            language,
            language_version,
            kernel,
            cells: collect_cells(&root),
        })
    }
}

/// nbformat 4 keeps cells at the top level; nbformat 3 nested them under
/// `worksheets[].cells`. Yields the raw cell `Value`s in document order so
/// both the parse model here and the blocks listing share one definition
/// of "where the cells live" — the nbformat-3-vs-4 rule can't drift.
pub(super) fn cells(root: &Value) -> Vec<&Value> {
    if let Some(cells) = root.get("cells").and_then(Value::as_array) {
        return cells.iter().collect();
    }
    root.get("worksheets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|ws| ws.get("cells").and_then(Value::as_array))
        .flatten()
        .collect()
}

fn collect_cells(root: &Value) -> Vec<Cell> {
    cells(root).into_iter().filter_map(parse_cell).collect()
}

fn parse_cell(cell: &Value) -> Option<Cell> {
    let kind = match cell.get("cell_type").and_then(Value::as_str)? {
        "markdown" => CellKind::Markdown,
        "code" => CellKind::Code,
        // "heading" is an nbformat-3 markdown variant; treat as markdown.
        "heading" => CellKind::Markdown,
        _ => CellKind::Raw,
    };
    // nbformat 4 uses `source`; nbformat 3 code cells used `input`.
    let source = join_text(cell.get("source").or_else(|| cell.get("input")));
    let exec_count = cell
        .get("execution_count")
        .or_else(|| cell.get("prompt_number"))
        .and_then(Value::as_i64);
    let outputs = cell
        .get("outputs")
        .and_then(Value::as_array)
        .map(|outs| outs.iter().filter_map(parse_output).collect())
        .unwrap_or_default();
    Some(Cell {
        kind,
        source,
        exec_count,
        outputs,
    })
}

fn parse_output(out: &Value) -> Option<Output> {
    match out.get("output_type").and_then(Value::as_str)? {
        "stream" => {
            let stderr = out.get("name").and_then(Value::as_str) == Some("stderr");
            Some(Output::Stream {
                stderr,
                text: join_text(out.get("text")),
            })
        }
        "error" => Some(Output::Error {
            ename: out
                .get("ename")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            evalue: out
                .get("evalue")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            traceback: strip_ansi(&join_traceback(out.get("traceback"))),
        }),
        "execute_result" | "display_data" => Some(parse_data_bundle(out.get("data")?)),
        _ => None,
    }
}

/// Collapse a mime bundle to one representation. Prefer an image (the
/// interesting output), then plain text, then an HTML note.
fn parse_data_bundle(data: &Value) -> Output {
    let obj = data.as_object();
    if let Some(mime) = obj.and_then(|o| o.keys().find(|k| k.starts_with("image/")).cloned()) {
        return Output::Image { mime };
    }
    if let Some(text) = obj.and_then(|o| o.get("text/plain")) {
        return Output::Text(join_text(Some(text)));
    }
    if obj.map(|o| o.contains_key("text/html")).unwrap_or(false) {
        return Output::Html;
    }
    Output::Text(String::new())
}

/// Notebook string fields are stored either as a single string or as an
/// array of line-strings (each usually carrying its own trailing `\n`).
/// Join both shapes to one string. The `Option` form is convenience over
/// [`value_text`] for the common `v.get("field")` call site.
pub(super) fn join_text(v: Option<&Value>) -> String {
    value_text(v.unwrap_or(&Value::Null))
}

/// Join a notebook string field (single string or array of line-strings)
/// to one owned string. Shared with the blocks listing.
pub(super) fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

/// Traceback frames are stored as an array of lines *without* trailing
/// newlines (unlike `source` / `text`), so they must be joined with
/// `\n` rather than concatenated — otherwise every frame runs together.
fn join_traceback(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NB: &str = r##"{
      "cells": [
        {"cell_type":"markdown","source":["# Title\n","body"]},
        {"cell_type":"code","execution_count":3,"source":["a=1\n","a"],
         "outputs":[
           {"output_type":"stream","name":"stdout","text":["hi\n"]},
           {"output_type":"execute_result","execution_count":3,"data":{"text/plain":["1"]}},
           {"output_type":"display_data","data":{"image/png":"AAA="}},
           {"output_type":"error","ename":"E","evalue":"v","traceback":["E: v"]}
         ]},
        {"cell_type":"raw","source":"raw text"}
      ],
      "metadata":{"kernelspec":{"display_name":"Python 3","language":"python"},
                  "language_info":{"name":"python","version":"3.11"}},
      "nbformat":4,"nbformat_minor":5
    }"##;

    #[test]
    fn parses_cells_metadata_and_outputs() {
        let nb = Notebook::parse(NB).expect("parses");
        assert_eq!(nb.nbformat, (4, 5));
        assert_eq!(nb.language.as_deref(), Some("python"));
        assert_eq!(nb.language_version.as_deref(), Some("3.11"));
        assert_eq!(nb.kernel.as_deref(), Some("Python 3"));
        assert_eq!(nb.cells.len(), 3);

        assert_eq!(nb.cells[0].kind, CellKind::Markdown);
        assert_eq!(nb.cells[0].source, "# Title\nbody");

        let code = &nb.cells[1];
        assert_eq!(code.kind, CellKind::Code);
        assert_eq!(code.exec_count, Some(3));
        assert_eq!(code.outputs.len(), 4);
        assert!(
            matches!(&code.outputs[0], Output::Stream { stderr: false, text } if text == "hi\n")
        );
        assert!(matches!(&code.outputs[1], Output::Text(t) if t == "1"));
        assert!(matches!(&code.outputs[2], Output::Image { mime } if mime == "image/png"));
        // Traceback ANSI stripped.
        assert!(matches!(&code.outputs[3], Output::Error { traceback, .. } if traceback == "E: v"));

        assert_eq!(nb.cells[2].kind, CellKind::Raw);
    }

    #[test]
    fn non_json_is_none() {
        assert!(Notebook::parse("not json").is_none());
    }

    #[test]
    fn nbformat3_worksheets_and_input() {
        let v3 = r#"{
          "worksheets":[{"cells":[
            {"cell_type":"code","input":["print(1)\n"],"prompt_number":7}
          ]}],
          "nbformat":3,"nbformat_minor":0
        }"#;
        let nb = Notebook::parse(v3).expect("parses");
        assert_eq!(nb.cells.len(), 1);
        assert_eq!(nb.cells[0].source, "print(1)\n");
        assert_eq!(nb.cells[0].exec_count, Some(7));
    }

    #[test]
    fn strip_ansi_removes_csi() {
        assert_eq!(strip_ansi("\u{1b}[0;31mred\u{1b}[0m"), "red");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    /// Parse the real `test-data/notebook.ipynb` fixture — a Jupyter
    /// export with markdown / code / raw cells, stream + result + image
    /// + error outputs, and an ANSI-coloured multi-line traceback.
    #[test]
    fn parses_real_fixture() {
        let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("test-data/notebook.ipynb");
        let text = std::fs::read_to_string(&path).expect("fixture present");
        let nb = Notebook::parse(&text).expect("fixture parses");

        assert_eq!(nb.nbformat, (4, 5));
        assert_eq!(nb.language.as_deref(), Some("python"));
        assert_eq!(nb.language_version.as_deref(), Some("3.11.4"));
        assert_eq!(nb.kernel.as_deref(), Some("Python 3 (ipykernel)"));

        let kinds: Vec<_> = nb.cells.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            vec![
                CellKind::Markdown,
                CellKind::Code,
                CellKind::Code,
                CellKind::Code,
                CellKind::Raw,
            ]
        );

        // Cell 1: stdout stream + a text/plain result.
        let c1 = &nb.cells[1];
        assert_eq!(c1.exec_count, Some(1));
        assert!(matches!(
            &c1.outputs[0],
            Output::Stream { stderr: false, text } if text == "loaded 8 points\n"
        ));
        assert!(matches!(&c1.outputs[1], Output::Text(t) if t == "np.float64(3.875)"));

        // Cell 2: the plot — image preferred over the text/plain Figure repr.
        let img = nb.cells[2]
            .outputs
            .iter()
            .find(|o| matches!(o, Output::Image { .. }))
            .expect("image output present");
        assert!(matches!(img, Output::Image { mime } if mime == "image/png"));

        // Cell 3: error — ANSI stripped, frames newline-joined (not run together).
        match &nb.cells[3].outputs[0] {
            Output::Error {
                ename,
                evalue,
                traceback,
            } => {
                assert_eq!(ename, "ZeroDivisionError");
                assert_eq!(evalue, "division by zero");
                assert!(!traceback.contains('\u{1b}'), "ANSI not stripped");
                assert!(traceback.contains('\n'), "frames not newline-joined");
                assert!(traceback.contains("Traceback (most recent call last)"));
            }
            other => panic!("expected error output, got {other:?}"),
        }
    }
}
