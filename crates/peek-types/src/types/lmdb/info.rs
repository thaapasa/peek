//! LMDB info shape: environment stats, or the reason the file couldn't be
//! read. `stats` is `Some` exactly when `error` is `None`.

use super::format::DbRecord;

pub struct LmdbInfo {
    pub stats: Option<LmdbStats>,
    pub error: Option<String>,
}

pub struct LmdbStats {
    /// On-disk format: `0.9` or `1.0`.
    pub format: &'static str,
    pub page_size: u64,
    pub map_size: u64,
    /// Pages the active transaction uses (`last_pg + 1`).
    pub used_pages: u64,
    pub last_txnid: u64,
    /// Which meta page (0 / 1) is active.
    pub active_meta: u8,
    pub free: DbRecord,
    pub main: DbRecord,
    /// Named sub-databases: display name + record, in key order.
    pub named: Vec<(String, DbRecord)>,
    /// Set when the named-DB scan stopped early or failed.
    pub named_note: Option<String>,
}
