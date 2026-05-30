//! Notebook blocks → listing entries + extract resolution.
//!
//! The TOC view lists every code cell and every image output as a flat,
//! ordered sequence with readable synthetic names (`code-1.py`,
//! `image-1.png`, …) rather than the notebook's own opaque cell ids.
//! Each name doubles as the extract key and the suggested filename, so
//! extracting (or descending into) a row yields a real `.py` / `.png`
//! that peek re-detects and renders — code highlighted, image drawn.
//!
//! This walks the raw `serde_json::Value` rather than the [`Notebook`]
//! model on purpose: the model deliberately drops image *bytes* to keep
//! the render hot-path lean, but extraction needs them. Keeping the
//! block walk here, off the render path, is the trade-off.
//!
//! [`Notebook`]: super::model::Notebook

use serde_json::Value;

use crate::base64;
use crate::viewer::listing::{Entry, EntryKind};

/// One extractable block, named in document order. `payload` borrows the
/// parsed JSON so byte materialisation stays lazy until extract time.
struct Block<'a> {
    name: String,
    payload: Payload<'a>,
}

enum Payload<'a> {
    /// Code-cell source, already joined.
    Code(String),
    /// Image output. `text` images (SVG) are taken verbatim; everything
    /// else is base64.
    Image { is_text: bool, data: &'a Value },
}

impl Payload<'_> {
    /// Decoded byte length, for the listing size column. Avoids a full
    /// decode for base64 images.
    fn size(&self) -> u64 {
        match self {
            Payload::Code(s) => s.len() as u64,
            Payload::Image {
                is_text: true,
                data,
            } => value_text(data).len() as u64,
            Payload::Image {
                is_text: false,
                data,
            } => base64::decoded_len(&value_text(data)) as u64,
        }
    }

    /// Materialise the block's bytes for extraction.
    fn bytes(&self) -> Option<Vec<u8>> {
        match self {
            Payload::Code(s) => Some(s.clone().into_bytes()),
            Payload::Image {
                is_text: true,
                data,
            } => Some(value_text(data).into_bytes()),
            Payload::Image {
                is_text: false,
                data,
            } => base64::decode(&value_text(data)),
        }
    }
}

/// Build the flat listing entries for a notebook's blocks. Returns an
/// empty list when the text isn't a parseable notebook.
pub(crate) fn block_entries(text: &str) -> Vec<Entry> {
    let Some(root) = serde_json::from_str::<Value>(text).ok() else {
        return Vec::new();
    };
    walk_blocks(&root)
        .into_iter()
        .map(|b| Entry {
            name: b.name,
            size: b.payload.size(),
            mtime: None,
            mode: None,
            kind: EntryKind::File,
        })
        .collect()
}

/// Resolve one block by its key (the synthetic name) to its suggested
/// filename + decoded bytes. `None` when the text isn't a notebook, the
/// key matches nothing, or an image fails to decode.
pub(crate) fn extract_block(text: &str, key: &str) -> Option<(String, Vec<u8>)> {
    let root = serde_json::from_str::<Value>(text).ok()?;
    let block = walk_blocks(&root).into_iter().find(|b| b.name == key)?;
    let bytes = block.payload.bytes()?;
    Some((block.name, bytes))
}

/// Walk cells in document order, emitting a [`Block`] per code cell and
/// per image output, numbered independently per kind.
fn walk_blocks(root: &Value) -> Vec<Block<'_>> {
    let lang = root
        .get("metadata")
        .and_then(|m| {
            m.get("kernelspec")
                .and_then(|k| k.get("language"))
                .or_else(|| m.get("language_info").and_then(|l| l.get("name")))
        })
        .and_then(Value::as_str)
        .unwrap_or("");
    let code_ext = lang_ext(lang);

    let mut blocks = Vec::new();
    let mut code_n = 0;
    let mut image_n = 0;
    for cell in cells(root) {
        if cell.get("cell_type").and_then(Value::as_str) != Some("code") {
            continue;
        }
        let source = join_value(cell.get("source").or_else(|| cell.get("input")));
        if !source.trim().is_empty() {
            code_n += 1;
            blocks.push(Block {
                name: format!("code-{code_n}.{code_ext}"),
                payload: Payload::Code(source),
            });
        }
        for out in cell
            .get("outputs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some((mime, data)) = image_output(out) {
                image_n += 1;
                let ext = mime_ext(mime);
                blocks.push(Block {
                    name: format!("image-{image_n}.{ext}"),
                    payload: Payload::Image {
                        is_text: ext == "svg",
                        data,
                    },
                });
            }
        }
    }
    blocks
}

/// nbformat 4 cells live at the top level; nbformat 3 nested them under
/// `worksheets[].cells`.
fn cells(root: &Value) -> Vec<&Value> {
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

/// First `image/*` entry in an output's mime bundle, with its raw data
/// `Value` (a string or an array of strings).
fn image_output(out: &Value) -> Option<(&str, &Value)> {
    match out.get("output_type").and_then(Value::as_str)? {
        "execute_result" | "display_data" => {
            let data = out.get("data")?.as_object()?;
            data.iter()
                .find(|(k, _)| k.starts_with("image/"))
                .map(|(k, v)| (k.as_str(), v))
        }
        _ => None,
    }
}

/// Source extension for a kernel language. Falls back to a sanitised
/// language token, then `txt`.
fn lang_ext(lang: &str) -> String {
    let known = match lang.to_ascii_lowercase().as_str() {
        "python" => Some("py"),
        "r" => Some("r"),
        "julia" => Some("jl"),
        "javascript" => Some("js"),
        "typescript" => Some("ts"),
        "ruby" => Some("rb"),
        "rust" => Some("rs"),
        "scala" => Some("scala"),
        "bash" | "sh" => Some("sh"),
        "c" => Some("c"),
        "c++" | "cpp" => Some("cpp"),
        "sql" => Some("sql"),
        "" => Some("txt"),
        _ => None,
    };
    match known {
        Some(ext) => ext.to_string(),
        None => sanitise_ext(lang),
    }
}

/// Synthetic name for the `seq`-th image output (1-based) of mime type
/// `mime` — the same name the Blocks listing assigns. Shared with the
/// rendered view so its inline image notes map to the listing rows.
pub(crate) fn image_name(seq: usize, mime: &str) -> String {
    format!("image-{seq}.{}", mime_ext(mime))
}

/// File extension for an image mime type (`image/png` → `png`,
/// `image/jpeg` → `jpg`, `image/svg+xml` → `svg`).
fn mime_ext(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        _ => "img",
    }
}

/// Keep only `[a-z0-9]` from an unknown language token so it can't
/// inject path separators or dots into the synthetic filename.
fn sanitise_ext(lang: &str) -> String {
    let cleaned: String = lang
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    if cleaned.is_empty() {
        "txt".to_string()
    } else {
        cleaned
    }
}

/// Notebook string fields are a single string or an array of line
/// strings; join both to one owned string.
fn join_value(v: Option<&Value>) -> String {
    value_text(v.unwrap_or(&Value::Null))
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NB: &str = r##"{
      "cells": [
        {"cell_type":"markdown","source":["# Title"]},
        {"cell_type":"code","source":["print(1)\n"],
         "outputs":[{"output_type":"display_data","data":{"image/png":"aGVsbG8="}}]},
        {"cell_type":"code","source":["x = 2\n"],
         "outputs":[{"output_type":"execute_result","data":{"text/plain":["2"]}}]},
        {"cell_type":"code","source":["   \n"]},
        {"cell_type":"code","source":["y = 3\n"],
         "outputs":[{"output_type":"display_data","data":{"image/svg+xml":["<svg/>"]}}]}
      ],
      "metadata":{"kernelspec":{"language":"python"}},
      "nbformat":4,"nbformat_minor":5
    }"##;

    #[test]
    fn lists_code_and_image_blocks_in_order() {
        let entries = block_entries(NB);
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        // Empty-source code cell is skipped; code/images numbered per kind.
        assert_eq!(
            names,
            vec![
                "code-1.py",
                "image-1.png",
                "code-2.py",
                "code-3.py",
                "image-2.svg"
            ]
        );
    }

    #[test]
    fn entry_sizes_reflect_decoded_bytes() {
        let entries = block_entries(NB);
        let by_name = |n: &str| entries.iter().find(|e| e.name == n).unwrap().size;
        assert_eq!(by_name("code-1.py"), "print(1)\n".len() as u64);
        assert_eq!(by_name("image-1.png"), 5); // "aGVsbG8=" → "hello"
        assert_eq!(by_name("image-2.svg"), "<svg/>".len() as u64);
    }

    #[test]
    fn extract_code_returns_source() {
        let (name, bytes) = extract_block(NB, "code-1.py").expect("code-1");
        assert_eq!(name, "code-1.py");
        assert_eq!(bytes, b"print(1)\n");
    }

    #[test]
    fn extract_png_decodes_base64() {
        let (name, bytes) = extract_block(NB, "image-1.png").expect("image-1");
        assert_eq!(name, "image-1.png");
        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn extract_svg_is_verbatim_text() {
        let (_, bytes) = extract_block(NB, "image-2.svg").expect("image-2");
        assert_eq!(bytes, b"<svg/>");
    }

    #[test]
    fn extract_unknown_key_is_none() {
        assert!(extract_block(NB, "code-9.py").is_none());
        assert!(extract_block("not json", "code-1.py").is_none());
    }
}
