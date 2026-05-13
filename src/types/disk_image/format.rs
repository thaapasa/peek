//! Disk-image container format enum + display label.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskImageFormat {
    Iso,
    Dmg,
    /// Generic raw disk image (`.img` / `.bin` / `.dd`) that doesn't
    /// match a recognised filesystem header. Listing isn't supported
    /// — the info section parses the partition table when one is
    /// present, otherwise falls back to "raw image".
    Raw,
}

impl DiskImageFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Iso => "ISO 9660 image",
            Self::Dmg => "Apple Disk Image (UDIF)",
            Self::Raw => "Raw disk image",
        }
    }
}
