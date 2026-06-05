//! Detection for bare COFF object files (`.obj`).
//!
//! Unlike ELF / Mach-O / PE, a relocatable COFF object has no dedicated
//! magic — it opens straight on the COFF file header, whose first field
//! is a 2-byte machine type. `object`'s own `FileKind` keys on just those
//! two bytes, which is far too loose to claim arbitrary input (and `.obj`
//! is also the Wavefront 3D extension). So this validates the whole
//! header before committing: a known machine, no optional header (the
//! mark of a relocatable object vs. a linked image), a sane section
//! count, and the executable-image flag clear.

/// COFF machine types `object` can parse, as little-endian `u16`s.
const COFF_MACHINES: &[u16] = &[
    0x014c, // i386
    0x8664, // x86-64
    0xaa64, // ARM64
    0x01c4, // ARMNT (Thumb-2)
    0xa641, // ARM64EC
];

/// `IMAGE_FILE_EXECUTABLE_IMAGE` — set on linked images, clear on the
/// relocatable objects we want here.
const IMAGE_FILE_EXECUTABLE_IMAGE: u16 = 0x0002;

/// True when `head` opens on a plausible bare COFF object header. Strict
/// by design: the 2-byte machine alone collides with too much, so every
/// fixed header field is checked.
pub fn is_bare_coff(head: &[u8]) -> bool {
    if head.len() < 20 {
        return false;
    }
    let machine = u16::from_le_bytes([head[0], head[1]]);
    if !COFF_MACHINES.contains(&machine) {
        return false;
    }
    let number_of_sections = u16::from_le_bytes([head[2], head[3]]);
    let size_of_optional_header = u16::from_le_bytes([head[16], head[17]]);
    let characteristics = u16::from_le_bytes([head[18], head[19]]);

    // A relocatable object carries no optional header; a linked image
    // does (and would start with `MZ` anyway). Section count must be
    // non-zero and within COFF's practical range. The executable-image
    // flag must be clear.
    size_of_optional_header == 0
        && (1..=96).contains(&number_of_sections)
        && characteristics & IMAGE_FILE_EXECUTABLE_IMAGE == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn read(name: &str) -> Vec<u8> {
        // Fixtures live in the workspace-root `test-data/`.
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("../../test-data");
        p.push(name);
        std::fs::read(p).unwrap()
    }

    #[test]
    fn accepts_real_bare_coff_object() {
        assert!(is_bare_coff(&read("tiny.obj")));
    }

    #[test]
    fn rejects_wavefront_obj_text() {
        // A 3D model `.obj` shares the extension but is text — its head
        // never matches a COFF machine type.
        assert!(!is_bare_coff(b"# Blender\nv 0.0 0.0 0.0\nf 1 2 3\n"));
    }

    #[test]
    fn rejects_too_short_head() {
        assert!(!is_bare_coff(&[0x64, 0x86, 0x07]));
    }

    #[test]
    fn rejects_matching_machine_with_optional_header() {
        // First two bytes look like x86-64 COFF, but a non-zero optional
        // header size marks a linked image, not a relocatable object.
        let mut head = vec![0u8; 20];
        head[0] = 0x64; // machine x86-64 (LE)
        head[1] = 0x86;
        head[2] = 1; // one section
        head[16] = 0xf0; // SizeOfOptionalHeader != 0
        assert!(!is_bare_coff(&head));
    }
}
