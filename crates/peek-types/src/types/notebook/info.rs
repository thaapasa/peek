//! Notebook info sidecar — kernel / language + cell and output tallies.

use super::model::{CellKind, Notebook, Output};

/// Per-notebook metadata for the Info section. Built by
/// [`info_gather`](super::info_gather) from a parsed [`Notebook`].
pub struct NotebookInfo {
    pub nbformat: (i64, i64),
    pub language: Option<String>,
    pub language_version: Option<String>,
    pub kernel: Option<String>,
    pub markdown_cells: usize,
    pub code_cells: usize,
    pub raw_cells: usize,
    pub output_count: usize,
    pub image_outputs: usize,
    pub error_outputs: usize,
    /// Highest `execution_count` seen — the run depth of the notebook.
    pub max_exec_count: Option<i64>,
}

impl NotebookInfo {
    pub(crate) fn from_notebook(nb: &Notebook) -> Self {
        let mut info = Self {
            nbformat: nb.nbformat,
            language: nb.language.clone(),
            language_version: nb.language_version.clone(),
            kernel: nb.kernel.clone(),
            markdown_cells: 0,
            code_cells: 0,
            raw_cells: 0,
            output_count: 0,
            image_outputs: 0,
            error_outputs: 0,
            max_exec_count: None,
        };
        for cell in &nb.cells {
            match cell.kind {
                CellKind::Markdown => info.markdown_cells += 1,
                CellKind::Code => info.code_cells += 1,
                CellKind::Raw => info.raw_cells += 1,
            }
            if let Some(n) = cell.exec_count {
                info.max_exec_count = Some(info.max_exec_count.map_or(n, |m| m.max(n)));
            }
            info.output_count += cell.outputs.len();
            for out in &cell.outputs {
                match out {
                    Output::Image { .. } => info.image_outputs += 1,
                    Output::Error { .. } => info.error_outputs += 1,
                    _ => {}
                }
            }
        }
        info
    }
}
