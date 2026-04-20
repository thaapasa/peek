use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Source of input content — either a file on disk or buffered stdin.
///
/// Decouples "where data comes from" from "how it's displayed".
pub enum InputSource {
    File(PathBuf),
    Stdin { data: Vec<u8> },
}

impl InputSource {
    /// Full content as UTF-8 text.
    pub fn read_text(&self) -> Result<String> {
        match self {
            Self::File(path) => fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display())),
            Self::Stdin { data } => {
                String::from_utf8(data.clone()).context("stdin is not valid UTF-8")
            }
        }
    }

    /// Display name: filename for files, `<stdin>` for stdin.
    pub fn name(&self) -> &str {
        match self {
            Self::File(path) => path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
            Self::Stdin { .. } => "<stdin>",
        }
    }

    /// Filesystem path (None for stdin).
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::File(path) => Some(path.as_path()),
            Self::Stdin { .. } => None,
        }
    }
}
