//! EPUB read mode: one chapter at a time.
//!
//! Renders the spine entry at `current` through the shared HTML
//! pipeline (`types::html::render`). `n` / `N` step forward / back
//! through the spine, resetting the scroll offset for the new
//! chapter. The render cache is keyed by `(chapter, width)` so a
//! resize re-renders only the visible chapter and a chapter step
//! reuses prior renders when stepping back.
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

use anyhow::Result;
use syntect::highlighting::Color;

use crate::input::InputSource;
use crate::output::PrintOutput;
use crate::theme::{PeekTheme, StyleMode};
use crate::types::image::pipeline::render::{
    self as image_render, GridWindow, TermSize, prepare_decoded,
};
use crate::types::image::pipeline::{Background, FitMode, ImageConfig, ImageMode};
use crate::viewer::cell_size::cell_aspect_h_over_w;
use crate::viewer::modes::{Handled, Mode, ModeId, RenderCtx, Window, slice_window};
use crate::viewer::ui::Action;

use super::package::{self, Chapter, Package};

const EXTRA_ACTIONS: &[(Action, &str)] = &[
    (Action::NextChapter, "Next chapter"),
    (Action::PrevChapter, "Previous chapter"),
    // Cycling these only affects cover-style chapters that render an
    // inline image, but the keys are declared unconditionally so the
    // user can pre-set them before stepping to a cover chapter.
    (Action::CycleBackground, "Cycle background (cover image)"),
    (Action::CycleBackgroundBack, "Cycle background backward"),
    (Action::CycleImageMode, "Cycle render mode (cover image)"),
    (Action::CycleImageModeBack, "Cycle render mode backward"),
    (Action::CycleFitMode, "Cycle fit (contain / width / height)"),
];

/// Heuristic threshold: chapters that produce at most this many
/// non-empty lines of text are considered "cover-style". When the
/// source also has an `<img>`, the first image is rendered inline.
const COVER_LIKE_LINE_THRESHOLD: usize = 3;

/// Cap on inline image height in pipe / `--print` mode where
/// `term_rows` is unbounded; otherwise an image-aspect chapter would
/// dominate the output.
const PIPE_IMAGE_MAX_ROWS: u32 = 30;

pub(crate) struct EpubReadMode {
    source: InputSource,
    /// Image config snapshot — only the cover-image render path uses
    /// it. `style_mode` is read live from the render context so a `c`
    /// cycle re-renders without going through this struct.
    image_config: ImageConfig,
    chapters: Vec<Chapter>,
    current: usize,
    /// Per-chapter rendered cache. Cache key embeds every input that
    /// can change the rendered output (width, rows, style mode, image
    /// config), so any of them shifting forces a re-render on next
    /// access without an explicit invalidation.
    cache: Vec<Option<CachedChapter>>,
    warnings: Vec<String>,
}

/// Inputs that affect the rendered output for one chapter. Stored
/// alongside the cached lines so the cache invalidates automatically
/// when the user cycles color (`c`), background (`b`), image mode
/// (`m`), or fit (`f`) — or when the terminal resizes.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    width: usize,
    rows: usize,
    style_mode: StyleMode,
    image_mode: ImageMode,
    background: Background,
    fit: FitMode,
}

struct CachedChapter {
    key: CacheKey,
    lines: Vec<String>,
}

impl EpubReadMode {
    pub(crate) fn new(source: InputSource, image_config: ImageConfig, package: Package) -> Self {
        let n = package.chapters.len();
        let mut cache = Vec::with_capacity(n);
        cache.resize_with(n, || None);
        Self {
            source,
            image_config,
            chapters: package.chapters,
            current: 0,
            cache,
            warnings: Vec::new(),
        }
    }

    fn key_for(&self, width: usize, rows: usize, style_mode: StyleMode) -> CacheKey {
        CacheKey {
            width,
            rows,
            style_mode,
            image_mode: self.image_config.mode,
            background: self.image_config.background,
            fit: self.image_config.fit,
        }
    }

    fn ensure_rendered(
        &mut self,
        width: usize,
        rows: usize,
        style_mode: StyleMode,
    ) -> Result<&[String]> {
        if self.chapters.is_empty() {
            return Ok(&[]);
        }
        let idx = self.current;
        let key = self.key_for(width, rows, style_mode);
        let needs = self
            .cache
            .get(idx)
            .and_then(|c| c.as_ref())
            .map(|c| c.key != key)
            .unwrap_or(true);
        if needs {
            let lines = self.render_chapter(idx, &key)?;
            self.cache[idx] = Some(CachedChapter { key, lines });
        }
        Ok(&self
            .cache
            .get(idx)
            .and_then(|c| c.as_ref())
            .expect("cache populated")
            .lines)
    }

    fn render_chapter(&mut self, idx: usize, key: &CacheKey) -> Result<Vec<String>> {
        let chapter = self.chapters[idx].clone();
        let mut zip = match package::open_zip(&self.source) {
            Ok(z) => z,
            Err(e) => {
                self.warnings.push(format!("chapter {}: {e:#}", idx + 1));
                return Ok(vec![format!("[chapter {} unavailable]", idx + 1)]);
            }
        };
        let raw_bytes = match package::read_entry(&mut zip, &chapter.full_path) {
            Ok(b) => b,
            Err(e) => {
                self.warnings.push(format!("chapter {}: {e:#}", idx + 1));
                return Ok(vec![format!("[chapter {} unavailable]", idx + 1)]);
            }
        };
        let raw_html = std::str::from_utf8(&raw_bytes).unwrap_or("");
        let labeled = label_images(raw_html);
        let text_lines = crate::types::html::render::render(
            labeled.as_bytes(),
            key.width.max(20),
            key.style_mode,
        )?;

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
            self.image_config,
            key.style_mode,
            key.width as u32,
            key.rows,
        ) {
            Ok(img_lines) => Ok(img_lines),
            Err(e) => {
                self.warnings
                    .push(format!("chapter {} image {img_path}: {e:#}", idx + 1));
                Ok(text_lines)
            }
        }
    }
}

impl Mode for EpubReadMode {
    fn id(&self) -> ModeId {
        ModeId::Rendered
    }

    fn label(&self) -> &str {
        "Read"
    }

    fn rerender_on_resize(&self) -> bool {
        true
    }

    fn render_window(&mut self, ctx: &RenderCtx, scroll: usize, rows: usize) -> Result<Window> {
        let lines =
            self.ensure_rendered(ctx.term_cols, ctx.term_rows, ctx.peek_theme.style_mode)?;
        let total = lines.len();
        let win = slice_window(lines, scroll, rows);
        Ok(Window { lines: win, total })
    }

    fn total_lines(&self) -> Option<usize> {
        self.cache
            .get(self.current)
            .and_then(|c| c.as_ref())
            .map(|c| c.lines.len())
    }

    /// Print mode walks every chapter in spine order, separating
    /// each with a blank line. Honors the cache so chapters already
    /// rendered (after Tab + scrolling in interactive mode) reuse
    /// their output. The interactive view stays single-chapter; only
    /// the pipe path materializes the whole book.
    fn render_to_pipe(&mut self, ctx: &RenderCtx, out: &mut PrintOutput) -> Result<()> {
        let total = self.chapters.len();
        let saved = self.current;
        for i in 0..total {
            self.current = i;
            let lines =
                self.ensure_rendered(ctx.term_cols, ctx.term_rows, ctx.peek_theme.style_mode)?;
            for line in lines {
                out.write_line(line)?;
            }
            if i + 1 < total {
                out.write_line("")?;
            }
        }
        self.current = saved;
        Ok(())
    }

    fn extra_actions(&self) -> &'static [(Action, &'static str)] {
        EXTRA_ACTIONS
    }

    fn handle(&mut self, action: Action) -> Handled {
        match action {
            Action::NextChapter => {
                if self.chapters.is_empty() {
                    return Handled::No;
                }
                let next = (self.current + 1).min(self.chapters.len() - 1);
                if next == self.current {
                    return Handled::Yes;
                }
                self.current = next;
                Handled::YesResetScroll
            }
            Action::PrevChapter => {
                if self.current == 0 {
                    return Handled::Yes;
                }
                self.current = self.current.saturating_sub(1);
                Handled::YesResetScroll
            }
            // Image controls — mutate the stored config; cache key
            // change auto-invalidates any cover-rendered chapter on
            // next access. Text-only chapters are unaffected by this
            // change but their cache still re-renders (cheap), which
            // keeps the implementation uniform.
            Action::CycleBackground => {
                self.image_config.background = self.image_config.background.next();
                Handled::Yes
            }
            Action::CycleBackgroundBack => {
                self.image_config.background = self.image_config.background.prev();
                Handled::Yes
            }
            Action::CycleImageMode => {
                self.image_config.mode = self.image_config.mode.next();
                Handled::Yes
            }
            Action::CycleImageModeBack => {
                self.image_config.mode = self.image_config.mode.prev();
                Handled::Yes
            }
            Action::CycleFitMode => {
                self.image_config.fit = self.image_config.fit.next();
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn status_segments(&self, theme: &PeekTheme) -> Vec<(String, Color)> {
        if self.chapters.is_empty() {
            return Vec::new();
        }
        vec![(
            format!("ch {}/{}", self.current + 1, self.chapters.len()),
            theme.muted,
        )]
    }

    fn status_hints(&self, _has_return_target: bool) -> Vec<&'static str> {
        if self.chapters.len() <= 1 {
            return Vec::new();
        }
        vec!["n/N:chapter"]
    }

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
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
    // Pipe contexts pass `usize::MAX` for term_rows; cap so the image
    // doesn't dominate piped output. Interactive callers pass the live
    // viewport height and want the image to fit it.
    let rows = if term_rows == usize::MAX {
        PIPE_IMAGE_MAX_ROWS
    } else {
        term_rows.min(u32::MAX as usize) as u32
    };
    let term = TermSize {
        cols: term_cols,
        rows,
        cell_h_over_w: cell_aspect_h_over_w(),
    };
    let prep = prepare_decoded(img, &config, term);
    let window = GridWindow::full(prep.cols, prep.rows);
    let lines = image_render::render_prepared(&prep, &config, window);
    Ok(lines)
}
