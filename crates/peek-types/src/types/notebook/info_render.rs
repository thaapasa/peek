//! The notebook info section, driven by one [`NotebookView`] that derives both
//! `serde::Serialize` (JSON) and [`InfoView`](crate::info::InfoView) (themed
//! print). [`NotebookInfo`] stays the gather struct; the view projects it.
//!
//! Several fields print as one row but serialize as several keys: the nbformat
//! version, the language + version, and the cell / output tallies (each a
//! small sub-view printing a count plus indented breakdown rows, flattening to
//! flat numeric JSON).

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::json;

use crate::info::{InfoNode, InfoValue, Role, Value, paint_count};
use peek_theme::PeekTheme;

use super::info::NotebookInfo;

crate::info_section!(NotebookInfo, NotebookView, "notebook");

#[derive(Serialize, crate::info::InfoView)]
#[info(title = "Notebook")]
struct NotebookView {
    #[info(label = "Format")]
    #[serde(flatten)]
    nbformat: NbFormat,
    #[info(label = "Kernel")]
    #[serde(skip_serializing_if = "Option::is_none")]
    kernel: Option<String>,
    #[info(label = "Language", skip_if = "Option::is_none")]
    #[serde(flatten)]
    language: Option<Language>,
    #[info(nest)]
    #[serde(flatten)]
    cells: Cells,
    #[info(nest)]
    #[serde(flatten)]
    outputs: Outputs,
    #[info(label = "Max Execution")]
    #[serde(rename = "max_exec_count", skip_serializing_if = "Option::is_none")]
    max_exec: Option<Value>,
}

impl From<&NotebookInfo> for NotebookView {
    fn from(info: &NotebookInfo) -> Self {
        let (major, minor) = info.nbformat;
        NotebookView {
            nbformat: NbFormat { major, minor },
            kernel: info.kernel.clone(),
            language: info.language.clone().map(|lang| Language {
                lang,
                version: info.language_version.clone(),
            }),
            cells: Cells {
                code: info.code_cells,
                markdown: info.markdown_cells,
                raw: info.raw_cells,
            },
            outputs: Outputs {
                total: info.output_count,
                images: info.image_outputs,
                errors: info.error_outputs,
            },
            max_exec: info
                .max_exec_count
                .map(|n| Value::split(format!("[{n}]"), Role::Value, json!(n))),
        }
    }
}

/// nbformat version. Print: `nbformat M.m`. JSON: `nbformat_major` +
/// `nbformat_minor`.
struct NbFormat {
    major: i64,
    minor: i64,
}

impl InfoValue for NbFormat {
    fn render_value(&self, theme: &PeekTheme) -> String {
        theme.paint_value(&format!("nbformat {}.{}", self.major, self.minor))
    }
}

impl Serialize for NbFormat {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut st = ser.serialize_struct("nbformat", 2)?;
        st.serialize_field("nbformat_major", &self.major)?;
        st.serialize_field("nbformat_minor", &self.minor)?;
        st.end()
    }
}

/// Kernel language. Print: `lang version` (or just `lang`). JSON: `language`
/// + optional `language_version`.
struct Language {
    lang: String,
    version: Option<String>,
}

impl InfoValue for Language {
    fn render_value(&self, theme: &PeekTheme) -> String {
        let label = match &self.version {
            Some(v) => format!("{} {v}", self.lang),
            None => self.lang.clone(),
        };
        theme.paint_value(&label)
    }
}

impl Serialize for Language {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let len = if self.version.is_some() { 2 } else { 1 };
        let mut st = ser.serialize_struct("language", len)?;
        st.serialize_field("language", &self.lang)?;
        if let Some(v) = &self.version {
            st.serialize_field("language_version", v)?;
        }
        st.end()
    }
}

/// Cell tally. Print: `Cells` total, an indented `Code/Markdown` split, and a
/// `Raw` row when any. JSON: `cell_count` + `code_cells` + `markdown_cells` +
/// `raw_cells`.
struct Cells {
    code: usize,
    markdown: usize,
    raw: usize,
}

impl crate::info::InfoView for Cells {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        let total = self.code + self.markdown + self.raw;
        let mut nodes = vec![
            InfoNode::Row {
                label: "Cells".into(),
                value: paint_count(total, theme),
            },
            InfoNode::Row {
                label: "  Code/Markdown".into(),
                value: format!(
                    "{}{}{}",
                    paint_count(self.code, theme),
                    theme.paint_muted(" / "),
                    paint_count(self.markdown, theme),
                ),
            },
        ];
        if self.raw > 0 {
            nodes.push(InfoNode::Row {
                label: "  Raw".into(),
                value: paint_count(self.raw, theme),
            });
        }
        nodes
    }
}

impl Serialize for Cells {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let total = self.code + self.markdown + self.raw;
        let mut st = ser.serialize_struct("cells", 4)?;
        st.serialize_field("cell_count", &total)?;
        st.serialize_field("code_cells", &self.code)?;
        st.serialize_field("markdown_cells", &self.markdown)?;
        st.serialize_field("raw_cells", &self.raw)?;
        st.end()
    }
}

/// Output tally. Print: `Outputs` count plus indented `Images` / `Errors` rows
/// when any (whole group omitted when there are no outputs). JSON:
/// `output_count` + `image_outputs` + `error_outputs`.
struct Outputs {
    total: usize,
    images: usize,
    errors: usize,
}

impl crate::info::InfoView for Outputs {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        if self.total == 0 {
            return Vec::new();
        }
        let mut nodes = vec![InfoNode::Row {
            label: "Outputs".into(),
            value: paint_count(self.total, theme),
        }];
        if self.images > 0 {
            nodes.push(InfoNode::Row {
                label: "  Images".into(),
                value: paint_count(self.images, theme),
            });
        }
        if self.errors > 0 {
            nodes.push(InfoNode::Row {
                label: "  Errors".into(),
                value: paint_count(self.errors, theme),
            });
        }
        nodes
    }
}

impl Serialize for Outputs {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut st = ser.serialize_struct("outputs", 3)?;
        st.serialize_field("output_count", &self.total)?;
        st.serialize_field("image_outputs", &self.images)?;
        st.serialize_field("error_outputs", &self.errors)?;
        st.end()
    }
}
