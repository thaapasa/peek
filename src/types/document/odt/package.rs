//! ODT package open + AST conversion.
//!
//! Hand-walks `content.xml` with `quick-xml` and resolves
//! `text:style-name` references against the pre-scanned automatic-styles
//! table inside the same file. Mirrors the DOCX parser's shape so both
//! formats produce the shared [`Doc`] AST.
//!
//! Style indirection is the only structural difference vs DOCX: ODT
//! runs carry a style *name* (`<text:span text:style-name="T1"/>`)
//! whose attributes live in `<office:automatic-styles>` higher up the
//! file. The parser does a single forward pass — automatic-styles is
//! required by the spec to appear before `<office:body>` — and resolves
//! span styles on the fly.
//!
//! `styles.xml` (the package's separate styles container) is intentionally
//! not consulted: real-world ODTs from LibreOffice / OpenOffice dump
//! all directly-used styling into content.xml's automatic-styles. Named
//! styles in styles.xml only matter for inheritance chains we don't
//! resolve in v1.

use std::collections::HashMap;
use std::io::Read;

use anyhow::{Context, Result, anyhow};
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;
use quick_xml::reader::Reader;
use zip::ZipArchive;

use crate::input::InputSource;
use crate::types::archive::reader::{ReadSeek, open_seekable};
use crate::types::document::DocumentMetadata;
use crate::types::document::ast::{Block, Doc, Paragraph, Run, count_words, merge_paragraphs};

pub fn open(source: &InputSource) -> Result<Doc> {
    let reader = open_seekable(source).context("failed to open ODT container")?;
    let mut zip = ZipArchive::new(reader).context("failed to read ODT archive")?;

    let content_xml = read_entry(&mut zip, "content.xml").context("ODT missing content.xml")?;
    let meta_xml = read_entry(&mut zip, "meta.xml").ok();

    let metadata = meta_xml.as_deref().map(parse_meta).unwrap_or_default();
    parse_content(&content_xml, metadata)
}

fn read_entry(zip: &mut ZipArchive<Box<dyn ReadSeek>>, path: &str) -> Result<String> {
    let mut file = zip
        .by_name(path)
        .with_context(|| format!("ODT entry {path:?} not found"))?;
    let mut buf = String::with_capacity(file.size() as usize);
    file.read_to_string(&mut buf)?;
    Ok(buf)
}

// ---------------------------------------------------------------------------
// content.xml — pre-scan styles, then walk body
// ---------------------------------------------------------------------------

/// Resolved attributes for a `<style:style>` of `family="text"`. Tri-state
/// because spans inherit from parent when a property isn't explicitly set.
#[derive(Default, Clone)]
struct StyleAttrs {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    color: Option<[u8; 3]>,
}

#[derive(Default)]
struct StyleTable {
    /// `style:name` → run-style attrs. Used for `<text:span>` and on
    /// run-family style refs from `<text:p>` paragraph styles.
    text: HashMap<String, StyleAttrs>,
    /// `style:name` → heading outline level when the paragraph style
    /// itself encodes a heading (Word→ODT exports sometimes use a
    /// "Heading_20_1" style on a plain `<text:p>` instead of `<text:h>`).
    para_heading_level: HashMap<String, u8>,
}

fn parse_content(xml: &str, metadata: DocumentMetadata) -> Result<Doc> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut styles = StyleTable::default();
    let mut state = WalkState::Top;
    let mut buf = Vec::new();

    let mut blocks: Vec<Block> = Vec::new();
    let mut paragraph_count = 0usize;
    let mut word_count = 0usize;
    let mut image_count = 0usize;
    let mut tbl_stack: Vec<TableState> = Vec::new();
    let mut list_depth: u8 = 0;
    let mut pending_list_marker = false;

    loop {
        let evt = reader
            .read_event_into(&mut buf)
            .map_err(|e| anyhow!("ODT XML error: {e}"))?;
        match evt {
            Event::Start(e) => {
                let tag = local_name(e.name());
                match state {
                    WalkState::Top => match tag.as_slice() {
                        b"automatic-styles" | b"styles" => {
                            state = WalkState::Styles(StylesWalk::default());
                        }
                        b"body" => state = WalkState::Body,
                        _ => {}
                    },
                    WalkState::Styles(_) => {
                        if let WalkState::Styles(sw) = &mut state {
                            styles_handle_start(sw, &mut styles, &tag, &e);
                        }
                    }
                    WalkState::Body => {
                        body_handle_start(
                            &tag,
                            &e,
                            &styles,
                            &mut blocks,
                            &mut tbl_stack,
                            &mut list_depth,
                            &mut pending_list_marker,
                            &mut paragraph_count,
                            &mut word_count,
                            &mut image_count,
                        );
                    }
                    WalkState::Paragraph(_) => {
                        body_handle_start(
                            &tag,
                            &e,
                            &styles,
                            &mut blocks,
                            &mut tbl_stack,
                            &mut list_depth,
                            &mut pending_list_marker,
                            &mut paragraph_count,
                            &mut word_count,
                            &mut image_count,
                        );
                    }
                }
                if let WalkState::Body = state {
                    body_after_start(
                        &tag,
                        &e,
                        &styles,
                        &mut state,
                        &mut tbl_stack,
                        &mut list_depth,
                        &mut pending_list_marker,
                    );
                } else if let WalkState::Paragraph(_) = state {
                    body_after_start_in_para(&tag, &e, &styles, &mut state);
                }
            }
            Event::Empty(e) => {
                let tag = local_name(e.name());
                if let WalkState::Styles(sw) = &mut state {
                    styles_handle_start(sw, &mut styles, &tag, &e);
                    styles_handle_end(sw, &mut styles, &tag);
                } else {
                    handle_empty(&tag, &e, &mut state, &mut image_count);
                }
            }
            Event::Text(t) => {
                if let WalkState::Paragraph(ps) = &mut state
                    && ps.drawing_depth == 0
                {
                    let s = t
                        .xml_content()
                        .map_err(|e| anyhow!("ODT text decode: {e}"))?
                        .into_owned();
                    push_text(ps, &s);
                }
            }
            Event::End(e) => {
                let tag = local_name(e.name());
                match state {
                    WalkState::Top => {}
                    WalkState::Styles(_) => {
                        if matches!(tag.as_slice(), b"automatic-styles" | b"styles") {
                            state = WalkState::Top;
                        } else if let WalkState::Styles(sw) = &mut state {
                            styles_handle_end(sw, &mut styles, &tag);
                        }
                    }
                    WalkState::Body => match tag.as_slice() {
                        b"body" => state = WalkState::Top,
                        b"list" => list_depth = list_depth.saturating_sub(1),
                        b"table" => {
                            if let Some(t) = tbl_stack.pop() {
                                blocks.push(Block::Table(t.rows));
                            }
                        }
                        b"table-row" => {
                            if let Some(t) = tbl_stack.last_mut()
                                && let Some(row) = t.current_row.take()
                            {
                                t.rows.push(row);
                            }
                        }
                        b"table-cell" => {
                            if let Some(t) = tbl_stack.last_mut() {
                                t.in_cell = false;
                                let cell_paragraphs = std::mem::take(&mut t.cell_paragraphs);
                                if let Some(row) = t.current_row.as_mut() {
                                    row.push(merge_paragraphs(cell_paragraphs));
                                }
                            }
                        }
                        _ => {}
                    },
                    WalkState::Paragraph(_) => match tag.as_slice() {
                        b"p" | b"h" => {
                            let WalkState::Paragraph(ps) =
                                std::mem::replace(&mut state, WalkState::Body)
                            else {
                                unreachable!()
                            };
                            let para = finish_paragraph(ps, list_depth, pending_list_marker);
                            pending_list_marker = false;
                            paragraph_count += 1;
                            word_count += count_words(&para.runs);
                            if let Some(t) = tbl_stack.last_mut()
                                && t.in_cell
                            {
                                t.cell_paragraphs.push(para);
                            } else {
                                blocks.push(Block::Paragraph(para));
                            }
                        }
                        b"span" => {
                            if let WalkState::Paragraph(ps) = &mut state {
                                ps.style_stack.pop();
                            }
                        }
                        b"a" => {
                            if let WalkState::Paragraph(ps) = &mut state {
                                ps.hyperlink_depth = ps.hyperlink_depth.saturating_sub(1);
                                // Mark trailing runs added under the link
                                // with underline. Simpler: we set underline
                                // on each run as we add it (see push_text).
                            }
                        }
                        b"frame" | b"object" => {
                            if let WalkState::Paragraph(ps) = &mut state {
                                ps.drawing_depth = ps.drawing_depth.saturating_sub(1);
                                if ps.drawing_depth == 0
                                    && let Some(name) = ps.pending_image_name.take()
                                {
                                    image_count += 1;
                                    ps.runs.push(Run {
                                        text: format!("[Image: {name}]"),
                                        italic: true,
                                        ..Run::default()
                                    });
                                }
                            }
                        }
                        _ => {}
                    },
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(Doc {
        metadata,
        blocks,
        paragraph_count,
        word_count,
        image_count,
    })
}

// ---------------------------------------------------------------------------
// Walk state
// ---------------------------------------------------------------------------

enum WalkState {
    Top,
    Styles(StylesWalk),
    Body,
    Paragraph(ParaState),
}

#[derive(Default)]
struct StylesWalk {
    /// Currently-open `<style:style>` definition, if any.
    current_name: Option<String>,
    current_family: Option<String>,
    current_attrs: StyleAttrs,
    /// Paragraph-style heading level (derived from style name like
    /// "Heading_20_1") for the currently-open paragraph style.
    current_para_heading: Option<u8>,
}

#[derive(Default)]
struct ParaState {
    heading_level: Option<u8>,
    /// Per-span style frames pushed on `<text:span>` open. The active
    /// run style is the top frame merged onto the paragraph default.
    style_stack: Vec<StyleAttrs>,
    /// Style inherited from the paragraph's own `text:style-name` (the
    /// text-family attrs at that style). Applies to runs that aren't
    /// inside a span.
    paragraph_run_style: StyleAttrs,
    hyperlink_depth: usize,
    drawing_depth: usize,
    /// Image basename pulled from `<draw:image xlink:href>` inside an
    /// open `<draw:frame>`.
    pending_image_name: Option<String>,
    runs: Vec<Run>,
}

#[derive(Default)]
struct TableState {
    rows: Vec<Vec<Paragraph>>,
    current_row: Option<Vec<Paragraph>>,
    in_cell: bool,
    cell_paragraphs: Vec<Paragraph>,
}

// ---------------------------------------------------------------------------
// Styles walk
// ---------------------------------------------------------------------------

fn styles_handle_start(
    sw: &mut StylesWalk,
    _table: &mut StyleTable,
    tag: &[u8],
    e: &BytesStart<'_>,
) {
    match tag {
        b"style" => {
            sw.current_name = attr_val_local(e, b"name");
            sw.current_family = attr_val_local(e, b"family");
            sw.current_attrs = StyleAttrs::default();
            sw.current_para_heading = sw
                .current_name
                .as_deref()
                .and_then(heading_level_from_style_name);
        }
        b"text-properties" => {
            if let Some(v) = attr_val_local(e, b"font-weight") {
                sw.current_attrs.bold = Some(v == "bold");
            }
            if let Some(v) = attr_val_local(e, b"font-style") {
                sw.current_attrs.italic = Some(v == "italic" || v == "oblique");
            }
            if let Some(v) = attr_val_local(e, b"text-underline-style") {
                sw.current_attrs.underline = Some(!matches!(v.as_str(), "none" | ""));
            }
            if let Some(v) = attr_val_local(e, b"text-line-through-style") {
                sw.current_attrs.strike = Some(!matches!(v.as_str(), "none" | ""));
            }
            if let Some(v) = attr_val_local(e, b"color") {
                sw.current_attrs.color = parse_hex_color(&v);
            }
        }
        _ => {}
    }
}

fn styles_handle_end(sw: &mut StylesWalk, table: &mut StyleTable, tag: &[u8]) {
    if tag == b"style"
        && let Some(name) = sw.current_name.take()
    {
        let family = sw.current_family.take();
        match family.as_deref() {
            Some("text") => {
                table.text.insert(name.clone(), sw.current_attrs.clone());
            }
            Some("paragraph") => {
                // Paragraph styles can carry text-properties too — that
                // becomes the default run style for plain text inside
                // the paragraph (not inside any span).
                table.text.insert(name.clone(), sw.current_attrs.clone());
                if let Some(level) = sw.current_para_heading.take() {
                    table.para_heading_level.insert(name, level);
                }
            }
            _ => {}
        }
        sw.current_attrs = StyleAttrs::default();
    }
}

// ---------------------------------------------------------------------------
// Body walk — paragraph / heading entry
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn body_handle_start(
    tag: &[u8],
    e: &BytesStart<'_>,
    styles: &StyleTable,
    _blocks: &mut [Block],
    tbl_stack: &mut Vec<TableState>,
    list_depth: &mut u8,
    pending_list_marker: &mut bool,
    _paragraph_count: &mut usize,
    _word_count: &mut usize,
    _image_count: &mut usize,
) {
    match tag {
        b"table" => tbl_stack.push(TableState::default()),
        b"table-row" => {
            if let Some(t) = tbl_stack.last_mut() {
                t.current_row = Some(Vec::new());
            }
        }
        b"table-cell" => {
            if let Some(t) = tbl_stack.last_mut() {
                t.in_cell = true;
                t.cell_paragraphs.clear();
            }
        }
        b"list" => {
            *list_depth = list_depth.saturating_add(1);
        }
        b"list-item" => {
            *pending_list_marker = true;
        }
        _ => {
            // Paragraph / heading entry is handled in body_after_start.
            let _ = (e, styles);
        }
    }
}

/// Called after `body_handle_start`. Promotes the walk state to
/// `Paragraph` when entering `<text:p>` / `<text:h>`.
fn body_after_start(
    tag: &[u8],
    e: &BytesStart<'_>,
    styles: &StyleTable,
    state: &mut WalkState,
    _tbl_stack: &mut [TableState],
    _list_depth: &mut u8,
    _pending_list_marker: &mut bool,
) {
    match tag {
        b"p" => {
            let style_name = attr_val_local(e, b"style-name");
            let mut ps = ParaState::default();
            if let Some(name) = &style_name {
                if let Some(attrs) = styles.text.get(name) {
                    ps.paragraph_run_style = attrs.clone();
                }
                if let Some(level) = styles.para_heading_level.get(name) {
                    ps.heading_level = Some(*level);
                }
            }
            *state = WalkState::Paragraph(ps);
        }
        b"h" => {
            let outline = attr_val_local(e, b"outline-level")
                .and_then(|v| v.parse::<u8>().ok())
                .filter(|n| (1..=6).contains(n))
                .unwrap_or(1);
            let style_name = attr_val_local(e, b"style-name");
            let mut ps = ParaState {
                heading_level: Some(outline),
                ..ParaState::default()
            };
            if let Some(name) = &style_name
                && let Some(attrs) = styles.text.get(name)
            {
                ps.paragraph_run_style = attrs.clone();
            }
            *state = WalkState::Paragraph(ps);
        }
        _ => {}
    }
}

/// Called for elements opened while a paragraph is being built (spans,
/// hyperlinks, drawings, line-breaks-as-empty handled elsewhere).
fn body_after_start_in_para(
    tag: &[u8],
    e: &BytesStart<'_>,
    styles: &StyleTable,
    state: &mut WalkState,
) {
    let WalkState::Paragraph(ps) = state else {
        return;
    };
    match tag {
        b"span" => {
            let attrs = attr_val_local(e, b"style-name")
                .and_then(|n| styles.text.get(&n).cloned())
                .unwrap_or_default();
            ps.style_stack.push(attrs);
        }
        b"a" => {
            ps.hyperlink_depth = ps.hyperlink_depth.saturating_add(1);
        }
        b"frame" | b"object" => {
            ps.drawing_depth = ps.drawing_depth.saturating_add(1);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Empty-element handling (line break, tab, spaces, image, etc.)
// ---------------------------------------------------------------------------

fn handle_empty(tag: &[u8], e: &BytesStart<'_>, state: &mut WalkState, _image_count: &mut usize) {
    let WalkState::Paragraph(ps) = state else {
        return;
    };
    match tag {
        b"line-break" => push_text(ps, "\n"),
        b"tab" => push_text(ps, "    "),
        b"s" => {
            let n: usize = attr_val_local(e, b"c")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1);
            push_text(ps, &" ".repeat(n));
        }
        b"image" => {
            if ps.drawing_depth > 0
                && let Some(href) = attr_val_local(e, b"href")
            {
                let basename = href.rsplit('/').next().unwrap_or(&href).to_string();
                ps.pending_image_name = Some(basename);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Text push — apply active style stack onto a new Run and append.
// ---------------------------------------------------------------------------

fn push_text(ps: &mut ParaState, text: &str) {
    if text.is_empty() || ps.drawing_depth > 0 {
        return;
    }
    let mut style = ps.paragraph_run_style.clone();
    for frame in &ps.style_stack {
        merge_attrs(&mut style, frame);
    }
    let mut run = Run {
        text: text.to_string(),
        ..Run::default()
    };
    if style.bold == Some(true) {
        run.bold = true;
    }
    if style.italic == Some(true) {
        run.italic = true;
    }
    if style.underline == Some(true) {
        run.underline = true;
    }
    if style.strike == Some(true) {
        run.strike = true;
    }
    if let Some(c) = style.color {
        run.color = Some(c);
    }
    if ps.hyperlink_depth > 0 {
        run.underline = true;
    }
    ps.runs.push(run);
}

fn merge_attrs(base: &mut StyleAttrs, override_: &StyleAttrs) {
    if override_.bold.is_some() {
        base.bold = override_.bold;
    }
    if override_.italic.is_some() {
        base.italic = override_.italic;
    }
    if override_.underline.is_some() {
        base.underline = override_.underline;
    }
    if override_.strike.is_some() {
        base.strike = override_.strike;
    }
    if override_.color.is_some() {
        base.color = override_.color;
    }
}

fn finish_paragraph(ps: ParaState, list_depth: u8, list_marker: bool) -> Paragraph {
    Paragraph {
        heading_level: ps.heading_level,
        list_marker: if list_marker {
            Some("\u{2022}".to_string())
        } else {
            None
        },
        indent_level: list_depth.saturating_sub(if list_marker { 1 } else { 0 }),
        runs: ps.runs,
    }
}

// ---------------------------------------------------------------------------
// meta.xml — Dublin Core + meta:* fields
// ---------------------------------------------------------------------------

fn parse_meta(xml: &str) -> DocumentMetadata {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = DocumentMetadata::default();

    let mut current_field: Option<MetaField> = None;
    let mut current_text = String::new();
    let mut keywords: Vec<String> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                current_field = meta_field_from_qname(e.name());
                current_text.clear();
            }
            Ok(Event::Text(t)) => {
                if current_field.is_some()
                    && let Ok(decoded) = t.xml_content()
                {
                    current_text.push_str(&decoded);
                }
            }
            Ok(Event::End(_)) => {
                if let Some(field) = current_field.take() {
                    let value = current_text.trim().to_string();
                    if !value.is_empty() {
                        match field {
                            MetaField::Keyword => keywords.push(value),
                            other => assign_meta(&mut out, other, value),
                        }
                    }
                    current_text.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    if !keywords.is_empty() {
        out.keywords = Some(keywords.join(", "));
    }
    out
}

#[derive(Clone, Copy)]
enum MetaField {
    Title,
    Creator,
    Subject,
    Description,
    Keyword,
    Created,
    Modified,
}

fn meta_field_from_qname(name: QName<'_>) -> Option<MetaField> {
    Some(match name.as_ref() {
        b"dc:title" => MetaField::Title,
        b"dc:creator" => MetaField::Creator,
        b"dc:subject" => MetaField::Subject,
        b"dc:description" => MetaField::Description,
        b"meta:keyword" => MetaField::Keyword,
        b"meta:creation-date" => MetaField::Created,
        b"dc:date" => MetaField::Modified,
        _ => return None,
    })
}

fn assign_meta(meta: &mut DocumentMetadata, field: MetaField, value: String) {
    let slot = match field {
        MetaField::Title => &mut meta.title,
        MetaField::Creator => &mut meta.creator,
        MetaField::Subject => &mut meta.subject,
        MetaField::Description => &mut meta.description,
        MetaField::Created => &mut meta.created,
        MetaField::Modified => &mut meta.modified,
        MetaField::Keyword => unreachable!("keywords aggregate in parse_meta"),
    };
    if slot.is_none() {
        *slot = Some(value);
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn local_name(name: QName<'_>) -> Vec<u8> {
    name.local_name().as_ref().to_vec()
}

fn attr_val_local(e: &BytesStart<'_>, want_local: &[u8]) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.local_name().as_ref() == want_local {
            return attr.unescape_value().ok().map(|s| s.into_owned());
        }
    }
    None
}

/// Style names from MS Office and LibreOffice both prefix headings as
/// `Heading_20_N` (the `_20_` is the encoded space). Match that and
/// the plainer `Heading N` form.
fn heading_level_from_style_name(name: &str) -> Option<u8> {
    let normalised = name.replace("_20_", " ");
    let stripped = normalised.strip_prefix("Heading")?.trim_start();
    let n: u8 = stripped.parse().ok()?;
    (1..=6).contains(&n).then_some(n)
}

/// `fo:color` is `#RRGGBB` (always 7 chars). Returns `None` for
/// `transparent` / empty / malformed.
fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some([r, g, b])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::document::ast::Block;
    use std::path::PathBuf;

    fn fixture() -> InputSource {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-data/sample.odt");
        assert!(path.exists(), "fixture missing: {}", path.display());
        InputSource::File(path)
    }

    #[test]
    fn parses_metadata() {
        let doc = open(&fixture()).expect("parse");
        assert_eq!(doc.metadata.title.as_deref(), Some("Peek sample ODT"));
        assert_eq!(doc.metadata.creator.as_deref(), Some("Tuukka Haapasalo"));
        assert_eq!(doc.metadata.subject.as_deref(), Some("Test fixture"));
        assert!(
            doc.metadata
                .description
                .as_deref()
                .is_some_and(|d| d.contains("ODT")),
        );
        assert_eq!(
            doc.metadata.keywords.as_deref(),
            Some("peek, odt, fixture"),
            "keyword aggregation must join all <meta:keyword> values",
        );
        assert_eq!(doc.metadata.created.as_deref(), Some("2026-05-12T10:00:00"),);
        assert_eq!(
            doc.metadata.modified.as_deref(),
            Some("2026-05-12T10:30:00"),
        );
    }

    #[test]
    fn counts_paragraphs_and_words() {
        let doc = open(&fixture()).expect("parse");
        // 2 headings + 2 body paragraphs (prose-with-spans, linked-phrase)
        // + 3 list-item paragraphs + 1 image-bearing paragraph + 4
        // table-cell paragraphs (merged into table rows, but each cell
        // paragraph counts).
        assert_eq!(doc.paragraph_count, 12, "{}", doc.paragraph_count);
        assert!(doc.word_count >= 20, "word_count = {}", doc.word_count);
        assert_eq!(doc.image_count, 1);
    }

    #[test]
    fn heading_levels_resolved() {
        let doc = open(&fixture()).expect("parse");
        let headings: Vec<u8> = doc
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => p.heading_level,
                _ => None,
            })
            .collect();
        assert_eq!(headings, vec![1, 2]);
    }

    #[test]
    fn span_styles_resolved_through_automatic_styles() {
        let doc = open(&fixture()).expect("parse");
        // First body paragraph has the bold + italic-red + strike spans.
        let para = doc
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.heading_level.is_none() => Some(p),
                _ => None,
            })
            .next()
            .expect("body paragraph");
        let runs = &para.runs;
        let any_bold = runs.iter().any(|r| r.bold && r.text.contains("bold"));
        let any_red_italic = runs
            .iter()
            .any(|r| r.italic && r.color == Some([0xcc, 0x33, 0x33]));
        let any_strike = runs.iter().any(|r| r.strike);
        assert!(any_bold, "bold span not resolved: {:?}", run_dump(runs));
        assert!(
            any_red_italic,
            "italic+red span not resolved: {:?}",
            run_dump(runs),
        );
        assert!(any_strike, "strike span not resolved: {:?}", run_dump(runs));
    }

    #[test]
    fn hyperlink_forces_underline() {
        let doc = open(&fixture()).expect("parse");
        let underlined: bool = doc.blocks.iter().any(|b| match b {
            Block::Paragraph(p) => p
                .runs
                .iter()
                .any(|r| r.underline && r.text.contains("linked")),
            _ => false,
        });
        assert!(underlined, "hyperlink anchor must underline its contents");
    }

    #[test]
    fn list_items_get_bullet_marker_with_nesting() {
        let doc = open(&fixture()).expect("parse");
        let list_items: Vec<&Paragraph> = doc
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.list_marker.is_some() => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(list_items.len(), 3, "expected three bullet items");
        let depths: Vec<u8> = list_items.iter().map(|p| p.indent_level).collect();
        assert_eq!(depths, vec![0, 0, 1], "nested item must indent deeper");
    }

    #[test]
    fn image_becomes_marker_run() {
        let doc = open(&fixture()).expect("parse");
        let has_marker = doc.blocks.iter().any(|b| match b {
            Block::Paragraph(p) => p
                .runs
                .iter()
                .any(|r| r.italic && r.text == "[Image: diagram.png]"),
            _ => false,
        });
        assert!(has_marker, "draw:image must emit [Image: basename] run");
    }

    #[test]
    fn table_rows_captured_as_table_block() {
        let doc = open(&fixture()).expect("parse");
        let table = doc
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Table(rows) => Some(rows),
                _ => None,
            })
            .expect("table block present");
        assert_eq!(table.len(), 2);
        assert_eq!(table[0].len(), 2);
        let cell_text: String = table[0][0].runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(cell_text, "row1col1");
    }

    #[derive(Debug)]
    #[allow(dead_code)]
    struct RunDump {
        text: String,
        bold: bool,
        italic: bool,
        strike: bool,
        color: Option<[u8; 3]>,
    }

    fn run_dump(runs: &[Run]) -> Vec<RunDump> {
        runs.iter()
            .map(|r| RunDump {
                text: r.text.clone(),
                bold: r.bold,
                italic: r.italic,
                strike: r.strike,
                color: r.color,
            })
            .collect()
    }
}
