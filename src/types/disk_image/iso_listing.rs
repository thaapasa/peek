//! ISO 9660 directory walker — produces a [`Listing`](crate::viewer::listing)
//! tree for the interactive TOC view.
//!
//! Walks recursively from the root extent, parsing fixed-layout
//! directory records out of each directory's data extent. Joliet
//! filenames (UCS-2 BE) are decoded when a Supplementary Volume
//! Descriptor with a Joliet escape was present; otherwise the
//! primary 8.3 / Level-2 ASCII names are used.
//!
//! No Rock Ridge: SUSP fields aren't parsed, so per-entry Unix
//! permissions remain `None` and the renderer falls back to its
//! default-mode preview. Bounded depth + entry caps defend against
//! malformed images that loop or balloon.

use std::path::Path;

use anyhow::{Context, Result};

use super::iso_pvd::{self, PVD_OFFSET};
use crate::input::InputSource;
use crate::viewer::listing::{Entry, EntryKind, EntryMtime, time_from_epoch_secs};

/// Cap on tree depth walked. Real ISOs respect ISO 9660's 8-level
/// limit; this cap is loose enough for non-conformant images while
/// still bounding work.
const MAX_DEPTH: u32 = 32;
/// Cap on total entries collected. Stops a malformed extent from
/// generating a runaway listing.
const MAX_ENTRIES: usize = 100_000;
/// Cap on bytes read for any single directory extent.
const MAX_DIR_BYTES: u32 = 16 * 1024 * 1024;
/// Bytes of the descriptor area to read for the initial PVD parse.
/// Mirrors `info_gather::IMAGE_HEAD_SCAN`.
const DESCRIPTOR_SCAN_BYTES: usize = 16 * 1024;

pub fn list_iso(source: &InputSource) -> Result<Vec<Entry>> {
    let bs = source.open_byte_source()?;
    let head = bs.read_range(PVD_OFFSET, DESCRIPTOR_SCAN_BYTES)?;
    let extents = iso_pvd::parse_root_extents(&head)
        .context("ISO 9660 Primary Volume Descriptor not found")?;

    // Joliet preferred when present (longer Unicode names). Block
    // size from the PVD applies to extent positioning regardless of
    // which root we walk.
    let (root_lba, root_size, joliet) = match extents.joliet {
        Some((lba, size)) => (lba, size, true),
        None => (extents.primary.0, extents.primary.1, false),
    };

    let mut counter = 0usize;
    walk_directory(
        bs.as_ref(),
        root_lba,
        root_size,
        extents.block_size,
        joliet,
        0,
        &mut counter,
    )
}

/// Resolve a `/`-separated path inside the ISO to the file's byte
/// range in the underlying source. Returns the absolute offset and
/// size, or `None` when the path doesn't match a file (or names a
/// directory). Used by the extract path to map an ISO entry onto a
/// `FileRange` view without copying.
pub fn lookup_file_range(source: &InputSource, target: &Path) -> Result<Option<(u64, u64)>> {
    let bs = source.open_byte_source()?;
    let head = bs.read_range(PVD_OFFSET, DESCRIPTOR_SCAN_BYTES)?;
    let extents = iso_pvd::parse_root_extents(&head)
        .context("ISO 9660 Primary Volume Descriptor not found")?;
    let (root_lba, root_size, joliet) = match extents.joliet {
        Some((lba, size)) => (lba, size, true),
        None => (extents.primary.0, extents.primary.1, false),
    };

    let segments: Vec<&std::ffi::OsStr> = target
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s),
            _ => None,
        })
        .collect();
    if segments.is_empty() {
        return Ok(None);
    }
    let segment_strs: Vec<String> = segments
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();

    walk_for_path(
        bs.as_ref(),
        root_lba,
        root_size,
        extents.block_size,
        joliet,
        &segment_strs,
        0,
    )
}

#[allow(clippy::too_many_arguments)]
fn walk_for_path(
    bs: &dyn crate::input::source::ByteSource,
    extent_lba: u32,
    data_len: u32,
    block_size: u32,
    joliet: bool,
    segments: &[String],
    depth: u32,
) -> Result<Option<(u64, u64)>> {
    if depth > MAX_DEPTH || data_len == 0 || data_len > MAX_DIR_BYTES || segments.is_empty() {
        return Ok(None);
    }
    let offset = (extent_lba as u64).saturating_mul(block_size as u64);
    let buf = bs.read_range(offset, data_len as usize)?;
    if buf.is_empty() {
        return Ok(None);
    }
    let target = &segments[0];
    let rest = &segments[1..];
    let mut i = 0usize;
    while i < buf.len() {
        let rec_len = buf[i] as usize;
        if rec_len == 0 {
            let next = (i + block_size as usize) & !(block_size as usize - 1);
            if next <= i {
                break;
            }
            i = next;
            continue;
        }
        if i + rec_len > buf.len() || rec_len < 33 {
            break;
        }
        let rec = &buf[i..i + rec_len];
        i += rec_len;
        let Some(parsed) = parse_record(rec, joliet) else {
            continue;
        };
        if parsed.is_self_or_parent {
            continue;
        }
        if &parsed.name != target {
            continue;
        }
        if rest.is_empty() {
            // Last segment must resolve to a file.
            if parsed.is_dir {
                return Ok(None);
            }
            let abs_off = (parsed.extent_lba as u64).saturating_mul(block_size as u64);
            return Ok(Some((abs_off, parsed.data_len as u64)));
        }
        if !parsed.is_dir {
            return Ok(None);
        }
        return walk_for_path(
            bs,
            parsed.extent_lba,
            parsed.data_len,
            block_size,
            joliet,
            rest,
            depth + 1,
        );
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn walk_directory(
    bs: &dyn crate::input::source::ByteSource,
    extent_lba: u32,
    data_len: u32,
    block_size: u32,
    joliet: bool,
    depth: u32,
    counter: &mut usize,
) -> Result<Vec<Entry>> {
    if depth > MAX_DEPTH || data_len == 0 || data_len > MAX_DIR_BYTES {
        return Ok(Vec::new());
    }
    let offset = (extent_lba as u64).saturating_mul(block_size as u64);
    let buf = bs.read_range(offset, data_len as usize)?;
    if buf.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let mut i = 0usize;
    while i < buf.len() {
        if *counter >= MAX_ENTRIES {
            break;
        }
        let rec_len = buf[i] as usize;
        if rec_len == 0 {
            // Records don't span sector boundaries — the rest of the
            // current sector is zero-padded. Skip to the next sector.
            let next = (i + block_size as usize) & !(block_size as usize - 1);
            if next <= i {
                break;
            }
            i = next;
            continue;
        }
        if i + rec_len > buf.len() || rec_len < 33 {
            break;
        }
        let rec = &buf[i..i + rec_len];
        i += rec_len;

        let Some(parsed) = parse_record(rec, joliet) else {
            continue;
        };
        // Skip "." and ".." self/parent entries.
        if parsed.is_self_or_parent {
            continue;
        }
        *counter += 1;

        if parsed.is_dir {
            let children = walk_directory(
                bs,
                parsed.extent_lba,
                parsed.data_len,
                block_size,
                joliet,
                depth + 1,
                counter,
            )?;
            out.push(Entry {
                name: parsed.name,
                size: 0,
                mtime: parsed.mtime,
                mode: None,
                kind: EntryKind::Dir { children },
            });
        } else {
            out.push(Entry {
                name: parsed.name,
                size: parsed.data_len as u64,
                mtime: parsed.mtime,
                mode: None,
                kind: EntryKind::File,
            });
        }
    }
    sort(&mut out);
    Ok(out)
}

fn sort(entries: &mut [Entry]) {
    entries.sort_by(|a, b| match (a.is_dir(), b.is_dir()) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
}

struct ParsedRecord {
    name: String,
    extent_lba: u32,
    data_len: u32,
    mtime: Option<EntryMtime>,
    is_dir: bool,
    is_self_or_parent: bool,
}

fn parse_record(rec: &[u8], joliet: bool) -> Option<ParsedRecord> {
    if rec.len() < 33 {
        return None;
    }
    let extent_lba = u32::from_le_bytes([rec[2], rec[3], rec[4], rec[5]]);
    let data_len = u32::from_le_bytes([rec[10], rec[11], rec[12], rec[13]]);
    let flags = rec[25];
    let is_dir = flags & 0x02 != 0;
    let id_len = rec[32] as usize;
    if 33 + id_len > rec.len() {
        return None;
    }
    let id = &rec[33..33 + id_len];

    // ISO 9660 reserves identifiers `\x00` and `\x01` for `.` and
    // `..`. They appear as the first two records of every directory.
    let is_self_or_parent = id == [0x00] || id == [0x01];

    let name = if is_self_or_parent {
        // Won't be surfaced; placeholder.
        String::new()
    } else {
        decode_identifier(id, joliet, is_dir)
    };

    let mtime = parse_dir_record_time(&rec[18..25]);

    Some(ParsedRecord {
        name,
        extent_lba,
        data_len,
        mtime,
        is_dir,
        is_self_or_parent,
    })
}

/// Decode a directory record's file identifier. Joliet stores
/// big-endian UCS-2; primary uses ASCII (Level 1: 8.3) or extended
/// ASCII (Level 2/3). For both, file revision suffixes (`;1`, `;2`)
/// are stripped — they're a versioning artifact users don't expect to
/// see in a TOC view.
fn decode_identifier(id: &[u8], joliet: bool, is_dir: bool) -> String {
    let raw = if joliet {
        let mut s = String::with_capacity(id.len() / 2);
        for chunk in id.chunks_exact(2) {
            let cp = u16::from_be_bytes([chunk[0], chunk[1]]);
            // Replace invalid scalar values with the replacement char
            // rather than dropping them — keeps display alignment.
            s.push(char::from_u32(cp as u32).unwrap_or('\u{FFFD}'));
        }
        s
    } else {
        String::from_utf8_lossy(id).into_owned()
    };
    let trimmed = if is_dir {
        raw.as_str()
    } else {
        // Strip ";N" version suffix once. Most images use ";1".
        match raw.rfind(';') {
            Some(pos) => &raw[..pos],
            None => raw.as_str(),
        }
    };
    // Trim trailing dot left by 8.3 names with no extension
    // ("README." → "README").
    trimmed.trim_end_matches('.').to_string()
}

/// 7-byte directory record timestamp:
/// year-since-1900, month, day, hour, minute, second, GMT offset
/// in 15-minute units (signed). All-zero means unset.
fn parse_dir_record_time(buf: &[u8]) -> Option<EntryMtime> {
    if buf.len() < 7 {
        return None;
    }
    if buf.iter().all(|b| *b == 0) {
        return None;
    }
    let year = 1900u32 + buf[0] as u32;
    let month = buf[1] as u32;
    let day = buf[2] as u32;
    let hour = buf[3] as u32;
    let minute = buf[4] as u32;
    let second = buf[5] as u32;
    let offset_quarters = buf[6] as i8;

    // Convert civil time → Unix epoch seconds (UTC), then apply the
    // record's GMT offset. Days-since-epoch via the Howard Hinnant
    // algorithm — handles all valid ISO timestamps without leap-year
    // table lookups.
    let epoch_secs = civil_to_unix_secs(year as i32, month, day, hour, minute, second)?;
    let adjusted = epoch_secs.checked_sub((offset_quarters as i64) * 15 * 60)?;
    if adjusted < 0 {
        return None;
    }
    Some(EntryMtime::Utc(time_from_epoch_secs(adjusted as u64)?))
}

/// Howard Hinnant's "days_from_civil" — civil date (year/month/day)
/// to days since 1970-01-01. Public-domain algorithm.
fn civil_to_unix_secs(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u32;
    let m_adj = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * m_adj + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era as i64 * 146097 + doe as i64 - 719468;
    let secs = days
        .checked_mul(86_400)?
        .checked_add(h as i64 * 3600)?
        .checked_add(mi as i64 * 60)?
        .checked_add(s as i64)?;
    Some(secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::listing::Stats;
    use std::path::PathBuf;

    fn fixture(name: &str) -> InputSource {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("test-data");
        p.push(name);
        InputSource::File(p)
    }

    /// `sample.iso` is a Joliet-enabled hybrid ISO with two top-level
    /// files (README.txt, config.ini) plus a `sub/` directory holding
    /// `inner.txt` and a `deeper/` directory holding `deep.txt`.
    #[test]
    fn list_iso_finds_expected_entries() {
        let entries = list_iso(&fixture("sample.iso")).unwrap();
        let stats = Stats::from_root("ISO 9660 image", &entries);
        assert_eq!(stats.file_count, 4, "README, config, inner, deep");
        assert_eq!(stats.dir_count, 2, "sub, deeper");
        // README "primary\n" = 8, config "config\n" = 7, inner "leaf\n"
        // = 5, deep "deep\n" = 5. Sum = 25.
        assert_eq!(stats.total_size, 25);
    }

    /// Top-level should be sorted dirs-first, alphabetically within
    /// each group: `sub/` before any of the file leaves.
    #[test]
    fn list_iso_sorts_dirs_first() {
        let entries = list_iso(&fixture("sample.iso")).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["sub", "README.txt", "config.ini"]);
    }

    /// Joliet preserves filename case ("README.txt" not "README.TXT;1").
    /// Verifies both the Joliet preference and the version-suffix strip.
    #[test]
    fn list_iso_strips_version_suffix_and_preserves_case() {
        let entries = list_iso(&fixture("sample.iso")).unwrap();
        let has_readme = entries.iter().any(|e| e.name == "README.txt");
        assert!(
            has_readme,
            "got: {:?}",
            entries.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
    }
}
