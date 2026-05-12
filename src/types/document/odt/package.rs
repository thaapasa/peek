//! Placeholder — populated in Phase 2.

use anyhow::{Result, anyhow};

use crate::input::InputSource;
use crate::types::document::ast::Doc;

pub fn open(_source: &InputSource) -> Result<Doc> {
    Err(anyhow!("ODT parser not yet implemented"))
}
