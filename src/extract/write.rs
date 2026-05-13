//! Read→write boundary for extract: the only place that opens new
//! files or writes raw bytes to stdout.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::Extracted;

/// Where extracted bytes land. `Stdout` writes raw bytes (no
/// rendering — that's the print path). `Path` creates parent dirs as
/// needed.
pub enum Output {
    Stdout,
    Path(PathBuf),
}

impl Output {
    /// `-` → stdout; explicit path → file at that path; `None` →
    /// suggested name in cwd.
    pub fn resolve(user_path: Option<&Path>, suggested_name: &str) -> Self {
        match user_path {
            Some(p) if p == Path::new("-") => Output::Stdout,
            Some(p) => Output::Path(p.to_path_buf()),
            None => Output::Path(PathBuf::from(suggested_name)),
        }
    }
}

/// Stream the extracted source via `io::copy` so large extracts (e.g.
/// an ISO entry) don't fully buffer before writing.
pub fn write_extracted(extracted: &Extracted, output: Output) -> Result<PathBuf> {
    let mut stream = extracted.source.open_stream()?;

    match output {
        Output::Stdout => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            io::copy(&mut stream, &mut handle).context("failed to write to stdout")?;
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
            io::copy(&mut stream, &mut file)
                .with_context(|| format!("failed to write to {}", dest.display()))?;
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
