//! Directory "extract": resolve a child name against the parent path
//! and hand back a fresh `InputSource::File`. The child is then
//! re-detected and rendered like any other file — and when it's itself
//! a directory, `ViewerState::push_extracted` collapses the new frame
//! onto the current one so the user doesn't accumulate a stack of
//! directories.

use crate::extract::{ExtractError, Extracted};
use crate::input::InputSource;

pub fn extract(source: &InputSource, key: &str) -> Result<Extracted, ExtractError> {
    let parent = source
        .path()
        .ok_or(ExtractError::Unsupported(
            "directory listing requires a filesystem path",
        ))?
        .to_path_buf();
    if key.is_empty() {
        return Err(ExtractError::InvalidKey(key.to_string()));
    }
    // `..` walks up one level. Canonicalize first so `peek .` →
    // `peek ..` actually resolves to the parent of cwd, not to the
    // literal `./..` path that would re-detect as the same directory.
    if key == ".." {
        let canonical = std::fs::canonicalize(&parent).map_err(|e| {
            ExtractError::Other(anyhow::anyhow!("failed to canonicalize {parent:?}: {e}"))
        })?;
        let up = canonical
            .parent()
            .ok_or_else(|| ExtractError::NotFound("already at filesystem root".to_string()))?;
        return Ok(Extracted {
            suggested_name: "..".to_string(),
            source: InputSource::File(up.to_path_buf()),
        });
    }
    // Single-segment names only — no slashes, no `.` traversal. The
    // viewer never sets a multi-segment key here, but reject defensively
    // so a hand-typed `--extract foo/bar` can't escape.
    if key.contains('/') || key == "." {
        return Err(ExtractError::UnsafePath(key.to_string()));
    }
    let child_path = parent.join(key);
    if !child_path.exists() {
        return Err(ExtractError::NotFound(key.to_string()));
    }
    Ok(Extracted {
        suggested_name: key.to_string(),
        source: InputSource::File(child_path),
    })
}
