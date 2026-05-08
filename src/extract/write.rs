//! Writing an [`Extracted`] item to disk or stdout.
//!
//! Sits at the read→write boundary: the rest of `extract` deals only in
//! `InputSource`s, and this module is the only place that creates new
//! files or opens stdout for binary output.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::Extracted;

/// Where the extracted bytes should land. `Stdout` writes raw bytes to
/// the process stdout (no formatting, no syntax highlighting — that's
/// the rendered-print path's job). `Path` writes to a specific file,
/// creating any missing parent directories.
pub enum Output {
    Stdout,
    Path(PathBuf),
}

impl Output {
    /// Resolve an `--output` argument plus the extract's suggested name
    /// into a destination. `--output` of `-` means stdout; an explicit
    /// path wins over the suggested name; otherwise the suggested name
    /// becomes the relative output path in the current directory.
    pub fn resolve(user_path: Option<&Path>, suggested_name: &str) -> Self {
        match user_path {
            Some(p) if p == Path::new("-") => Output::Stdout,
            Some(p) => Output::Path(p.to_path_buf()),
            None => Output::Path(PathBuf::from(suggested_name)),
        }
    }
}

/// Stream the extracted source to `output`. Reads in 64 KB chunks so
/// large extracts (e.g. an ISO entry) don't get fully buffered in
/// memory before writing.
pub fn write_extracted(extracted: &Extracted, output: Output) -> Result<PathBuf> {
    const CHUNK: usize = 64 * 1024;
    let bs = extracted.source.open_byte_source()?;
    let total = bs.len();

    match output {
        Output::Stdout => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            let mut offset: u64 = 0;
            while offset < total {
                let buf = bs.read_range(offset, CHUNK)?;
                if buf.is_empty() {
                    break;
                }
                handle
                    .write_all(&buf)
                    .context("failed to write to stdout")?;
                offset += buf.len() as u64;
            }
            handle.flush().context("failed to flush stdout")?;
            Ok(PathBuf::from("-"))
        }
        Output::Path(dest) => {
            if let Some(parent) = dest.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            let mut file = fs::File::create(&dest)
                .with_context(|| format!("failed to create {}", dest.display()))?;
            let mut offset: u64 = 0;
            while offset < total {
                let buf = bs.read_range(offset, CHUNK)?;
                if buf.is_empty() {
                    break;
                }
                file.write_all(&buf)
                    .with_context(|| format!("failed to write to {}", dest.display()))?;
                offset += buf.len() as u64;
            }
            file.flush()
                .with_context(|| format!("failed to flush {}", dest.display()))?;
            Ok(dest)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::Extracted;
    use crate::input::InputSource;
    use bytes::Bytes;

    #[test]
    fn resolve_dash_means_stdout() {
        let dash = PathBuf::from("-");
        assert!(matches!(
            Output::resolve(Some(&dash), "x.png"),
            Output::Stdout
        ));
    }

    #[test]
    fn resolve_explicit_path_wins() {
        let p = PathBuf::from("custom.png");
        match Output::resolve(Some(&p), "x.png") {
            Output::Path(out) => assert_eq!(out, p),
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn resolve_falls_back_to_suggested() {
        match Output::resolve(None, "frame-3.png") {
            Output::Path(out) => assert_eq!(out, PathBuf::from("frame-3.png")),
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn write_to_path_round_trips() {
        let extracted = Extracted {
            suggested_name: "out.bin".to_string(),
            source: InputSource::memory(Bytes::from_static(b"hello world"), "out.bin"),
        };
        let mut tmp = std::env::temp_dir();
        tmp.push(format!("peek-extract-write-{}.bin", std::process::id()));
        let dest = write_extracted(&extracted, Output::Path(tmp.clone())).unwrap();
        assert_eq!(dest, tmp);
        assert_eq!(fs::read(&tmp).unwrap(), b"hello world");
        let _ = fs::remove_file(&tmp);
    }
}
