//! Extract vocabulary shared by the per-type extractors.
//!
//! The `FileType → types::<x>::extract` dispatch is session glue and lives
//! in the binary; the value types every extractor produces / consumes
//! ([`Extracted`], [`ExtractOptions`], [`ExtractError`]) and the path-safety
//! helpers ([`sanitize_entry_path`], [`forward_slash_key`]) live here so
//! `peek-types` can build extract results without depending on the bin.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::input::InputSource;

/// Successful extract: fresh `InputSource` + suggested filename.
#[derive(Debug)]
pub struct Extracted {
    pub suggested_name: String,
    pub source: InputSource,
}

/// Per-extract knobs. Extractor-specific; defaults are always sensible.
#[derive(Debug, Default, Clone)]
pub struct ExtractOptions {
    /// Explicit SVG raster size in pixels (longest axis). CLI: `--extract-size`.
    /// Wins over `view_cols` when both are set.
    pub svg_size: Option<u32>,
    /// Hint that the extract will be rendered at this many character
    /// columns downstream (e.g. `--print --width N`). The SVG extractor
    /// derives a raster size that matches what live rendering at the
    /// same width would produce, so extract-then-render output
    /// matches plain render output. Ignored when `svg_size` is set.
    pub view_cols: Option<u32>,
    /// Force in-memory materialisation on the archive extract path,
    /// bypassing the tempfile spool. Also drops the 256 MiB safety cap
    /// since the user explicitly chose the memory path. CLI:
    /// `--no-tempfile`.
    pub no_tempfile: bool,
}

/// `Unsupported` = container has no extractor; `NotFound` / `InvalidKey`
/// = container opened but the key didn't resolve.
#[derive(Debug)]
pub enum ExtractError {
    NotFound(String),
    InvalidKey(String),
    UnsafePath(String),
    Unsupported(&'static str),
    Other(anyhow::Error),
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(k) => write!(f, "no entry matching {k:?}"),
            Self::InvalidKey(k) => write!(f, "invalid extract key {k:?}"),
            Self::UnsafePath(p) => write!(f, "unsafe entry path {p:?}"),
            Self::Unsupported(reason) => write!(f, "extract not supported: {reason}"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ExtractError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Other(e) = self {
            Some(e.as_ref())
        } else {
            None
        }
    }
}

impl From<anyhow::Error> for ExtractError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(e)
    }
}

/// Reject traversal / absolute paths in an untrusted archive/ISO key.
/// Shared across container types.
pub fn sanitize_entry_path(raw: &str) -> Result<PathBuf, ExtractError> {
    let trimmed = raw.trim_start_matches('/');
    let p = Path::new(trimmed);
    if p.is_absolute() {
        return Err(ExtractError::UnsafePath(raw.to_string()));
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => continue,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(ExtractError::UnsafePath(raw.to_string()));
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(ExtractError::InvalidKey(raw.to_string()));
    }
    Ok(out)
}

/// Render a sanitized entry path as a forward-slash key for archive /
/// container lookup. `PathBuf::push` uses the OS separator (`\` on
/// Windows), but archive members, PDF embedded-file names, and audio
/// embed keys are all `/`-separated regardless of host — so comparing
/// the raw `to_string_lossy()` of a sanitized PathBuf fails on Windows.
/// This helper rebuilds the key from the path's `Normal` components,
/// joined by `/`.
pub fn forward_slash_key(p: &Path) -> String {
    let mut out = String::new();
    for c in p.components() {
        if let Component::Normal(seg) = c {
            if !out.is_empty() {
                out.push('/');
            }
            out.push_str(&seg.to_string_lossy());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_traversal() {
        assert!(matches!(
            sanitize_entry_path("../../etc/passwd"),
            Err(ExtractError::UnsafePath(_))
        ));
        assert!(matches!(
            sanitize_entry_path("foo/../bar"),
            Err(ExtractError::UnsafePath(_))
        ));
    }

    #[test]
    fn sanitize_treats_leading_slash_as_relative() {
        // Container TOCs frequently store entries with a leading `/`
        // that are intended as relative-to-the-archive paths. We trim
        // the slash and treat the remainder as a relative key — there
        // is no host filesystem involved at sanitize time.
        let p = sanitize_entry_path("/etc/passwd").unwrap();
        assert_eq!(p, PathBuf::from("etc/passwd"));
    }

    #[test]
    fn sanitize_strips_leading_slash() {
        let p = sanitize_entry_path("foo/bar.txt").unwrap();
        assert_eq!(p, PathBuf::from("foo/bar.txt"));
    }

    #[test]
    fn sanitize_allows_dotted_segment() {
        let p = sanitize_entry_path("./foo/bar.txt").unwrap();
        assert_eq!(p, PathBuf::from("foo/bar.txt"));
    }

    #[test]
    fn sanitize_rejects_empty() {
        assert!(matches!(
            sanitize_entry_path(""),
            Err(ExtractError::InvalidKey(_))
        ));
    }

    /// `forward_slash_key` must rebuild the lookup key with `/`
    /// regardless of host OS separator. `PathBuf::push` uses `\` on
    /// Windows, which previously leaked into archive / PDF / audio
    /// lookups and made nested entries unreachable.
    #[test]
    fn forward_slash_key_joins_components_with_slash() {
        let p = sanitize_entry_path("config/theme.rs").unwrap();
        assert_eq!(forward_slash_key(&p), "config/theme.rs");

        let p = sanitize_entry_path("a/b/c/d.txt").unwrap();
        assert_eq!(forward_slash_key(&p), "a/b/c/d.txt");

        let p = sanitize_entry_path("flat.txt").unwrap();
        assert_eq!(forward_slash_key(&p), "flat.txt");
    }
}
