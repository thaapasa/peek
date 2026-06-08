//! `.DS_Store` info shape: parsed summary, or a parse-error surface.
//! Mirrors the classfile pattern — `meta` is `Some` exactly when `error`
//! is `None`. `info_render` projects this into the themed + JSON section.

/// Summary metadata, or the reason parsing failed.
pub struct DsStoreInfo {
    /// Summary. `None` when parsing failed.
    pub meta: Option<DsStoreMeta>,
    /// User-facing parse-failure reason. `None` on success.
    pub error: Option<String>,
}

impl DsStoreInfo {
    pub fn ok(meta: DsStoreMeta) -> Self {
        Self {
            meta: Some(meta),
            error: None,
        }
    }

    pub fn err(msg: String) -> Self {
        Self {
            meta: None,
            error: Some(msg),
        }
    }
}

/// One parsed store's summary.
pub struct DsStoreMeta {
    /// Total stored records.
    pub record_count: usize,
    /// Distinct filenames the records describe.
    pub file_count: usize,
    /// Folder view style label (`List view`, …), when a `vstl` record
    /// is present.
    pub view_style: Option<String>,
    /// Background setting (`default`, `color #…`, `picture`), when a
    /// `BKGD` record is present.
    pub background: Option<String>,
    /// Set when the B-tree walk stopped early.
    pub truncated: bool,
}
