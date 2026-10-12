//! An opened LMDB environment: the active meta page plus bounds-checked
//! page reads over a [`ByteSource`]. Never mmaps, never opens or writes the
//! lock file — a running writer is undisturbed, but peek also doesn't
//! register as a reader, so a file under active writes can read torn.

use anyhow::{Context, Result, bail, ensure};
use bytes::Bytes;
use peek_io::{ByteSource, InputSource};

use super::format::{
    self, F_DUPDATA, F_SUBDATA, Layout, META_LEN, Meta, Page, PageHeader, V09, V10, Variant,
};
use super::tree::LeafWalker;
use crate::types::lmdb::format::DbRecord;

/// Page sizes probed for meta page 1 when meta page 0 is damaged and can't
/// say where page 1 sits.
const FALLBACK_PAGE_SIZES: [u64; 5] = [4096, 8192, 16384, 32768, 65536];

/// MAIN entries scanned for named-DB records. MAIN usually holds only the
/// names; a MAIN with millions of plain keys stops here instead of being
/// walked end to end.
const MAX_MAIN_SCAN: u64 = 100_000;

pub struct LmdbEnv {
    src: Box<dyn ByteSource>,
    /// 0.9 or 1.0 page layout.
    pub variant: Variant,
    pub psize: u64,
    pub meta: Meta,
    /// Which meta page (0 / 1) is active — the higher valid `txnid`.
    pub active_meta: u8,
    pub file_len: u64,
}

/// One named sub-database and its B+tree record. LMDB 1.0 stores the name
/// NUL-terminated (`strlen + 1`); `name` drops that NUL.
pub struct NamedDb {
    pub name: Bytes,
    pub db: DbRecord,
}

/// Named DBs found in MAIN. `truncated` when the MAIN scan stopped early.
pub struct NamedDbs {
    pub dbs: Vec<NamedDb>,
    pub truncated: bool,
}

impl LmdbEnv {
    pub fn open(source: &InputSource) -> Result<Self> {
        let src = source.open_byte_source()?;
        let file_len = src.len();
        let head = src.read_range(0, META_LEN)?;
        // A damaged page 0 reads as `Unknown`; meta page 1 may still be
        // intact, so try both variants. A recognised foreign layout stops.
        let variants = match format::classify_layout(&head) {
            Layout::Native(v) => vec![v],
            Layout::Unknown => vec![V09, V10],
            other => bail!(other.unsupported_reason()),
        };
        for variant in variants {
            if let Some((meta, active_meta)) = pick_meta(&*src, &head, variant)? {
                return Ok(Self {
                    src,
                    variant,
                    psize: meta.page_size(),
                    meta,
                    active_meta,
                    file_len,
                });
            }
        }
        bail!("no valid LMDB meta page")
    }

    /// Pages present in the file — the bound on any honest tree walk.
    pub fn page_count(&self) -> u64 {
        self.file_len / self.psize
    }

    fn page_offset(&self, pgno: u64, len: u64) -> Result<u64> {
        let off = pgno
            .checked_mul(self.psize)
            .with_context(|| format!("page {pgno} out of range"))?;
        ensure!(
            off.checked_add(len).is_some_and(|end| end <= self.file_len),
            "page {pgno} lies past the end of the file"
        );
        Ok(off)
    }

    pub fn read_page(&self, pgno: u64) -> Result<Page> {
        let off = self.page_offset(pgno, self.psize)?;
        Page::new(self.src.read_range(off, self.psize as usize)?, self.variant)
            .with_context(|| format!("page {pgno}"))
    }

    /// Just the page header — enough to count a non-dupsort leaf.
    pub fn read_header(&self, pgno: u64) -> Result<PageHeader> {
        let h = self.variant.header;
        let off = self.page_offset(pgno, h as u64)?;
        let b = self.src.read_range(off, h)?;
        PageHeader::parse(&b, self.variant).with_context(|| format!("page {pgno} header truncated"))
    }

    /// File offset of an overflow value starting at page `pgno`, after
    /// checking the page is an overflow run long enough for `len` bytes.
    pub fn overflow_offset(&self, pgno: u64, len: u64) -> Result<u64> {
        let hdr = self.read_header(pgno)?;
        ensure!(hdr.is_overflow(), "page {pgno} is not an overflow page");
        let off = self.page_offset(pgno, 0)? + self.variant.header as u64;
        ensure!(
            off.checked_add(len).is_some_and(|end| end <= self.file_len),
            "overflow value at page {pgno} runs past the end of the file"
        );
        Ok(off)
    }

    /// Up to `max` leading bytes of an overflow value.
    pub fn read_overflow_prefix(&self, pgno: u64, len: u64, max: usize) -> Result<Bytes> {
        let off = self.overflow_offset(pgno, len)?;
        let take = len.min(max as u64) as usize;
        self.src.read_range(off, take)
    }

    /// Named sub-databases: MAIN leaf nodes carrying a DB record.
    pub fn named_dbs(&self) -> Result<NamedDbs> {
        let mut dbs = Vec::new();
        let mut walker = LeafWalker::new(self.meta.main.root);
        let mut scanned = 0u64;
        while let Some(leaf) = walker.next_leaf(self)? {
            let page = self.read_page(leaf.pgno)?;
            for i in 0..page.num_keys() {
                if scanned >= MAX_MAIN_SCAN {
                    return Ok(NamedDbs {
                        dbs,
                        truncated: true,
                    });
                }
                scanned += 1;
                let node = page.node(i)?;
                if node.flags & F_SUBDATA != 0
                    && node.flags & F_DUPDATA == 0
                    && let Some(db) = DbRecord::parse(&node.data()?)
                {
                    let name = match node.key.strip_suffix(b"\0") {
                        Some(stripped) => node.key.slice_ref(stripped),
                        None => node.key,
                    };
                    dbs.push(NamedDb { name, db });
                }
            }
        }
        Ok(NamedDbs {
            dbs,
            truncated: false,
        })
    }
}

/// The active meta page for `variant`: page 0 (from `head`) and page 1,
/// higher valid `txnid` wins. `None` when neither parses.
fn pick_meta(src: &dyn ByteSource, head: &[u8], variant: Variant) -> Result<Option<(Meta, u8)>> {
    let meta0 = Meta::parse(head, variant).ok();
    let candidates: Vec<u64> = match meta0 {
        Some(m) => vec![m.page_size()],
        None => FALLBACK_PAGE_SIZES.to_vec(),
    };
    let mut meta1 = None;
    for psize in candidates {
        let b = src.read_range(psize, META_LEN)?;
        if let Ok(m) = Meta::parse(&b, variant)
            && m.page_size() == psize
        {
            meta1 = Some(m);
            break;
        }
    }
    Ok(match (meta0, meta1) {
        (Some(a), Some(b)) if b.txnid > a.txnid => Some((b, 1)),
        (Some(a), _) => Some((a, 0)),
        (None, Some(b)) => Some((b, 1)),
        (None, None) => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::lmdb::testing::{catalogs, event_log};

    #[test]
    fn opens_fixture_metas() {
        for source in catalogs() {
            let env = LmdbEnv::open(&source.clone()).unwrap();
            assert_eq!(env.psize, 16384);
            assert_eq!(env.meta.main.entries, 6);
            assert!(env.meta.txnid > 0);

            let env = LmdbEnv::open(&event_log()).unwrap();
            assert_eq!(env.meta.main.entries, 24);
            assert_ne!(env.meta.main.flags & format::MDB_INTEGERKEY, 0);
        }
    }

    #[test]
    fn named_dbs_found_in_main() {
        for source in catalogs() {
            let env = LmdbEnv::open(&source.clone()).unwrap();
            let named = env.named_dbs().unwrap();
            let names: Vec<&[u8]> = named.dbs.iter().map(|d| &d.name[..]).collect();
            assert_eq!(names, [&b"blobs"[..], b"counters", b"tags", b"users"]);
            let users = &named.dbs[3].db;
            assert_eq!(
                (users.entries, users.depth, users.branch_pages),
                (600, 2, 1)
            );
            assert!(named.dbs[2].db.is_dupsort());
            assert!(!named.truncated);

            let env = LmdbEnv::open(&event_log()).unwrap();
            assert!(env.named_dbs().unwrap().dbs.is_empty());
        }
    }

    #[test]
    fn newer_meta_wins_and_damaged_meta_falls_back() {
        use crate::types::lmdb::testing::{CATALOG, CATALOG_09};
        for path in [CATALOG, CATALOG_09] {
            let bytes = std::fs::read(path).unwrap();
            let env = LmdbEnv::open(&InputSource::memory(bytes.clone(), "c")).unwrap();
            let (active, txnid) = (env.active_meta, env.meta.txnid);

            // Zero the active meta's magic: the other meta takes over.
            let mut damaged = bytes;
            let off = usize::from(active) * env.psize as usize + env.variant.header;
            damaged[off..off + 4].fill(0);
            let env = LmdbEnv::open(&InputSource::memory(damaged, "c")).unwrap();
            assert_eq!(env.active_meta, 1 - active, "{path}");
            assert!(env.meta.txnid < txnid);
        }
    }

    #[test]
    fn variants_detected() {
        use crate::types::lmdb::format::{V09, V10};
        let [v10, v09] = catalogs();
        assert_eq!(LmdbEnv::open(&v10).unwrap().variant, V10);
        assert_eq!(LmdbEnv::open(&v09).unwrap().variant, V09);
        assert_eq!(LmdbEnv::open(&event_log()).unwrap().variant, V10);
    }

    #[test]
    fn page_reads_are_bounds_checked() {
        for source in catalogs() {
            let env = LmdbEnv::open(&source.clone()).unwrap();
            assert!(env.read_page(env.page_count()).is_err());
            assert!(env.read_page(u64::MAX).is_err());
            assert!(
                env.overflow_offset(0, 1).is_err(),
                "meta page isn't overflow"
            );
        }
    }
}
