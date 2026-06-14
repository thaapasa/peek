//! EPUB read reader: one chapter at a time.
//!
//! Supplies per-chapter rendering to the shared
//! [`PagedTextReadMode`](crate::viewer::paged::PagedTextReadMode) shell —
//! `n` / `p` stepping, per-chapter search, the render cache, and the
//! whole `Mode` impl live there. Each chapter renders through the shared
//! HTML pipeline (`types::html::render`). The cache key is a
//! [`PageCacheKey`] so a resize or image-config cycle re-renders only the
//! visible chapter.
//!
//! Two image conveniences sit on top of the text path:
//!
//! - Every `<img>` source HTML tag is pre-processed so empty `alt=""`
//!   attributes get a fallback label of `image: <basename of src>`,
//!   keeping image references visible in flowing prose instead of
//!   being silently dropped by html2text.
//! - Cover-style chapters (chapter renders to ≤ 3 non-empty lines and
//!   the source has at least one `<img>`) render that first image as
//!   ASCII art inline. The TOC view still exposes every container
//!   entry for general image inspection via recursive peek.
//!
//! These two are why the reader plugs into the *text* shell, not the
//! paged-*image* `PagedImageMode<R>`: chapter search and cover-image
//! rendering don't belong on the image trait (PDFs don't search per page;
//! comics never cover-render text). The image-config cycle keys are wired
//! through [`PagedText::pre_handle`].

use anyhow::Result;

use crate::types::image::pipeline::ImageConfig;
use crate::types::image::pipeline::render::{self as image_render, GridWindow, prepare_decoded};
use crate::viewer::cell_size;
use crate::viewer::modes::{Handled, RenderCtx};
use crate::viewer::paged::{
    self, CYCLE_FIT_HELP, PageCacheKey, PagedText, PagedTextReadMode, cycle_image_config,
};
use crate::viewer::ui::{Action, HelpEntry};
use peek_io::InputSource;
use peek_theme::StyleMode;

use super::package::{self, Chapter, Package};

const EXTRA_ACTIONS: &[HelpEntry] = &[
    (
        // With a search active these step matches instead of chapters.
        &[Action::Next, Action::Prev],
        "Next / previous chapter",
    ),
    (&[Action::OpenSearch], "Search"),
    // Cycling these only affects cover-style chapters that render an
    // inline image, but the keys are declared unconditionally so the
    // user can pre-set them before stepping to a cover chapter.
    (
        &[Action::CycleBackground, Action::CycleBackgroundBack],
        "Cycle background (cover image)",
    ),
    (
        &[Action::CycleImageMode, Action::CycleImageModeBack],
        "Cycle render mode (cover image)",
    ),
    CYCLE_FIT_HELP,
];

/// Heuristic threshold: chapters that produce at most this many
/// non-empty lines of text are considered "cover-style". When the
/// source also has an `<img>`, the first image is rendered inline.
const COVER_LIKE_LINE_THRESHOLD: usize = 3;

pub(crate) struct EpubReader {
    source: InputSource,
    /// Image config snapshot — only the cover-image render path uses
    /// it. `style_mode` is read live from the render context so a `c`
    /// cycle re-renders without going through this struct.
    image_config: ImageConfig,
    chapters: Vec<Chapter>,
    warnings: Vec<String>,
}

impl EpubReader {
    pub(crate) fn into_mode(
        source: InputSource,
        image_config: ImageConfig,
        package: Package,
    ) -> PagedTextReadMode<Self> {
        PagedTextReadMode::new(Self {
            source,
            image_config,
            chapters: package.chapters,
            warnings: Vec::new(),
        })
    }
}

impl PagedText for EpubReader {
    type Key = PageCacheKey;

    fn pages_len(&self) -> usize {
        self.chapters.len()
    }

    fn page_label(&self) -> &'static str {
        "ch"
    }

    fn nav_hint(&self) -> &'static str {
        "n/p:chapter"
    }

    fn extra_actions(&self) -> &'static [HelpEntry] {
        EXTRA_ACTIONS
    }

    fn cache_key(&self, ctx: &RenderCtx) -> PageCacheKey {
        PageCacheKey::build(
            &self.image_config,
            ctx.term_cols,
            ctx.term_rows,
            ctx.peek_theme.style_mode,
        )
    }

    fn render_page(&mut self, idx: usize, ctx: &RenderCtx) -> Result<Vec<String>> {
        // Disjoint field borrows: the render reads `source` / `chapters` /
        // `image_config` (Copy) and pushes into `warnings`.
        render_chapter(
            &self.source,
            &self.chapters,
            self.image_config,
            idx,
            ctx.term_cols,
            ctx.term_rows,
            ctx.peek_theme.style_mode,
            &mut self.warnings,
        )
    }

    fn pre_handle(&mut self, action: Action) -> Option<Handled> {
        // Image controls — mutate the stored config; the cache-key change
        // auto-invalidates any cover-rendered chapter on next access.
        // Text-only chapters are unaffected but still re-render (cheap),
        // which keeps the implementation uniform.
        cycle_image_config(action, &mut self.image_config)
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}

#[allow(clippy::too_many_arguments)]
fn render_chapter(
    source: &InputSource,
    chapters: &[Chapter],
    image_config: ImageConfig,
    idx: usize,
    width: usize,
    rows: usize,
    style_mode: StyleMode,
    warnings: &mut Vec<String>,
) -> Result<Vec<String>> {
    let chapter = chapters[idx].clone();
    let mut zip = match package::open_zip(source) {
        Ok(z) => z,
        Err(e) => {
            warnings.push(format!("chapter {}: {e:#}", idx + 1));
            return Ok(vec![format!("[chapter {} unavailable]", idx + 1)]);
        }
    };
    let raw_bytes = match package::read_entry(&mut zip, &chapter.full_path) {
        Ok(b) => b,
        Err(e) => {
            warnings.push(format!("chapter {}: {e:#}", idx + 1));
            return Ok(vec![format!("[chapter {} unavailable]", idx + 1)]);
        }
    };
    let raw_html = std::str::from_utf8(&raw_bytes).unwrap_or("");
    let labeled = label_images(raw_html);
    let text_lines =
        crate::types::html::render::render(labeled.as_bytes(), width.max(20), style_mode)?;

    let non_empty = text_lines.iter().filter(|l| !l.trim().is_empty()).count();
    if non_empty > COVER_LIKE_LINE_THRESHOLD {
        return Ok(text_lines);
    }
    let Some(img_src) = first_img_src(raw_html) else {
        return Ok(text_lines);
    };
    let chapter_dir = parent_dir(&chapter.full_path);
    let img_path = resolve_relative(chapter_dir, &img_src);
    match render_inline_image(
        &mut zip,
        &img_path,
        image_config,
        style_mode,
        width as u32,
        rows,
    ) {
        Ok(img_lines) => Ok(img_lines),
        Err(e) => {
            warnings.push(format!("chapter {} image {img_path}: {e:#}", idx + 1));
            Ok(text_lines)
        }
    }
}

// ---------------------------------------------------------------------------
// HTML pre-processing
// ---------------------------------------------------------------------------

/// Walk `<img>` tags in `html` and ensure each has a non-empty `alt`
/// attribute. Empty / missing alt is replaced with
/// `alt="image: {basename(src)}"` so html2text emits a visible
/// placeholder instead of silently dropping the tag. Hand-scanned
/// rather than full HTML-parsed: EPUB content is XHTML-shaped and we
/// only touch one tag, so a real parser would be overkill (and
/// expensive — quick-xml's strict mode rejects HTML5 quirks).
fn label_images(html: &str) -> String {
    let mut out = String::with_capacity(html.len() + 64);
    let mut rest = html;
    while let Some(idx) = find_case_insensitive(rest, "<img") {
        out.push_str(&rest[..idx]);
        rest = &rest[idx..];
        // Only treat as a tag when the next char isn't an identifier
        // continuation (avoids matching `<imgno>` or similar).
        let after = rest.as_bytes().get(4).copied();
        if !matches!(after, Some(b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>')) {
            // Not actually an img tag — copy the `<img` and continue.
            out.push_str(&rest[..4]);
            rest = &rest[4..];
            continue;
        }
        let Some(end_offset) = rest.find('>') else {
            out.push_str(rest);
            return out;
        };
        let tag_end = end_offset + 1;
        let tag = &rest[..tag_end];
        out.push_str(&rewrite_img_tag(tag));
        rest = &rest[tag_end..];
    }
    out.push_str(rest);
    out
}

fn rewrite_img_tag(tag: &str) -> String {
    let alt = extract_attr(tag, "alt");
    let src = extract_attr(tag, "src");
    let needs_fallback = alt.as_deref().map(|a| a.trim().is_empty()).unwrap_or(true);
    if !needs_fallback {
        return tag.to_string();
    }
    let label = match src.as_deref() {
        Some(s) if !s.is_empty() => format!("image: {}", basename(s)),
        _ => "image".to_string(),
    };
    if alt.is_some() {
        replace_attr(tag, "alt", &label)
    } else {
        insert_attr_before_tag_end(tag, "alt", &label)
    }
}

fn extract_attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{name}=");
    let mut search = 0;
    while let Some(rel) = lower[search..].find(&needle) {
        let pos = search + rel;
        // Reject attribute names that are suffixes of a longer name
        // (e.g. avoid matching `data-alt=` when looking for `alt=`).
        let prev = pos
            .checked_sub(1)
            .and_then(|i| tag.as_bytes().get(i))
            .copied();
        if !matches!(prev, None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'/')) {
            search = pos + needle.len();
            continue;
        }
        let value_start = pos + needle.len();
        let bytes = tag.as_bytes();
        let quote = bytes.get(value_start).copied();
        let (vstart, vend) = match quote {
            Some(b'"') => {
                let s = value_start + 1;
                let e = tag[s..].find('"').map(|i| s + i)?;
                (s, e)
            }
            Some(b'\'') => {
                let s = value_start + 1;
                let e = tag[s..].find('\'').map(|i| s + i)?;
                (s, e)
            }
            _ => return None,
        };
        return Some(tag[vstart..vend].to_string());
    }
    None
}

fn replace_attr(tag: &str, name: &str, new_value: &str) -> String {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{name}=");
    let mut out = String::with_capacity(tag.len() + new_value.len());
    let mut search = 0;
    while let Some(rel) = lower[search..].find(&needle) {
        let pos = search + rel;
        let prev = pos
            .checked_sub(1)
            .and_then(|i| tag.as_bytes().get(i))
            .copied();
        if !matches!(prev, None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'/')) {
            search = pos + needle.len();
            continue;
        }
        let value_start = pos + needle.len();
        let bytes = tag.as_bytes();
        let quote = bytes.get(value_start).copied();
        let close = match quote {
            Some(b'"') => Some('"'),
            Some(b'\'') => Some('\''),
            _ => None,
        };
        let Some(quote_ch) = close else {
            return tag.to_string();
        };
        let s = value_start + 1;
        let Some(rel_end) = tag[s..].find(quote_ch) else {
            return tag.to_string();
        };
        let e = s + rel_end;
        out.push_str(&tag[..s]);
        out.push_str(new_value);
        out.push_str(&tag[e..]);
        return out;
    }
    tag.to_string()
}

fn insert_attr_before_tag_end(tag: &str, name: &str, value: &str) -> String {
    let trimmed = tag.trim_end();
    let close_off = if let Some(stripped) = trimmed.strip_suffix("/>") {
        stripped.len()
    } else if let Some(stripped) = trimmed.strip_suffix('>') {
        stripped.len()
    } else {
        return tag.to_string();
    };
    let pre = &tag[..close_off];
    let post = &tag[close_off..];
    let sep = if pre.ends_with([' ', '\t', '\n', '\r']) {
        ""
    } else {
        " "
    };
    format!("{pre}{sep}{name}=\"{value}\"{post}")
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let lower = haystack.to_ascii_lowercase();
    lower.find(needle)
}

fn first_img_src(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut search = 0;
    while let Some(rel) = lower[search..].find("<img") {
        let pos = search + rel;
        let after = html.as_bytes().get(pos + 4).copied();
        if !matches!(after, Some(b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>')) {
            search = pos + 4;
            continue;
        }
        let end_off = html[pos..].find('>')?;
        let tag = &html[pos..pos + end_off + 1];
        if let Some(src) = extract_attr(tag, "src")
            && !src.is_empty()
        {
            return Some(src);
        }
        search = pos + end_off + 1;
    }
    None
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Resolve a relative path inside an EPUB: `dir` is a slash-joined
/// directory (no trailing slash), `href` is the link from the chapter.
/// Handles `../` and absolute hrefs.
fn resolve_relative(dir: &str, href: &str) -> String {
    if href.starts_with('/') {
        return href.trim_start_matches('/').to_string();
    }
    let mut parts: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for seg in href.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

// ---------------------------------------------------------------------------
// Inline image rendering
// ---------------------------------------------------------------------------

fn render_inline_image(
    zip: &mut zip::ZipArchive<Box<dyn crate::types::archive::reader::ReadSeek>>,
    path: &str,
    base_config: ImageConfig,
    style_mode: StyleMode,
    term_cols: u32,
    term_rows: usize,
) -> Result<Vec<String>> {
    let bytes = package::read_entry(zip, path)?;
    let img = image::load_from_memory(&bytes)?;
    let mut config = base_config;
    config.style_mode = style_mode;
    let term = cell_size::term_size(term_cols, paged::pipe_rows(term_rows));
    let prep = prepare_decoded(img, &config, term);
    let window = GridWindow::full(prep.cols, prep.rows);
    let lines = image_render::render_prepared(&prep, &config, window);
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::{FileInfo, NoExtras, RenderOptions};
    use crate::types::image::pipeline::ImageConfig;
    use crate::viewer::image_render::{Background, FitMode, ImageMode};
    use crate::viewer::modes::{Mode, RenderCtx};
    use peek_theme::{PeekTheme, PeekThemeName, StyleMode, ThemeManager};

    fn epub_fixture() -> InputSource {
        let path = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("test-books/frankenstein.epub");
        InputSource::File(path)
    }

    fn image_config() -> ImageConfig {
        ImageConfig {
            mode: ImageMode::from_str("block"),
            width: 0,
            background: Background::from_str("auto"),
            margin: 0,
            style_mode: StyleMode::Plain,
            edge_density: 0.1,
            fit: FitMode::Contain,
        }
    }

    fn synthetic_file_info() -> FileInfo {
        FileInfo {
            file_name: String::new(),
            path: String::new(),
            size_bytes: 0,
            mimes: Vec::new(),
            warnings: Vec::new(),
            modified: None,
            created: None,
            permissions: None,
            compression: None,
            extras: Box::new(NoExtras),
        }
    }

    fn make_ctx<'a>(file_info: &'a FileInfo, peek_theme: &'a PeekTheme) -> RenderCtx<'a> {
        RenderCtx {
            file_info,
            theme_name: PeekThemeName::IdeaDark,
            peek_theme,
            render_opts: RenderOptions::default(),
            term_cols: 80,
            term_rows: 40,
        }
    }

    /// End-to-end through the shared `PagedTextReadMode` shell: the EPUB
    /// reader renders a chapter, reports a `ch i/N` status, and `n` steps
    /// to the next chapter. Guards the `EpubReader` → shell wiring (cache
    /// key, page label, step dispatch) against a real multi-chapter book.
    #[test]
    fn renders_and_steps_chapters() {
        let source = epub_fixture();
        let pkg = package::open(&source).unwrap();
        let chapter_count = pkg.chapters.len();
        assert!(chapter_count > 1, "fixture must have multiple chapters");

        let mut mode = EpubReader::into_mode(source.clone(), image_config(), pkg);
        let file_info = synthetic_file_info();
        let tm = ThemeManager::new(PeekThemeName::IdeaDark, StyleMode::Plain);
        let ctx = make_ctx(&file_info, tm.peek_theme());

        // First chapter renders to a non-empty window.
        let win = mode.render_window(&ctx, 0, 40).unwrap();
        assert!(win.total > 0);
        assert!(!win.lines.is_empty());

        // Status reports the page counter through the shell.
        let seg = |m: &dyn Mode| m.status_segments(tm.peek_theme())[0].0.clone();
        assert_eq!(seg(&mode), format!("ch 1/{chapter_count}"));

        // `n` steps to the next chapter; render it so the counter updates.
        mode.handle(Action::Next);
        let _ = mode.render_window(&ctx, 0, 40).unwrap();
        assert_eq!(seg(&mode), format!("ch 2/{chapter_count}"));
    }
}
