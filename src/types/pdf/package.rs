//! Pdfium-backed PDF reader.
//!
//! Owns the lazy `Pdfium` global (one per process — the underlying C++
//! library has process-wide state) and exposes high-level primitives
//! the read-modes / extract / info-gather paths consume:
//!
//! * `open_doc` parses a PDF from any [`InputSource`] into a `Doc` that
//!   can be cheaply cloned across the page mode, text mode, and the
//!   embed listing — `Doc` is `Arc`-backed.
//! * `list_pages`, `page_text`, `render_page` cover the read-mode work.
//! * `list_embeds`, `read_embed` cover the `/EmbeddedFiles` listing
//!   and per-attachment extract.
//!
//! The Pdfium dynamic library is located in this priority order:
//!   1. The directory of the running executable (release-tarball layout).
//!   2. The compile-time `.pdfium/lib` under the project root (dev fallback).
//!   3. System library paths (`Pdfium::bind_to_system_library`).
//!
//! All three are tried before surfacing a missing-library error.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use image::DynamicImage;
use pdfium_render::prelude::*;

use crate::input::InputSource;
use crate::types::listing::FlatEntry;

/// Process-wide Pdfium handle. Holds the C++ library bindings; a
/// `PdfDocument` borrows from it for as long as the doc lives.
static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

fn pdfium() -> Result<&'static Pdfium> {
    let result = PDFIUM.get_or_init(init_pdfium);
    result
        .as_ref()
        .map_err(|e| anyhow!("pdfium init failed: {e}"))
}

fn init_pdfium() -> Result<Pdfium, String> {
    let bindings = locate_bindings().map_err(|e| e.to_string())?;
    Ok(Pdfium::new(bindings))
}

fn locate_bindings() -> Result<Box<dyn PdfiumLibraryBindings>> {
    let mut errors: Vec<String> = Vec::new();

    // 1. Next to the running executable (release tarball ships the
    //    dylib alongside `peek`).
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let path = Pdfium::pdfium_platform_library_name_at_path(dir);
        match Pdfium::bind_to_library(&path) {
            Ok(b) => return Ok(b),
            Err(e) => errors.push(format!("exe-dir bind ({}): {e}", dir.display())),
        }
    }

    // 2. Project-local `.pdfium/lib` (dev workflow). `CARGO_MANIFEST_DIR`
    //    is baked in at compile time — only useful when running from
    //    the dev's own machine. Released binaries land elsewhere and
    //    fall through to system search.
    let dev_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".pdfium")
        .join("lib");
    if dev_dir.exists() {
        let path = Pdfium::pdfium_platform_library_name_at_path(&dev_dir);
        match Pdfium::bind_to_library(&path) {
            Ok(b) => return Ok(b),
            Err(e) => errors.push(format!(".pdfium/lib bind: {e}")),
        }
    }

    // 3. System library path.
    match Pdfium::bind_to_system_library() {
        Ok(b) => Ok(b),
        Err(e) => Err(anyhow!(
            "no Pdfium library found. Tried: {}; system: {e}",
            errors.join("; ")
        )),
    }
}

/// Cheaply-cloneable parsed PDF document. The underlying Pdfium handle
/// holds onto the original bytes (via `load_pdf_from_byte_vec`) for the
/// lifetime of the document, so callers don't have to keep the source.
#[derive(Clone)]
pub struct Doc {
    inner: Arc<DocInner>,
}

struct DocInner {
    document: PdfDocument<'static>,
    /// PDF version string read off the header (e.g. "1.7"). Empty on
    /// failure or when Pdfium can't decode it.
    pdf_version: String,
}

impl Doc {
    /// Number of pages in the document.
    pub fn page_count(&self) -> usize {
        self.inner.document.pages().len() as usize
    }

    /// Render page `idx` to an RGBA image at the given target width.
    /// Height is auto-scaled by Pdfium to preserve the page's native
    /// aspect ratio — the downstream image pipeline then resizes the
    /// bitmap to the terminal cell grid.
    pub fn render_page(&self, idx: usize, width_px: u32) -> Result<DynamicImage> {
        let pages = self.inner.document.pages();
        let page = pages
            .get(idx as i32)
            .with_context(|| format!("page {idx} not found"))?;
        let config = PdfRenderConfig::new()
            .set_target_width(width_px as i32)
            .render_form_data(false)
            .render_annotations(true);
        let bitmap = page
            .render_with_config(&config)
            .context("pdfium render failed")?;
        bitmap.as_image().context("pdfium bitmap → image failed")
    }

    /// Extract page `idx`'s text content. Layout heuristics use Pdfium's
    /// own text iterator (whitespace + character ordering reflect
    /// in-document order, not visual layout).
    pub fn page_text(&self, idx: usize) -> Result<String> {
        let pages = self.inner.document.pages();
        let page = pages
            .get(idx as i32)
            .with_context(|| format!("page {idx} not found"))?;
        let text = page.text().context("pdfium page text failed")?;
        Ok(text.all())
    }

    /// Document metadata (title / author / subject / keywords / dates).
    /// Empty fields drop to `None` so the renderer can skip them.
    pub fn metadata(&self) -> crate::types::document::DocumentMetadata {
        let mut meta = crate::types::document::DocumentMetadata::default();
        let tags = self.inner.document.metadata();
        for tag in tags.iter() {
            let value = tag.value().to_string();
            if value.trim().is_empty() {
                continue;
            }
            match tag.tag_type() {
                PdfDocumentMetadataTagType::Title => meta.title = Some(value),
                PdfDocumentMetadataTagType::Author => meta.creator = Some(value),
                PdfDocumentMetadataTagType::Subject => meta.subject = Some(value),
                PdfDocumentMetadataTagType::Keywords => meta.keywords = Some(value),
                PdfDocumentMetadataTagType::CreationDate => {
                    meta.created = Some(format_pdf_date(&value))
                }
                PdfDocumentMetadataTagType::ModificationDate => {
                    meta.modified = Some(format_pdf_date(&value))
                }
                _ => {}
            }
        }
        meta
    }

    pub fn pdf_version(&self) -> &str {
        &self.inner.pdf_version
    }

    pub fn is_encrypted(&self) -> bool {
        // Pdfium only loads unprotected (or already-unlocked) PDFs via
        // `load_pdf_from_byte_vec(_, None)`, so a successfully opened
        // doc was either unencrypted or the password was empty. We
        // don't have a reliable signal for "carries an /Encrypt dict"
        // from this entry point, so leave the flag off until there's
        // a real use for it.
        false
    }

    /// List embedded resources for the listing view: `/EmbeddedFiles`
    /// attachments under `attachments/`, plus inline page-image
    /// XObjects under `pages/page{N}/image{M}.{ext}`. Both sets share
    /// one tree so the user gets one TOC for everything they could
    /// pull out of the PDF.
    ///
    /// Image format extension is inferred from raw embedded bytes
    /// when those carry a recognisable codec (JPEG / PNG); other
    /// codecs (JBIG2, FlateDecode'd raw RGB) fall back to `.png` and
    /// the extract path re-encodes via `image::DynamicImage`.
    pub fn list_embeds(&self) -> Vec<FlatEntry> {
        let mut out = Vec::new();

        // 1. /EmbeddedFiles attachments
        let attachments = self.inner.document.attachments();
        let mut seen_attach = std::collections::HashSet::new();
        for (i, att) in attachments.iter().enumerate() {
            let name = unique_name(&mut seen_attach, &att.name(), i, "attachment");
            out.push(FlatEntry {
                path: format!("attachments/{name}"),
                size: att.len() as u64,
                mtime: None,
                mode: None,
                is_dir: false,
            });
        }

        // 2. Inline page-image XObjects. Walk every page's object list
        //    and surface anything the page-content stream paints as a
        //    raster. Vector / shading / form XObjects are skipped.
        let pages = self.inner.document.pages();
        for page_idx in 0..pages.len() {
            let Ok(page) = pages.get(page_idx) else {
                continue;
            };
            let mut img_idx = 0usize;
            for obj in page.objects().iter() {
                let Some(img) = obj.as_image_object() else {
                    continue;
                };
                img_idx += 1;
                // Use the raw embedded codec extension when it's a
                // recognisable file format; otherwise mark `.png`
                // because the extract path re-encodes the decoded
                // pixmap as PNG. List label has to match what the
                // user actually gets on extract — a `.bin` listed
                // here would route through detect.rs's `.bin` →
                // "Raw disk image" arm on recursive peek even
                // though the bytes are a valid PNG.
                let (ext, size) = match img.get_raw_image_data() {
                    Ok(bytes) => (
                        image_ext_from_bytes(&bytes).unwrap_or("png"),
                        bytes.len() as u64,
                    ),
                    Err(_) => ("png", 0),
                };
                out.push(FlatEntry {
                    path: format!("pages/page{}/image{}.{}", page_idx + 1, img_idx, ext),
                    size,
                    mtime: None,
                    mode: None,
                    is_dir: false,
                });
            }
        }

        out
    }

    /// Read a listed embedded resource by its `list_embeds` path.
    ///
    /// * `attachments/<name>` — looks up the matching `/EmbeddedFiles`
    ///   entry and saves its bytes.
    /// * `pages/page{N}/image{M}.{ext}` — walks page N's objects to
    ///   the Mth image object. Tries `get_raw_image_data` first
    ///   (preserves the original JPEG / PNG bytes); on failure or
    ///   when the codec isn't a usable file format, falls back to
    ///   `get_raw_image` → re-encode as PNG.
    pub fn read_embed(&self, key: &str) -> Result<Bytes> {
        if let Some(name) = key.strip_prefix("attachments/") {
            return self.read_attachment(name);
        }
        if key.starts_with("pages/page") {
            return self.read_page_image(key);
        }
        Err(anyhow!("unknown embed key {key}"))
    }

    fn read_attachment(&self, name: &str) -> Result<Bytes> {
        let attachments = self.inner.document.attachments();
        let mut seen = std::collections::HashSet::new();
        for (i, att) in attachments.iter().enumerate() {
            let candidate = unique_name(&mut seen, &att.name(), i, "attachment");
            if candidate == name {
                let bytes = att
                    .save_to_bytes()
                    .with_context(|| format!("failed to extract attachment {name}"))?;
                return Ok(Bytes::from(bytes));
            }
        }
        Err(anyhow!("attachment {name} not found"))
    }

    /// Parse `pages/page{N}/image{M}.<ext>` and return the matching
    /// raw image bytes. N and M are 1-based.
    fn read_page_image(&self, key: &str) -> Result<Bytes> {
        let (page_n, img_n) =
            parse_page_image_key(key).ok_or_else(|| anyhow!("malformed page-image key {key}"))?;
        let pages = self.inner.document.pages();
        let page = pages
            .get((page_n - 1) as i32)
            .with_context(|| format!("page {page_n} not found"))?;
        let mut count = 0usize;
        for obj in page.objects().iter() {
            let Some(img) = obj.as_image_object() else {
                continue;
            };
            count += 1;
            if count != img_n {
                continue;
            }
            // Prefer raw embedded bytes — preserves original JPEG /
            // PNG fidelity. Fall back to re-encoding the decoded
            // pixmap as PNG for codecs we can't surface directly
            // (JBIG2, raw FlateDecode'd RGB, …).
            if let Ok(raw) = img.get_raw_image_data()
                && image_ext_from_bytes(&raw).is_some()
            {
                return Ok(Bytes::from(raw));
            }
            let pixmap = img.get_raw_image().context("decode embedded image")?;
            let mut buf = Vec::new();
            pixmap
                .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
                .context("re-encode embedded image as PNG")?;
            return Ok(Bytes::from(buf));
        }
        Err(anyhow!("image {img_n} on page {page_n} not found"))
    }
}

/// Return a unique, filesystem-safe variant of `raw` for the
/// listing's TOC. Strips slashes from inner names and adds `_2`,
/// `_3`, … on collision.
fn unique_name(
    seen: &mut std::collections::HashSet<String>,
    raw: &str,
    idx: usize,
    fallback: &str,
) -> String {
    let base = if raw.trim().is_empty() {
        format!("{fallback}{}", idx + 1)
    } else {
        raw.replace('/', "_")
    };
    if seen.insert(base.clone()) {
        return base;
    }
    let mut n = 2usize;
    loop {
        let candidate = format!("{base}_{n}");
        if seen.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

/// Detect a useful file extension from an embedded image's raw
/// bytes. PDFs commonly carry JPEG-DCT and PNG-style streams that
/// `get_raw_image_data` returns verbatim.
fn image_ext_from_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG") {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.starts_with(b"RIFF") {
        Some("webp")
    } else if bytes.starts_with(b"II*\x00") || bytes.starts_with(b"MM\x00*") {
        Some("tiff")
    } else {
        None
    }
}

/// Parse `pages/page{N}/image{M}.<ext>` → `(N, M)`. Returns `None`
/// if the layout doesn't match (unsanitised / unrelated keys).
fn parse_page_image_key(key: &str) -> Option<(usize, usize)> {
    let rest = key.strip_prefix("pages/page")?;
    let (page_str, after_page) = split_digits(rest)?;
    let after_slash = after_page.strip_prefix("/image")?;
    let (img_str, _) = split_digits(after_slash)?;
    Some((page_str.parse().ok()?, img_str.parse().ok()?))
}

fn split_digits(s: &str) -> Option<(&str, &str)> {
    let end = s.bytes().take_while(|b| b.is_ascii_digit()).count();
    if end == 0 {
        return None;
    }
    Some((&s[..end], &s[end..]))
}

/// Open a PDF from any [`InputSource`]. Bytes are pulled into memory
/// and handed to Pdfium via `load_pdf_from_byte_vec` so the document
/// owns the underlying buffer.
pub fn open_doc(source: &InputSource) -> Result<Doc> {
    let pdfium = pdfium()?;
    let bytes = source.read_bytes().context("failed to read PDF source")?;
    let pdf_version = read_pdf_version(&bytes);
    let document = pdfium
        .load_pdf_from_byte_vec(bytes, None)
        .context("pdfium failed to parse PDF")?;
    Ok(Doc {
        inner: Arc::new(DocInner {
            document,
            pdf_version,
        }),
    })
}

/// Reformat a PDF date string (`D:YYYYMMDDHHmmSSOHH'mm'`) into a
/// readable `YYYY-MM-DD HH:MM:SS [±HH:MM | UTC]`. Falls back to the
/// raw value when the prefix or layout doesn't match.
fn format_pdf_date(raw: &str) -> String {
    let body = raw.strip_prefix("D:").unwrap_or(raw);
    if body.len() < 4 {
        return raw.to_string();
    }
    let bytes = body.as_bytes();
    if !bytes.iter().take(4).all(|b| b.is_ascii_digit()) {
        return raw.to_string();
    }
    let take = |start: usize, len: usize| -> Option<&str> {
        if body.len() < start + len {
            return None;
        }
        let slice = &body[start..start + len];
        if slice.bytes().all(|b| b.is_ascii_digit()) {
            Some(slice)
        } else {
            None
        }
    };
    let year = &body[..4];
    let month = take(4, 2).unwrap_or("01");
    let day = take(6, 2).unwrap_or("01");
    let hour = take(8, 2).unwrap_or("00");
    let minute = take(10, 2).unwrap_or("00");
    let second = take(12, 2).unwrap_or("00");
    let date_part = format!("{year}-{month}-{day} {hour}:{minute}:{second}");

    // Trailing offset: `Z` (UTC), `+HH'mm'` or `-HH'mm'`. Strip the
    // apostrophes per PDF spec and present as `±HH:MM`.
    let tz_start = 14usize;
    if body.len() <= tz_start {
        return date_part;
    }
    let tz = &body[tz_start..];
    if tz.starts_with('Z') {
        return format!("{date_part} UTC");
    }
    if (tz.starts_with('+') || tz.starts_with('-')) && tz.len() >= 3 {
        let sign = &tz[..1];
        let hh = &tz[1..3];
        let mm = if tz.len() >= 6 && (tz.as_bytes()[3] == b'\'' || tz.as_bytes()[3] == b':') {
            &tz[4..6]
        } else {
            "00"
        };
        if hh.bytes().all(|b| b.is_ascii_digit()) && mm.bytes().all(|b| b.is_ascii_digit()) {
            return format!("{date_part} {sign}{hh}:{mm}");
        }
    }
    date_part
}

/// Read the PDF version string from the file header (`%PDF-1.7\n…`).
/// Returns an empty string if the header isn't recognisable. Works on
/// raw bytes (avoids `from_utf8` since the binary blob immediately
/// after the version often isn't valid UTF-8).
fn read_pdf_version(bytes: &[u8]) -> String {
    const MAGIC: &[u8] = b"%PDF-";
    if !bytes.starts_with(MAGIC) {
        return String::new();
    }
    let rest = &bytes[MAGIC.len()..];
    let end = rest
        .iter()
        .take(8)
        .take_while(|b| b.is_ascii_digit() || **b == b'.')
        .count();
    String::from_utf8_lossy(&rest[..end]).into_owned()
}
