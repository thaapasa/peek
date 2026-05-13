//! Archive container format enum + display label.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
    SevenZ,
    /// Unix `ar(1)` archive — used by `.deb` packages (Debian binary
    /// package layout: `debian-binary`, `control.tar.*`, `data.tar.*`).
    Ar,
    /// tar + lz4 frame (`.tar.lz4`).
    TarLz4,
    /// cpio archive (newc `070701` / `070702` or ODC `070707`).
    Cpio,
    /// cpio + gzip (`.cpio.gz`).
    CpioGz,
}

impl ArchiveFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "ZIP archive",
            Self::Tar => "tar archive",
            Self::TarGz => "tar + gzip",
            Self::TarBz2 => "tar + bzip2",
            Self::TarXz => "tar + xz",
            Self::TarZst => "tar + zstd",
            Self::TarLz4 => "tar + lz4",
            Self::SevenZ => "7-Zip archive",
            Self::Ar => "ar archive",
            Self::Cpio => "cpio archive",
            Self::CpioGz => "cpio + gzip",
        }
    }
}
