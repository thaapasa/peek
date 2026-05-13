//! Name- and magic-byte-based archive format detection. Double-extensions
//! (e.g. `.tar.gz`, `.tgz`) are matched before bare ones so callers that
//! sequence `archive_from_name` ahead of compression detection get the
//! correct tarball classification.

use super::format::ArchiveFormat;

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
