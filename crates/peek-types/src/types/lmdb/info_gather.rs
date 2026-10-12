//! LMDB info gathering: meta pages plus the named-DB records in MAIN. Never
//! walks the named DBs themselves, so the cost stays flat on multi-GB
//! files. Open failures land in `LmdbInfo::error` so Info always renders.

use peek_io::InputSource;

use super::cells;
use super::env::LmdbEnv;
use super::info::{LmdbInfo, LmdbStats};
use crate::info::Extras;

pub fn gather_extras(source: &InputSource) -> Extras {
    Box::new(gather(source))
}

fn gather(source: &InputSource) -> LmdbInfo {
    let env = match LmdbEnv::open(source) {
        Ok(env) => env,
        Err(e) => {
            return LmdbInfo {
                stats: None,
                error: Some(format!("{e:#}")),
            };
        }
    };
    let (named, named_note) = match env.named_dbs() {
        Ok(found) => (
            found
                .dbs
                .iter()
                .map(|d| (cells::text_or_hex(&d.name, d.name.len() as u64), d.db))
                .collect(),
            found
                .truncated
                .then(|| "MAIN scan stopped early — more databases may exist".to_string()),
        ),
        Err(e) => (Vec::new(), Some(format!("MAIN unreadable: {e:#}"))),
    };
    let meta = env.meta;
    LmdbInfo {
        stats: Some(LmdbStats {
            format: env.variant.label(),
            page_size: env.psize,
            map_size: meta.mapsize,
            used_pages: meta.last_pg.saturating_add(1),
            last_txnid: meta.txnid,
            active_meta: env.active_meta,
            free: meta.free,
            main: meta.main,
            named,
            named_note,
        }),
        error: None,
    }
}
