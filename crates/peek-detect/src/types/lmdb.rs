//! LMDB detection. An environment's data file (`data.mdb`, or any name in
//! no-subdir mode — `.lmdb`, `.mdb`, none) opens with meta page 0: a page
//! header flagged `P_META`, then the `0xBEEFC0DE` magic. LMDB 1.0 widened
//! the header from 16 to 24 bytes, so the magic sits at 16 or 24. No extension
//! rule — `.mdb` is also MS Access, and LMDB names are arbitrary — so the
//! magic decides. 32-bit and big-endian layouts are detected too; the
//! reader reports them as unsupported.

const MAGIC: u32 = 0xBEEF_C0DE;
const P_META: u16 = 0x08;

/// `(magic offset, flags offset)` per page-header layout: 0.9 64-bit
/// (16-byte header), 1.0 64-bit (24-byte) and 0.9 32-bit (12-byte).
const LAYOUTS: [(usize, usize); 3] = [(16, 10), (24, 18), (12, 6)];

/// True when `head` opens with an LMDB meta page in any byte layout.
pub fn sniff_magic(head: &[u8]) -> bool {
    LAYOUTS.iter().any(|&(magic_at, flags_at)| {
        let (Some(magic), Some(flags)) = (
            head.get(magic_at..magic_at + 4),
            head.get(flags_at..flags_at + 2),
        ) else {
            return false;
        };
        let magic: [u8; 4] = magic.try_into().unwrap_or_default();
        let flags: [u8; 2] = flags.try_into().unwrap_or_default();
        (u32::from_le_bytes(magic) == MAGIC && u16::from_le_bytes(flags) & P_META != 0)
            || (u32::from_be_bytes(magic) == MAGIC && u16::from_be_bytes(flags) & P_META != 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta_head(magic_at: usize, flags_at: usize, be: bool) -> Vec<u8> {
        let mut h = vec![0u8; 64];
        let (m, f) = if be {
            (MAGIC.to_be_bytes(), P_META.to_be_bytes())
        } else {
            (MAGIC.to_le_bytes(), P_META.to_le_bytes())
        };
        h[magic_at..magic_at + 4].copy_from_slice(&m);
        h[flags_at..flags_at + 2].copy_from_slice(&f);
        h
    }

    #[test]
    fn sniffs_every_layout() {
        assert!(sniff_magic(&meta_head(16, 10, false)));
        assert!(sniff_magic(&meta_head(16, 10, true)));
        assert!(sniff_magic(&meta_head(24, 18, false)));
        assert!(sniff_magic(&meta_head(12, 6, false)));
    }

    #[test]
    fn rejects_magic_without_meta_flag_and_short_heads() {
        let mut h = meta_head(16, 10, false);
        h[10] = 0;
        assert!(!sniff_magic(&h));
        assert!(!sniff_magic(&h[..18]));
        assert!(!sniff_magic(b""));
    }
}
