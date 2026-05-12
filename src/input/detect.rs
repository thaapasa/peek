use std::fs;
use std::io::Read;
use std::path::Path;

use anyhow::{Result, bail};

use crate::input::InputSource;
use crate::input::mime;

/// Bytes read from the head of a file for magic-byte detection. `infer`
/// inspects only the first few hundred bytes; 16 KB is comfortable headroom.
const HEAD_BYTES: usize = 16 * 1024;

/// Chunk size for streaming UTF-8 validation of the file body.
const SCAN_CHUNK: usize = 64 * 1024;

/// Detected file type, used to dispatch to the right viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileType {
    /// Source code or text file with optional syntax name
    SourceCode { syntax: Option<String> },
    /// Structured data format
    Structured(StructuredFormat),
    /// Raster image
    Image,
    /// SVG vector image (rasterized for preview, XML source for raw view)
    Svg,
    /// HTML document (rendered text view via html2text, XML source for raw view)
    Html,
    /// E-book (EPUB = ZIP container with HTML chapters + OPF
    /// metadata). Drives a per-chapter rendered read mode plus the
    /// container's listing TOC.
    Ebook(EbookFormat),
    /// Comic-archive (one image per page in a ZIP / RAR / 7z / tar
    /// container). Drives the paged-image read mode.
    Comic(ComicFormat),
    /// Word-style document (DOCX = ZIP of XML, RTF = control-word
    /// markup). Drives a styled-text read view; DOCX additionally
    /// exposes the ZIP listing TOC and per-entry extract.
    Document(DocumentFormat),
    /// PDF document. Drives a paged-image render mode + text-extraction
    /// view + embedded-files listing.
    Pdf,
    /// Container archive (zip / tar / compressed tar). Drives the
    /// listing-only TOC viewer — no payload decompression.
    Archive(ArchiveFormat),
    /// Bare single-stream compressed file (`.gz` / `.bz2` / `.xz` /
    /// `.zst` / `.lz4`). Transparently decompressed by `compose_modes`
    /// — the user sees the inner content rendered as its real type,
    /// and the info section surfaces a Compression row.
    Compressed(CompressionFormat),
    /// Disk image (ISO / DMG / etc). Drives a metadata-only info view —
    /// volume descriptor / trailer parsing, no filesystem walk.
    DiskImage(DiskImageFormat),
    /// Filesystem directory. One-level listing view. Selecting a child
    /// file descends into peek; selecting a child directory re-targets
    /// the current frame (no stack of directories).
    Directory,
    /// Sound / music file. Drives a metadata-only info view —
    /// container / codec / channels / bit depth / sample rate + tag
    /// fields (title / artist / album / etc). No playback.
    Audio(AudioFormat),
    /// Binary / unknown
    Binary,
}

/// Sound-file container. Encompasses the common consumer audio formats;
/// the symphonia probe resolves codec details (e.g. ALAC inside an
/// M4a container) on top of this container-level classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    Flac,
    /// Ogg container — usually Vorbis, sometimes FLAC.
    Ogg,
    /// Ogg container carrying an Opus stream (`.opus`).
    Opus,
    Wav,
    /// MPEG-4 audio container (`.m4a` / `.m4b` / `.mp4` audio-only /
    /// `.aac` in ADTS).
    M4a,
    /// Raw AAC ADTS stream (`.aac`).
    Aac,
    /// Audio Interchange File Format (`.aiff` / `.aif`).
    Aiff,
    /// Apple Core Audio Format (`.caf`).
    Caf,
    /// Matroska audio (`.mka`).
    Mka,
    /// Windows Media Audio (`.wma`).
    Wma,
}

impl AudioFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mp3 => "MP3",
            Self::Flac => "FLAC",
            Self::Ogg => "Ogg",
            Self::Opus => "Opus",
            Self::Wav => "WAV",
            Self::M4a => "MPEG-4 audio",
            Self::Aac => "AAC",
            Self::Aiff => "AIFF",
            Self::Caf => "CAF",
            Self::Mka => "Matroska audio",
            Self::Wma => "WMA",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredFormat {
    Json,
    /// JSON with comments (VS Code flavor): `//` and `/* … */` allowed.
    Jsonc,
    /// JSON5: comments, unquoted keys, trailing commas, single quotes, hex.
    Json5,
    /// JSON Lines / NDJSON: one JSON value per line.
    Jsonl,
    Yaml,
    Toml,
    Xml,
}

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

/// Bare single-stream compression codec. Detected when the file has
/// only one of these as its outer wrapper (e.g. `notes.txt.gz`); the
/// viewer transparently decompresses and renders the inner content as
/// whatever it actually is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionFormat {
    /// gzip stream (`.gz`).
    Gz,
    /// bzip2 stream (`.bz2`).
    Bz2,
    /// xz / LZMA2 stream (`.xz`).
    Xz,
    /// zstd stream (`.zst`).
    Zst,
    /// lz4 frame stream (`.lz4`).
    Lz4,
}

impl CompressionFormat {
    /// Short codec name for the info-section Compression row.
    pub fn codec_label(self) -> &'static str {
        match self {
            Self::Gz => "gzip",
            Self::Bz2 => "bzip2",
            Self::Xz => "xz",
            Self::Zst => "zstd",
            Self::Lz4 => "lz4",
        }
    }

    /// Lowercased filename suffix used for name-strip when building the
    /// in-memory decompressed source's name.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Gz => ".gz",
            Self::Bz2 => ".bz2",
            Self::Xz => ".xz",
            Self::Zst => ".zst",
            Self::Lz4 => ".lz4",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComicFormat {
    /// Comic Book ZIP (most common comic-archive form in the wild).
    Cbz,
}

impl ComicFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cbz => "Comic Book ZIP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EbookFormat {
    /// EPUB — ZIP container with HTML chapters + OPF metadata.
    Epub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentFormat {
    /// Office Open XML word-processing document. ZIP container with
    /// `word/document.xml` body + `docProps/*.xml` metadata.
    Docx,
    /// OpenDocument Text. ZIP container with `content.xml` body and
    /// `meta.xml` Dublin Core metadata.
    Odt,
    /// Rich Text Format. Control-word markup; single file, not a
    /// container.
    Rtf,
}

impl DocumentFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Docx => "DOCX document",
            Self::Odt => "ODT document",
            Self::Rtf => "RTF document",
        }
    }
}

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

/// Result of file-type detection. Carries the magic-byte MIME forward so
/// `info::gather` doesn't need to re-read the file and re-run `infer`.
#[derive(Debug, Clone)]
pub struct Detected {
    pub file_type: FileType,
    /// MIME type from `infer::get` magic-byte detection. `None` when the
    /// file's leading bytes don't match any format `infer` recognizes
    /// (true for plain-text source code, structured text files, etc.).
    pub magic_mime: Option<String>,
    /// Set when this `Detected` describes the inner content of a
    /// transparently-decompressed bare-codec source. Set by
    /// `compose_modes` after a successful decompression so the info
    /// view can render a Compression row; carries the failure reason
    /// when decompression bombed (Hex fallback path).
    pub decompressed_from: Option<DecompressionContext>,
}

/// Metadata about the compressed outer source that produced an inner
/// `Detected`. Threaded through `Detected.decompressed_from`.
#[derive(Debug, Clone)]
pub struct DecompressionContext {
    pub codec: CompressionFormat,
    /// Compressed size of the outer stream (file size, or length of
    /// the stdin buffer).
    pub compressed_size: u64,
    /// Outer file name (`notes.txt.gz`). The inner Memory source's
    /// name is the suffix-stripped form.
    pub outer_name: String,
    /// Decompression error when the codec couldn't materialise inner
    /// bytes — viewer falls back to Hex view on the raw compressed
    /// source and Info surfaces this string as a warning.
    pub error: Option<String>,
}

impl Detected {
    /// Build a `Detected` for non-decompressed sources. The
    /// `decompressed_from` field defaults to `None`; only
    /// `compose_modes` sets it (after a transparent decompression).
    pub fn new(file_type: FileType, magic_mime: Option<String>) -> Self {
        Self {
            file_type,
            magic_mime,
            decompressed_from: None,
        }
    }
}

/// Detect the file type of an input source.
pub fn detect(source: &InputSource) -> Result<Detected> {
    detect_with(source, false)
}

/// Re-detect ignoring the source's path / entry name. Used as a
/// fallback retry when rendering fails — if the file's extension lied
/// about the content, magic-byte detection on the body still resolves
/// the real type.
pub fn detect_ignore_name(source: &InputSource) -> Result<Detected> {
    detect_with(source, true)
}

fn detect_with(source: &InputSource, ignore_name: bool) -> Result<Detected> {
    match source {
        InputSource::File(path) => detect_file(path, ignore_name),
        InputSource::Memory { bytes, name } => Ok(detect_bytes_named(
            bytes,
            if ignore_name {
                None
            } else {
                Some(name.as_str())
            },
        )),
        InputSource::FileRange { name, .. } => {
            let buf = source.read_bytes()?;
            Ok(detect_bytes_named(
                &buf,
                if ignore_name {
                    None
                } else {
                    Some(name.as_str())
                },
            ))
        }
    }
}

fn detect_file(path: &Path, ignore_name: bool) -> Result<Detected> {
    if !path.exists() {
        bail!("file not found: {}", path.display());
    }

    // Directories get their own one-level listing viewer; everything
    // below assumes a regular file we can read bytes from.
    if path.is_dir() {
        return Ok(Detected::new(FileType::Directory, None));
    }

    // Read just the head for magic-byte detection — `infer` only inspects
    // the first few hundred bytes, so we never need the whole file. Done
    // up front (before extension routing) so the magic-byte MIME flows
    // into `Detected.magic_mime` even when the extension is what picks
    // the viewer. Downstream info section uses both to flag
    // extension/MIME mismatches.
    let mut file = fs::File::open(path)?;
    let mut head = vec![0u8; HEAD_BYTES];
    let n = read_fill(&mut file, &mut head)?;
    head.truncate(n);
    let head_magic = head_magic_mime(&head);

    // Name-based routing: extension / full-name → FileType. ISO probe
    // upgrades a `.img` Raw to Iso when the body carries the PVD.
    if !ignore_name
        && let Some(name) = path.file_name().and_then(|n| n.to_str())
        && let Some(file_type) = classify_by_name(name)
    {
        return Ok(Detected::new(
            upgrade_disk_image_path(file_type, path),
            head_magic,
        ));
    }

    let magic_mime = head_magic;
    if let Some(ref mime) = magic_mime
        && let Some(file_type) = file_type_from_magic_mime(mime)
    {
        return Ok(Detected::new(file_type, magic_mime));
    }

    // Content-sniff the head (cheap, ASCII-pattern based) BEFORE
    // streaming the whole body for UTF-8 validation — sniffing only
    // needs the head bytes, and the result fills in `magic_mime` for
    // text formats `infer` doesn't classify (SVG / HTML / XML / JSON /
    // YAML). Compute now so the head buffer can move into the streaming
    // UTF-8 check below.
    let sniffed = std::str::from_utf8(&head).ok().and_then(sniff_text_content);

    // Stream the file body to check for non-UTF-8 content. Reuses the head
    // buffer as the first chunk so we don't read it twice.
    if !is_utf8_streaming(head, &mut file)? {
        return Ok(Detected::new(FileType::Binary, magic_mime));
    }

    if let Some((file_type, content_mime)) = sniffed {
        return Ok(Detected::new(
            file_type,
            magic_mime.or_else(|| Some(content_mime.to_string())),
        ));
    }

    // It's a text file — use extension as syntax hint (unless we're
    // ignoring the name on a fallback retry).
    let syntax = if ignore_name {
        None
    } else {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
    };

    Ok(Detected::new(FileType::SourceCode { syntax }, magic_mime))
}

/// Magic-byte MIME for a file head. Combines the explicit AR / RTF /
/// PDF prefix probes (which `infer` doesn't classify) with
/// `infer::get`. Returned MIME flows into `Detected.magic_mime` so the
/// info section can flag extension/MIME mismatches even when the
/// extension was the thing that picked the viewer.
fn head_magic_mime(head: &[u8]) -> Option<String> {
    if head.len() >= AR_MAGIC.len() && &head[..AR_MAGIC.len()] == AR_MAGIC {
        return Some("application/x-archive".to_string());
    }
    if head.starts_with(RTF_MAGIC) {
        return Some("application/rtf".to_string());
    }
    if head.starts_with(PDF_MAGIC) {
        return Some("application/pdf".to_string());
    }
    if head.len() >= 6
        && (&head[..6] == CPIO_NEWC_MAGIC
            || &head[..6] == CPIO_CRC_MAGIC
            || &head[..6] == CPIO_ODC_MAGIC)
    {
        return Some("application/x-cpio".to_string());
    }
    if head.len() >= 4 && &head[..4] == LZ4_FRAME_MAGIC {
        return Some("application/x-lz4".to_string());
    }
    infer::get(head).map(|k| k.mime_type().to_string())
}

/// Map a magic-byte MIME to a `FileType`. Single source of truth for
/// the magic-byte → viewer mapping; consumed by both the file and
/// byte detection paths so the rule stays consistent across sources.
/// Returns `None` for MIMEs we don't classify (caller falls through
/// to content sniffing / source-code defaults).
fn file_type_from_magic_mime(mime: &str) -> Option<FileType> {
    if mime == "application/x-archive" {
        return Some(FileType::Archive(ArchiveFormat::Ar));
    }
    if mime == "application/rtf" {
        return Some(FileType::Document(DocumentFormat::Rtf));
    }
    if mime == "application/pdf" {
        return Some(FileType::Pdf);
    }
    if mime == "image/svg+xml" {
        return Some(FileType::Svg);
    }
    if mime.starts_with("image/") {
        return Some(FileType::Image);
    }
    if let Some(fmt) = archive_format_from_mime(mime) {
        return Some(FileType::Archive(fmt));
    }
    if let Some(fmt) = compression_format_from_mime(mime) {
        return Some(FileType::Compressed(fmt));
    }
    if let Some(fmt) = audio_format_from_mime(mime) {
        return Some(FileType::Audio(fmt));
    }
    if mime.starts_with("video/") || mime.starts_with("application/x-executable") {
        return Some(FileType::Binary);
    }
    None
}

/// Map an `infer` magic-byte MIME to an audio container format. Covers
/// the formats `infer` recognises for audio; other audio MIMEs (rare)
/// fall through to plain `audio/*` → Binary above is unreachable thanks
/// to the catch-all here returning a Format::* for any audio/* it
/// knows.
fn audio_format_from_mime(mime: &str) -> Option<AudioFormat> {
    match mime {
        "audio/mpeg" | "audio/mp3" => Some(AudioFormat::Mp3),
        "audio/flac" | "audio/x-flac" => Some(AudioFormat::Flac),
        "audio/ogg" | "application/ogg" => Some(AudioFormat::Ogg),
        "audio/opus" => Some(AudioFormat::Opus),
        "audio/wav" | "audio/wave" | "audio/x-wav" => Some(AudioFormat::Wav),
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => Some(AudioFormat::M4a),
        "audio/aac" => Some(AudioFormat::Aac),
        "audio/aiff" | "audio/x-aiff" => Some(AudioFormat::Aiff),
        "audio/x-caf" => Some(AudioFormat::Caf),
        "audio/x-matroska" => Some(AudioFormat::Mka),
        "audio/x-ms-wma" => Some(AudioFormat::Wma),
        _ => None,
    }
}

/// Upgrade an `.img`/`.bin`/`.dd`-derived `DiskImage::Raw` to
/// `DiskImage::Iso` when the byte buffer carries an ISO 9660 PVD at
/// offset 32768. Byte form (used by Memory / FileRange sources).
fn upgrade_disk_image_bytes(file_type: FileType, data: &[u8]) -> FileType {
    if matches!(file_type, FileType::DiskImage(DiskImageFormat::Raw))
        && data.len() >= 32774
        && data[32768] == 1
        && &data[32769..32774] == b"CD001"
    {
        return FileType::DiskImage(DiskImageFormat::Iso);
    }
    file_type
}

/// Path form of [`upgrade_disk_image_bytes`] — opens the file and
/// reads the 6-byte PVD signature without slurping the whole image.
fn upgrade_disk_image_path(file_type: FileType, path: &Path) -> FileType {
    if matches!(file_type, FileType::DiskImage(DiskImageFormat::Raw))
        && matches!(probe_iso_or_raw(path), Some(DiskImageFormat::Iso))
    {
        return FileType::DiskImage(DiskImageFormat::Iso);
    }
    file_type
}

/// Read into `buf` until full or EOF. Returns the number of bytes read.
/// Unlike `Read::read`, this loops until the buffer is full or the source
/// is exhausted, so partial syscall returns don't truncate the head.
fn read_fill<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// Streaming UTF-8 validation. `head` is the already-read leading chunk;
/// the rest is pulled from `reader` in `SCAN_CHUNK`-sized pieces. The
/// running buffer carries any incomplete trailing UTF-8 sequence (≤3 bytes)
/// across chunk boundaries so multi-byte characters that straddle a chunk
/// boundary are validated correctly.
fn is_utf8_streaming<R: Read>(head: Vec<u8>, reader: &mut R) -> Result<bool> {
    let mut buf = head;
    let mut chunk = vec![0u8; SCAN_CHUNK];
    loop {
        match std::str::from_utf8(&buf) {
            Ok(_) => buf.clear(),
            Err(e) => {
                if e.error_len().is_some() {
                    // A genuine invalid sequence — not text.
                    return Ok(false);
                }
                // Incomplete trailing sequence; drop everything before it
                // and let the next chunk complete it.
                let valid_up_to = e.valid_up_to();
                buf.drain(..valid_up_to);
            }
        }
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            // EOF — anything still buffered is an unfinished sequence.
            return Ok(buf.is_empty());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Match a filename against archive double-extensions (e.g. `.tar.gz`,
/// `.tgz`) and single archive extensions (e.g. `.zip`). Returns `None`
/// for non-archive names. Case-insensitive.
fn archive_format_from_name(name: &str) -> Option<ArchiveFormat> {
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

/// Match a filename against bare single-stream compression
/// extensions. Returns `None` for non-compression names. The caller
/// (`classify_by_name`) checks `archive_format_from_name` first so
/// double-extensions like `.tar.gz` route to `ArchiveFormat::TarGz`
/// before bare `.gz` is considered.
fn compression_format_from_name(name: &str) -> Option<CompressionFormat> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".gz") {
        return Some(CompressionFormat::Gz);
    }
    if lower.ends_with(".bz2") {
        return Some(CompressionFormat::Bz2);
    }
    if lower.ends_with(".xz") {
        return Some(CompressionFormat::Xz);
    }
    if lower.ends_with(".zst") {
        return Some(CompressionFormat::Zst);
    }
    if lower.ends_with(".lz4") {
        return Some(CompressionFormat::Lz4);
    }
    None
}

/// Map a single file extension to an audio container format.
fn audio_format_from_ext(ext: &str) -> Option<AudioFormat> {
    match ext {
        "mp3" => Some(AudioFormat::Mp3),
        "flac" => Some(AudioFormat::Flac),
        "ogg" | "oga" => Some(AudioFormat::Ogg),
        "opus" => Some(AudioFormat::Opus),
        "wav" | "wave" => Some(AudioFormat::Wav),
        "m4a" | "m4b" | "m4p" => Some(AudioFormat::M4a),
        "aac" => Some(AudioFormat::Aac),
        "aiff" | "aif" | "aifc" => Some(AudioFormat::Aiff),
        "caf" => Some(AudioFormat::Caf),
        "mka" => Some(AudioFormat::Mka),
        "wma" => Some(AudioFormat::Wma),
        _ => None,
    }
}

/// Map a single file extension to a comic-archive format.
fn comic_format_from_ext(ext: &str) -> Option<ComicFormat> {
    match ext {
        "cbz" => Some(ComicFormat::Cbz),
        _ => None,
    }
}

/// Map a single file extension to a disk-image format. `.img` /
/// `.bin` / `.dd` map to `Raw` provisionally; the caller probes for
/// an ISO 9660 PVD before committing to that classification.
fn disk_image_format_from_ext(ext: &str) -> Option<DiskImageFormat> {
    match ext {
        "iso" => Some(DiskImageFormat::Iso),
        "dmg" => Some(DiskImageFormat::Dmg),
        "img" | "bin" | "dd" => Some(DiskImageFormat::Raw),
        _ => None,
    }
}

/// Read 6 bytes at the ISO 9660 PVD location (offset 32768 + 0..=5)
/// and check for the `\x01CD001` signature. Returns
/// `Some(DiskImageFormat::Iso)` on match, `None` otherwise (caller
/// falls back to `Raw`).
fn probe_iso_or_raw(path: &Path) -> Option<DiskImageFormat> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(32768)).ok()?;
    let mut buf = [0u8; 6];
    file.read_exact(&mut buf).ok()?;
    if buf[0] == 1 && &buf[1..6] == b"CD001" {
        Some(DiskImageFormat::Iso)
    } else {
        None
    }
}

/// Map an `infer` magic-byte MIME to a multi-entry archive format.
/// Bare single-stream codecs live in [`compression_format_from_mime`].
fn archive_format_from_mime(mime: &str) -> Option<ArchiveFormat> {
    match mime {
        "application/zip" => Some(ArchiveFormat::Zip),
        "application/x-tar" => Some(ArchiveFormat::Tar),
        "application/x-cpio" => Some(ArchiveFormat::Cpio),
        "application/x-7z-compressed" => Some(ArchiveFormat::SevenZ),
        _ => None,
    }
}

/// Map an `infer` magic-byte MIME to a single-stream compression codec.
fn compression_format_from_mime(mime: &str) -> Option<CompressionFormat> {
    match mime {
        "application/gzip" | "application/x-gzip" => Some(CompressionFormat::Gz),
        "application/x-bzip2" | "application/x-bzip" => Some(CompressionFormat::Bz2),
        "application/x-xz" => Some(CompressionFormat::Xz),
        "application/zstd" | "application/x-zstd" => Some(CompressionFormat::Zst),
        "application/x-lz4" => Some(CompressionFormat::Lz4),
        _ => None,
    }
}

/// 8-byte ar archive magic — `!<arch>\n`. `infer` doesn't recognise
/// ar; without an explicit check, stdin-piped `.deb` files would
/// classify as binary.
const AR_MAGIC: &[u8; 8] = b"!<arch>\n";

/// RTF (Rich Text Format) signature. Every conforming RTF starts with
/// `{\rtf1`; `infer` doesn't classify RTF, so the explicit prefix
/// match is what routes stdin-piped RTF away from plain text.
const RTF_MAGIC: &[u8] = b"{\\rtf1";

/// PDF signature. Every conforming PDF starts with `%PDF-1.x` (v1.x)
/// or `%PDF-2.0` (v2.0). `infer` recognises PDFs but only for the
/// canonical path — the explicit prefix check keeps stdin-piped PDFs
/// reliable across infer versions.
const PDF_MAGIC: &[u8] = b"%PDF-";

/// cpio "newc" header magic (SVR4, no CRC) — the dominant form in
/// the wild (initramfs, RPM payloads).
const CPIO_NEWC_MAGIC: &[u8; 6] = b"070701";
/// cpio "newc + CRC" header magic. Same layout as newc; the `check`
/// field carries a checksum (we don't verify it on listing).
const CPIO_CRC_MAGIC: &[u8; 6] = b"070702";
/// cpio "ODC" / POSIX portable header magic (76-byte ASCII header).
const CPIO_ODC_MAGIC: &[u8; 6] = b"070707";

/// LZ4 frame format magic (little-endian `0x184D2204`). `infer` knows
/// this on some versions but not all; explicit check keeps stdin-piped
/// `.lz4` reliable across infer versions.
const LZ4_FRAME_MAGIC: &[u8; 4] = &[0x04, 0x22, 0x4D, 0x18];

/// Inspect a UTF-8 text buffer for a recognisable structured /
/// markup format. Returns the detected `FileType` plus a canonical
/// MIME so the caller can populate `Detected.magic_mime` when
/// `infer` didn't classify the bytes (it never identifies plain
/// text/XML formats). Used by both file and byte detection paths so
/// the rules stay in one place.
fn sniff_text_content(text: &str) -> Option<(FileType, &'static str)> {
    let trimmed = text.trim_start();
    let first = trimmed.as_bytes().first().copied();
    #[allow(clippy::collapsible_match)]
    match first {
        Some(b'{') | Some(b'[') => {
            if serde_json::from_str::<serde_json::Value>(text).is_ok() {
                return Some((
                    FileType::Structured(StructuredFormat::Json),
                    "application/json",
                ));
            }
        }
        Some(b'<') => {
            if trimmed.contains("<svg") {
                return Some((FileType::Svg, "image/svg+xml"));
            }
            let head_lower = trimmed[..trimmed.len().min(512)].to_ascii_lowercase();
            if head_lower.starts_with("<!doctype html") || head_lower.contains("<html") {
                return Some((FileType::Html, "text/html"));
            }
            return Some((
                FileType::Structured(StructuredFormat::Xml),
                "application/xml",
            ));
        }
        _ => {}
    }
    if trimmed.starts_with("---\n")
        || trimmed.starts_with("---\r\n")
        || trimmed == "---"
        || trimmed.starts_with("%YAML")
    {
        return Some((
            FileType::Structured(StructuredFormat::Yaml),
            "application/yaml",
        ));
    }
    None
}

/// Detect the file type from an in-memory byte buffer (for stdin).
/// Uses magic bytes for binary formats, then content sniffing for text.
fn detect_bytes(data: &[u8]) -> Detected {
    let magic_mime = head_magic_mime(data);
    if let Some(ref mime) = magic_mime
        && let Some(file_type) = file_type_from_magic_mime(mime)
    {
        return Detected::new(file_type, magic_mime);
    }

    // Non-UTF-8 → binary
    let Ok(text) = std::str::from_utf8(data) else {
        return Detected::new(FileType::Binary, magic_mime);
    };

    if let Some((file_type, content_mime)) = sniff_text_content(text) {
        return Detected::new(
            file_type,
            magic_mime.or_else(|| Some(content_mime.to_string())),
        );
    }

    // Plain text — `--language` can still pin a syntax for highlighting.
    Detected::new(FileType::SourceCode { syntax: None }, magic_mime)
}

/// Detect from a byte buffer with an optional source name. The name is
/// consulted first for extension-based classification (so a file
/// extracted from an archive into memory still routes by `.json` /
/// `.svg` / etc. just like a real path would), then for a syntect
/// syntax hint if content sniffing only resolves to plain SourceCode.
///
/// Used by `detect()` for `Memory` and `FileRange` sources so recursive
/// peek into a container (EPUB / archive / ISO) doesn't lose the entry
/// name's classification on its way back through the pipeline.
fn detect_bytes_named(data: &[u8], name: Option<&str>) -> Detected {
    if let Some(name) = name
        && let Some(file_type) = classify_by_name(name)
    {
        return Detected::new(
            upgrade_disk_image_bytes(file_type, data),
            head_magic_mime(data),
        );
    }
    let mut detected = detect_bytes(data);
    if let FileType::SourceCode { syntax: None } = &detected.file_type
        && let Some(ext) = name.and_then(mime::extension_from_name)
    {
        detected.file_type = FileType::SourceCode { syntax: Some(ext) };
    }
    detected
}

/// Single source of truth for name-based detection. Used by both the
/// file path and the in-memory byte path so the extension rules stay
/// consistent. Returns the unprobed `DiskImage::Raw` for `.img` /
/// `.bin` / `.dd`; callers run `upgrade_disk_image_path` /
/// `upgrade_disk_image_bytes` to upgrade to `Iso` when the body
/// carries the ISO 9660 PVD.
fn classify_by_name(name: &str) -> Option<FileType> {
    // Multi-entry containers (zip / tar / 7z / cpio / their compressed
    // tarballs) take precedence — double-extensions like `.tar.gz`
    // must classify as `ArchiveFormat::TarGz`, not bare `Compressed::Gz`.
    if let Some(fmt) = archive_format_from_name(name) {
        return Some(FileType::Archive(fmt));
    }
    if let Some(fmt) = compression_format_from_name(name) {
        return Some(FileType::Compressed(fmt));
    }
    let ext = mime::extension_from_name(name)?;
    if let Some(fmt) = comic_format_from_ext(&ext) {
        return Some(FileType::Comic(fmt));
    }
    if let Some(fmt) = disk_image_format_from_ext(&ext) {
        return Some(FileType::DiskImage(fmt));
    }
    if let Some(fmt) = audio_format_from_ext(&ext) {
        return Some(FileType::Audio(fmt));
    }
    Some(match ext.as_str() {
        "json" | "geojson" => FileType::Structured(StructuredFormat::Json),
        "jsonc" => FileType::Structured(StructuredFormat::Jsonc),
        "json5" => FileType::Structured(StructuredFormat::Json5),
        "jsonl" | "ndjson" => FileType::Structured(StructuredFormat::Jsonl),
        "yaml" | "yml" => FileType::Structured(StructuredFormat::Yaml),
        "toml" => FileType::Structured(StructuredFormat::Toml),
        "svg" => FileType::Svg,
        "html" | "htm" | "xhtml" => FileType::Html,
        "epub" => FileType::Ebook(EbookFormat::Epub),
        "docx" => FileType::Document(DocumentFormat::Docx),
        "odt" => FileType::Document(DocumentFormat::Odt),
        "rtf" => FileType::Document(DocumentFormat::Rtf),
        "pdf" => FileType::Pdf,
        "xml" | "plist" => FileType::Structured(StructuredFormat::Xml),
        _ => return None,
    })
}
