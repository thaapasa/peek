//! `.DS_Store` detection — Apple Finder's per-folder "Desktop Services
//! Store".
//!
//! The file is a Buddy-allocator container (Mark "Bud1" format) that
//! holds Finder view settings keyed by filename. It carries a fixed
//! 8-byte signature — a `0x00000001` alignment word followed by the
//! ASCII tag `Bud1` — which `infer` doesn't classify, so the explicit
//! magic probe is what routes both renamed files and stdin-piped bytes
//! to the viewer. The canonical on-disk name is the literal `.DS_Store`
//! (a dotfile with no real extension), matched separately by name.

/// Leading signature of every `.DS_Store`: a `0x00000001` alignment
/// word followed by the ASCII allocator tag `Bud1`.
const DS_STORE_MAGIC: &[u8; 8] = b"\x00\x00\x00\x01Bud1";

/// True when `head` opens with the Buddy-allocator `Bud1` signature.
pub fn sniff_magic(head: &[u8]) -> bool {
    head.len() >= DS_STORE_MAGIC.len() && &head[..DS_STORE_MAGIC.len()] == DS_STORE_MAGIC
}

/// True when `name` is the canonical `.DS_Store` filename (case-folded —
/// the file is also seen as `.ds_store` on case-insensitive volumes).
pub fn is_ds_store_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(".DS_Store")
}
