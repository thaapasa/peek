//! WOFF 1.0 → sfnt unwrap. WOFF wraps an ordinary OpenType font: the
//! 12-byte sfnt offset table and the per-table directory are replaced
//! with a WOFF header + directory, and each table's payload is stored
//! either verbatim or zlib-compressed (RFC 1950). Reconstructing the
//! sfnt means rebuilding the offset table + directory and inflating
//! each table back to its original length.
//!
//! ttf-parser and fontdue both operate on raw sfnt and know nothing
//! about WOFF, so this runs once up front (see
//! [`crate::types::font::sfnt::decode`]) and every downstream consumer
//! sees a plain in-memory sfnt.
//!
//! The metadata / private blocks (optional XML + vendor data trailing
//! the tables) are dropped — they carry no glyph or `name`-table data
//! and the sfnt has no place for them.
//!
//! Spec: <https://www.w3.org/TR/WOFF/>.

use std::io::Read;

use anyhow::{Result, anyhow, bail};
use flate2::read::ZlibDecoder;

/// WOFF header signature.
const WOFF_SIGNATURE: &[u8; 4] = b"wOFF";
/// Bytes in the WOFF header preceding the table directory.
const WOFF_HEADER_LEN: usize = 44;
/// Bytes per WOFF table-directory entry.
const WOFF_ENTRY_LEN: usize = 20;
/// Bytes in an sfnt offset table (the rebuilt header).
const SFNT_OFFSET_TABLE_LEN: usize = 12;
/// Bytes per sfnt table-directory entry.
const SFNT_ENTRY_LEN: usize = 16;

struct TableEntry {
    tag: u32,
    offset: usize,
    comp_length: usize,
    orig_length: usize,
    orig_checksum: u32,
}

/// Decode a WOFF 1.0 buffer into its inner sfnt bytes. Returns an error
/// for a malformed or truncated container — the caller falls back to
/// the binary / Hex view rather than surfacing a half-built font.
pub fn decode(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() < WOFF_HEADER_LEN {
        bail!("WOFF: truncated header ({} bytes)", bytes.len());
    }
    if &bytes[0..4] != WOFF_SIGNATURE {
        bail!("WOFF: bad signature");
    }

    let flavor = read_u32(bytes, 4);
    let num_tables = read_u16(bytes, 12) as usize;
    // totalSfntSize (offset 16) is advisory — we compute our own
    // layout — but a zero table count means there's nothing to rebuild.
    if num_tables == 0 {
        bail!("WOFF: no tables");
    }

    let dir_end = WOFF_HEADER_LEN
        .checked_add(
            num_tables
                .checked_mul(WOFF_ENTRY_LEN)
                .ok_or_else(overflow)?,
        )
        .ok_or_else(overflow)?;
    if bytes.len() < dir_end {
        bail!("WOFF: truncated table directory");
    }

    let mut entries = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let base = WOFF_HEADER_LEN + i * WOFF_ENTRY_LEN;
        let entry = TableEntry {
            tag: read_u32(bytes, base),
            offset: read_u32(bytes, base + 4) as usize,
            comp_length: read_u32(bytes, base + 8) as usize,
            orig_length: read_u32(bytes, base + 12) as usize,
            orig_checksum: read_u32(bytes, base + 16),
        };
        // Bounds-check the payload slice now so the rebuild loop can
        // index freely.
        let payload_end = entry
            .offset
            .checked_add(entry.comp_length)
            .ok_or_else(overflow)?;
        if payload_end > bytes.len() {
            bail!("WOFF: table {} payload out of bounds", i);
        }
        if entry.comp_length > entry.orig_length {
            bail!("WOFF: table {} compressed larger than original", i);
        }
        entries.push(entry);
    }

    // sfnt layout: offset table, then the directory, then each table's
    // payload padded to a 4-byte boundary. Tables keep WOFF directory
    // order (already tag-sorted in practice; parsers re-sort anyway).
    let mut total = SFNT_OFFSET_TABLE_LEN + num_tables * SFNT_ENTRY_LEN;
    let mut table_offsets = Vec::with_capacity(num_tables);
    for entry in &entries {
        table_offsets.push(total);
        total = total
            .checked_add(pad4(entry.orig_length))
            .ok_or_else(overflow)?;
    }

    let mut out = vec![0u8; total];

    // Offset table.
    write_u32(&mut out, 0, flavor);
    write_u16(&mut out, 4, num_tables as u16);
    let (search_range, entry_selector, range_shift) = search_params(num_tables);
    write_u16(&mut out, 6, search_range);
    write_u16(&mut out, 8, entry_selector);
    write_u16(&mut out, 10, range_shift);

    // Directory + table payloads.
    for (i, entry) in entries.iter().enumerate() {
        let dir = SFNT_OFFSET_TABLE_LEN + i * SFNT_ENTRY_LEN;
        let dest = table_offsets[i];
        write_u32(&mut out, dir, entry.tag);
        write_u32(&mut out, dir + 4, entry.orig_checksum);
        write_u32(&mut out, dir + 8, dest as u32);
        write_u32(&mut out, dir + 12, entry.orig_length as u32);

        let src = &bytes[entry.offset..entry.offset + entry.comp_length];
        let slot = &mut out[dest..dest + entry.orig_length];
        if entry.comp_length == entry.orig_length {
            // Stored verbatim — WOFF leaves a table uncompressed when
            // zlib wouldn't shrink it.
            slot.copy_from_slice(src);
        } else {
            inflate_exact(src, slot)
                .map_err(|e| anyhow!("WOFF: table {} inflate failed: {e}", i))?;
        }
        // The padding bytes between `dest + orig_length` and the next
        // table are already zero from the `vec![0u8; total]` init.
    }

    Ok(out)
}

/// Inflate `src` (zlib stream) into `dst`, requiring it to fill `dst`
/// exactly. A short or long stream means the directory's `origLength`
/// disagrees with the payload — treat as corruption.
fn inflate_exact(src: &[u8], dst: &mut [u8]) -> Result<()> {
    let mut decoder = ZlibDecoder::new(src);
    let mut written = 0;
    while written < dst.len() {
        let n = decoder.read(&mut dst[written..])?;
        if n == 0 {
            bail!("stream ended {} bytes short", dst.len() - written);
        }
        written += n;
    }
    // Confirm nothing trails — exactly origLength bytes expected.
    let mut extra = [0u8; 1];
    if decoder.read(&mut extra)? != 0 {
        bail!("stream longer than declared length");
    }
    Ok(())
}

/// sfnt offset-table search params. Advisory hints (parsers recompute),
/// but emitting the spec values keeps the rebuilt font byte-clean for
/// any validator that checks them. `searchRange = 16 * 2^floor(log2 n)`,
/// `entrySelector = floor(log2 n)`, `rangeShift = 16n - searchRange`.
///
/// The two `* 16` products are u16 fields that overflow past 4095 tables;
/// no real font comes close (table counts are tens), but compute in usize
/// and saturate so an adversarial `numTables` yields a bounded value
/// rather than a wrapped one. The fields stay advisory either way.
fn search_params(num_tables: usize) -> (u16, u16, u16) {
    let mut entry_selector = 0u16;
    let mut largest_pow2 = 1usize;
    while largest_pow2 * 2 <= num_tables {
        largest_pow2 *= 2;
        entry_selector += 1;
    }
    let search_range = largest_pow2 * 16;
    let range_shift = num_tables * 16 - search_range;
    (
        search_range.min(u16::MAX as usize) as u16,
        entry_selector,
        range_shift.min(u16::MAX as usize) as u16,
    )
}

/// Round up to the next 4-byte boundary (sfnt tables are 4-aligned).
fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

fn overflow() -> anyhow::Error {
    anyhow!("WOFF: size arithmetic overflow")
}

fn read_u16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn read_u32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn write_u16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_be_bytes());
}

fn write_u32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_params_match_spec_examples() {
        // 9 tables: largest pow2 ≤ 9 is 8 → selector 3, range 128,
        // shift 16*9-128 = 16.
        assert_eq!(search_params(9), (128, 3, 16));
        // 1 table: range 16, selector 0, shift 0.
        assert_eq!(search_params(1), (16, 0, 0));
        // 16 tables: range 256, selector 4, shift 0.
        assert_eq!(search_params(16), (256, 4, 0));
    }

    #[test]
    fn search_params_saturate_past_u16() {
        // > 4095 tables overflow the u16 `* 16` products; saturate rather
        // than wrap. No real font reaches this — adversarial-input guard.
        let (range, selector, shift) = search_params(u16::MAX as usize);
        assert_eq!(range, u16::MAX); // 32768*16 = 524288, saturated
        assert_eq!(selector, 15); // floor(log2(65535))
        assert_eq!(shift, u16::MAX); // 65535*16 - 524288 = 524272, saturated
    }

    #[test]
    fn rejects_non_woff() {
        assert!(decode(b"OTTO\x00\x00\x00\x00").is_err());
        assert!(decode(b"short").is_err());
    }

    /// Round-trip the bundled WOFF fixture against the source TTF it was
    /// generated from: the rebuilt sfnt must parse and carry the same
    /// glyph count + family name as the original.
    #[test]
    fn decodes_sacramento_to_parseable_sfnt() {
        let woff = std::fs::read("test-data/fonts/sacramento/Sacramento-Regular.woff")
            .expect("woff fixture present");
        let sfnt = decode(&woff).expect("decode succeeds");

        // Rebuilt header must be a TrueType sfnt (flavor 0x00010000).
        assert_eq!(&sfnt[0..4], &[0x00, 0x01, 0x00, 0x00]);

        let woff_info =
            crate::types::font::info_gather::gather(&sfnt, crate::types::font::FontFormat::Woff);
        let ttf = std::fs::read("test-data/fonts/sacramento/Sacramento-Regular.ttf")
            .expect("ttf fixture present");
        let ttf_info =
            crate::types::font::info_gather::gather(&ttf, crate::types::font::FontFormat::TrueType);

        assert!(
            woff_info.parse_errors.is_empty(),
            "{:?}",
            woff_info.parse_errors
        );
        assert_eq!(woff_info.faces[0].family, "Sacramento");
        assert_eq!(
            woff_info.faces[0].glyph_count,
            ttf_info.faces[0].glyph_count
        );
        assert_eq!(
            woff_info.faces[0].codepoint_count,
            ttf_info.faces[0].codepoint_count
        );
    }
}
