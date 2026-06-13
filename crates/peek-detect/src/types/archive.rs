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
    /// tar + brotli (`.tar.br`).
    TarBr,
    /// cpio archive (newc `070701` / `070702` or ODC `070707`).
    Cpio,
    /// cpio + gzip (`.cpio.gz`).
    CpioGz,
}

impl ArchiveFormat {
    /// Whether listing this format's table of contents requires streaming
    /// the whole *decompressed* archive — true for the compressed tar /
    /// cpio variants, whose entry data can't be seeked past without
    /// inflating it. Seekable formats (zip / 7z / plain tar / cpio / ar)
    /// read a cheap index or skip entry bytes by seeking, so they return
    /// false. Drives the latency gate: a small `.tar.gz` can expand to a
    /// huge tar, making the TOC walk slow.
    pub fn streams_compressed(self) -> bool {
        matches!(
            self,
            Self::TarGz
                | Self::TarBz2
                | Self::TarXz
                | Self::TarZst
                | Self::TarLz4
                | Self::TarBr
                | Self::CpioGz
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "ZIP archive",
            Self::Tar => "tar archive",
            Self::TarGz => "tar + gzip",
            Self::TarBz2 => "tar + bzip2",
            Self::TarXz => "tar + xz",
            Self::TarZst => "tar + zstd",
            Self::TarLz4 => "tar + lz4",
            Self::TarBr => "tar + brotli",
            Self::SevenZ => "7-Zip archive",
            Self::Ar => "ar archive",
            Self::Cpio => "cpio archive",
            Self::CpioGz => "cpio + gzip",
        }
    }
}

// Name- and magic-byte-based archive format detection. Double-extensions
// (e.g. `.tar.gz`, `.tgz`) are matched before bare ones so callers that
// sequence `archive_from_name` ahead of compression detection get the
// correct tarball classification.

/// Match a filename against archive double-extensions and single
/// archive extensions. Returns `None` for non-archive names.
/// Case-insensitive.
pub fn format_from_name(name: &str) -> Option<ArchiveFormat> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        return Some(ArchiveFormat::TarGz);
    }
    if lower.ends_with(".tar.bz2") || lower.ends_with(".tbz2") || lower.ends_with(".tbz") {
        return Some(ArchiveFormat::TarBz2);
    }
    if lower.ends_with(".tar.xz") || lower.ends_with(".txz") {
        return Some(ArchiveFormat::TarXz);
    }
    if lower.ends_with(".tar.zst") || lower.ends_with(".tzst") {
        return Some(ArchiveFormat::TarZst);
    }
    if lower.ends_with(".tar.lz4") || lower.ends_with(".tlz4") {
        return Some(ArchiveFormat::TarLz4);
    }
    if lower.ends_with(".tar.br") || lower.ends_with(".tbr") {
        return Some(ArchiveFormat::TarBr);
    }
    if lower.ends_with(".tar") {
        return Some(ArchiveFormat::Tar);
    }
    if lower.ends_with(".cpio.gz") {
        return Some(ArchiveFormat::CpioGz);
    }
    if lower.ends_with(".cpio") {
        return Some(ArchiveFormat::Cpio);
    }
    if lower.ends_with(".7z") {
        return Some(ArchiveFormat::SevenZ);
    }
    if lower.ends_with(".zip")
        || lower.ends_with(".jar")
        || lower.ends_with(".war")
        || lower.ends_with(".apk")
    {
        return Some(ArchiveFormat::Zip);
    }
    if lower.ends_with(".deb") || lower.ends_with(".ar") || lower.ends_with(".a") {
        return Some(ArchiveFormat::Ar);
    }
    None
}

/// Map an `infer` magic-byte MIME to a multi-entry archive format.
/// Bare single-stream codecs live in the compression detector.
pub fn format_from_mime(mime: &str) -> Option<ArchiveFormat> {
    match mime {
        "application/zip" => Some(ArchiveFormat::Zip),
        "application/x-tar" => Some(ArchiveFormat::Tar),
        "application/x-cpio" => Some(ArchiveFormat::Cpio),
        "application/x-7z-compressed" => Some(ArchiveFormat::SevenZ),
        _ => None,
    }
}
