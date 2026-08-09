//! CBZ container reader: list image pages out of the ZIP and pull
//! single-page bytes for the read mode.
//!
//! A CBZ is just a ZIP whose payload is one image per file plus
//! optional metadata (`ComicInfo.xml`, `cover.jpg`). The container
//! itself enforces no order — pages display in lexicographic
//! filename order, matching the convention used by every comic
//! reader in the wild.

use anyhow::Result;
use bytes::Bytes;
use peek_io::InputSource;
use zip::ZipArchive;

use crate::types::archive::reader::{self, ReadSeek};

/// Image extensions that count as a comic page. Matches what
/// established readers (Komga, Tachiyomi, ComicRack) accept; uncommon
/// formats are ignored rather than erroring.
const PAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"];

/// One page entry resolved from the ZIP central directory. Holds the
/// absolute ZIP entry name (suitable for `ZipArchive::by_name`) and
/// the uncompressed byte size for stats.
#[derive(Clone)]
pub(crate) struct Page {
    pub full_path: String,
    pub size: u64,
}

/// Walk the central directory once and collect image pages, sorted
/// by name. Skips directory entries and `__MACOSX/` resource forks
/// (added by macOS Archive Utility) so they don't pollute the spine.
pub(crate) fn list_pages(source: &InputSource) -> Result<Vec<Page>> {
    let mut zip = open_zip(source)?;
    let mut pages = Vec::new();
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name();
        if name.starts_with("__MACOSX/") || name.contains("/__MACOSX/") {
            continue;
        }
        if has_page_extension(name) {
            pages.push(Page {
                full_path: name.to_string(),
                size: entry.size(),
            });
        }
    }
    pages.sort_by(|a, b| a.full_path.cmp(&b.full_path));
    Ok(pages)
}

/// Open a fresh ZIP handle over the source. Each page read takes
/// one — keeping a single archive across calls would require
/// carrying a mutable reader through the mode, which doesn't pay
/// for itself at the page-cadence the user actually navigates at.
pub(crate) fn open_zip(source: &InputSource) -> Result<ZipArchive<Box<dyn ReadSeek>>> {
    reader::open_zip(source, "CBZ")
}

/// Read one page's bytes out of the ZIP. Cap-gated against zip bombs
/// via the shared archive helper.
pub(crate) fn read_page(zip: &mut ZipArchive<Box<dyn ReadSeek>>, path: &str) -> Result<Bytes> {
    reader::read_zip_entry(zip, path, "CBZ")
}

fn has_page_extension(name: &str) -> bool {
    let basename = name.rsplit('/').next().unwrap_or(name);
    let Some(dot) = basename.rfind('.') else {
        return false;
    };
    let ext = basename[dot + 1..].to_ascii_lowercase();
    PAGE_EXTENSIONS.iter().any(|e| *e == ext)
}
