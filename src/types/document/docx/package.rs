//! DOCX package open + AST conversion.
//!
//! Hand-walks the DOCX with `quick-xml` rather than going through a
//! full WordprocessingML deserializer. The full deserializer (the
//! `docx-rust` crate) tripped on real-world Word documents because
//! many numeric attributes on real files carry non-numeric values
//! (`"auto"`, `"none"`, `"true"`) that strict `isize` / `i64` types
//! can't decode. Walking events directly lets us pick out only what
//! the read view actually needs (paragraph style, run formatting,
//! literal text, image refs) and skip everything else.

use std::collections::HashMap;
use std::io::Read;

use anyhow::{Context, Result, anyhow};
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::reader::Reader;
use zip::ZipArchive;

use crate::input::InputSource;
use crate::types::archive::reader::{ReadSeek, open_seekable};
use crate::types::document::DocumentMetadata;
use crate::types::document::ast::{Block, Doc, Paragraph, Run, count_words, merge_paragraphs};

pub(crate) fn open(source: &InputSource) -> Result<Doc> {
    let reader = open_seekable(source).context("failed to open DOCX container")?;
    let mut zip = ZipArchive::new(reader).context("failed to read DOCX archive")?;

    let document_xml =
        read_entry(&mut zip, "word/document.xml").context("DOCX missing word/document.xml")?;
    let core_xml = read_entry(&mut zip, "docProps/core.xml").ok();
    let rels_xml = read_entry(&mut zip, "word/_rels/document.xml.rels").ok();

    let metadata = core_xml.as_deref().map(parse_core_xml).unwrap_or_default();
    let image_rels = rels_xml
        .as_deref()
        .map(parse_image_rels)
        .unwrap_or_default();

    parse_document(&document_xml, metadata, &image_rels)
}

fn read_entry(zip: &mut ZipArchive<Box<dyn ReadSeek>>, path: &str) -> Result<String> {
    let mut file = zip
        .by_name(path)
        .with_context(|| format!("DOCX entry {path:?} not found"))?;
    let mut buf = String::with_capacity(file.size() as usize);
    file.read_to_string(&mut buf)?;
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Document body — the bulk of the work
// ---------------------------------------------------------------------------

/// Walk `word/document.xml`. The reader doesn't preserve namespaces
/// because every part of the file uses the same `w:` prefix in
/// practice; matching against the literal local-name is enough.
fn parse_document(
    xml: &str,
    metadata: DocumentMetadata,
    image_rels: &HashMap<String, String>,
) -> Result<Doc> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut blocks: Vec<Block> = Vec::new();
    let mut paragraph_count = 0usize;
    let mut word_count = 0usize;
    let mut image_count = 0usize;

    // Stack of in-progress parsing states. `Top` is the document
    // body; tables push their own state onto the stack so nested
    // paragraphs route to the active cell instead of the body.
    let mut state = ParseState::Top;
    let mut tbl_stack: Vec<TableState> = Vec::new();
    let mut buf = Vec::new();

    loop {
        let evt = reader
            .read_event_into(&mut buf)
            .map_err(|e| anyhow!("DOCX XML error: {e}"))?;
        match evt {
            Event::Start(e) => {
                let tag = local_name(e.name());
                match tag.as_slice() {
                    b"p" => state = ParseState::Paragraph(ParaState::default()),
                    b"pPr" => {
                        if let ParseState::Paragraph(ps) = &mut state {
                            ps.in_pPr = true;
                        }
                    }
                    b"pStyle" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && ps.in_pPr
                            && let Some(val) = attr_val(&e, b"val")
                        {
                            ps.heading_level = heading_level_from_style(&val);
                        }
                    }
                    b"numPr" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && ps.in_pPr
                        {
                            ps.list = true;
                        }
                    }
                    b"ilvl" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && ps.in_pPr
                            && let Some(val) = attr_val(&e, b"val")
                            && let Ok(n) = val.parse::<u8>()
                        {
                            ps.list_indent = n;
                        }
                    }
                    b"r" => {
                        if let ParseState::Paragraph(ps) = &mut state {
                            ps.cur_run = Some(RunState::default());
                        }
                    }
                    b"rPr" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && let Some(rs) = ps.cur_run.as_mut()
                        {
                            rs.in_rPr = true;
                        }
                    }
                    b"b" => set_run_flag(&mut state, &e, |rs, v| rs.style.bold = v),
                    b"i" => set_run_flag(&mut state, &e, |rs, v| rs.style.italic = v),
                    b"strike" => set_run_flag(&mut state, &e, |rs, v| rs.style.strike = v),
                    b"u" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && let Some(rs) = ps.cur_run.as_mut()
                            && rs.in_rPr
                        {
                            // `<w:u/>` with no val or `val="single"`
                            // both mean underlined; only `none` turns
                            // it off.
                            let val = attr_val(&e, b"val");
                            rs.style.underline =
                                !matches!(val.as_deref(), Some("none" | "false" | "0"));
                        }
                    }
                    b"color" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && let Some(rs) = ps.cur_run.as_mut()
                            && rs.in_rPr
                            && let Some(val) = attr_val(&e, b"val")
                        {
                            rs.style.color = parse_hex_rgb(&val);
                        }
                    }
                    b"hyperlink" => {
                        if let ParseState::Paragraph(ps) = &mut state {
                            ps.in_hyperlink_depth += 1;
                        }
                    }
                    b"t" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && let Some(rs) = ps.cur_run.as_mut()
                        {
                            rs.collecting_text = true;
                        }
                    }
                    b"drawing" | b"pict" | b"object" => {
                        // The whole drawing subtree hides text / refs
                        // we don't care about; toggle a guard so any
                        // `<w:t>` text inside (rare — usually alt) is
                        // not collected as run text.
                        if let ParseState::Paragraph(ps) = &mut state {
                            ps.drawing_depth += 1;
                            scan_drawing_open(&mut ps.pending_image_rid, &e);
                        }
                    }
                    b"blip" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && ps.drawing_depth > 0
                            && let Some(rid) = attr_val_ns(&e, b"embed")
                        {
                            ps.pending_image_rid = Some(rid);
                        }
                    }
                    b"tbl" => {
                        tbl_stack.push(TableState::default());
                    }
                    b"tr" => {
                        if let Some(t) = tbl_stack.last_mut() {
                            t.current_row = Some(Vec::new());
                        }
                    }
                    b"tc" => {
                        if let Some(t) = tbl_stack.last_mut() {
                            t.in_cell = true;
                            t.cell_paragraphs.clear();
                        }
                    }
                    _ => {}
                }
            }
            Event::Empty(e) => {
                let tag = local_name(e.name());
                match tag.as_slice() {
                    b"pStyle" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && ps.in_pPr
                            && let Some(val) = attr_val(&e, b"val")
                        {
                            ps.heading_level = heading_level_from_style(&val);
                        }
                    }
                    b"ilvl" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && ps.in_pPr
                            && let Some(val) = attr_val(&e, b"val")
                            && let Ok(n) = val.parse::<u8>()
                        {
                            ps.list_indent = n;
                        }
                    }
                    b"numPr" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && ps.in_pPr
                        {
                            ps.list = true;
                        }
                    }
                    b"b" => set_run_flag(&mut state, &e, |rs, v| rs.style.bold = v),
                    b"i" => set_run_flag(&mut state, &e, |rs, v| rs.style.italic = v),
                    b"strike" => set_run_flag(&mut state, &e, |rs, v| rs.style.strike = v),
                    b"u" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && let Some(rs) = ps.cur_run.as_mut()
                            && rs.in_rPr
                        {
                            let val = attr_val(&e, b"val");
                            rs.style.underline =
                                !matches!(val.as_deref(), Some("none" | "false" | "0"));
                        }
                    }
                    b"color" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && let Some(rs) = ps.cur_run.as_mut()
                            && rs.in_rPr
                            && let Some(val) = attr_val(&e, b"val")
                        {
                            rs.style.color = parse_hex_rgb(&val);
                        }
                    }
                    b"br" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && let Some(rs) = ps.cur_run.as_mut()
                        {
                            rs.runs.push(Run {
                                text: "\n".to_string(),
                                ..rs.style.clone()
                            });
                        }
                    }
                    b"tab" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && let Some(rs) = ps.cur_run.as_mut()
                        {
                            rs.runs.push(Run {
                                text: "    ".to_string(),
                                ..rs.style.clone()
                            });
                        }
                    }
                    b"cr" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && let Some(rs) = ps.cur_run.as_mut()
                        {
                            rs.runs.push(Run {
                                text: "\n".to_string(),
                                ..rs.style.clone()
                            });
                        }
                    }
                    b"blip" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && ps.drawing_depth > 0
                            && let Some(rid) = attr_val_ns(&e, b"embed")
                        {
                            ps.pending_image_rid = Some(rid);
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(t) => {
                if let ParseState::Paragraph(ps) = &mut state
                    && let Some(rs) = ps.cur_run.as_mut()
                    && rs.collecting_text
                    && ps.drawing_depth == 0
                {
                    let s = t
                        .xml_content()
                        .map_err(|e| anyhow!("DOCX text decode: {e}"))?
                        .into_owned();
                    rs.runs.push(Run {
                        text: s,
                        ..rs.style.clone()
                    });
                }
            }
            Event::End(e) => {
                let tag = local_name(e.name());
                match tag.as_slice() {
                    b"pPr" => {
                        if let ParseState::Paragraph(ps) = &mut state {
                            ps.in_pPr = false;
                        }
                    }
                    b"rPr" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && let Some(rs) = ps.cur_run.as_mut()
                        {
                            rs.in_rPr = false;
                        }
                    }
                    b"t" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && let Some(rs) = ps.cur_run.as_mut()
                        {
                            rs.collecting_text = false;
                        }
                    }
                    b"r" => {
                        if let ParseState::Paragraph(ps) = &mut state
                            && let Some(mut rs) = ps.cur_run.take()
                        {
                            // Mark all runs inside an open hyperlink
                            // as underlined to signal the link.
                            if ps.in_hyperlink_depth > 0 {
                                for r in &mut rs.runs {
                                    r.underline = true;
                                }
                            }
                            ps.runs.extend(rs.runs);
                        }
                    }
                    b"hyperlink" => {
                        if let ParseState::Paragraph(ps) = &mut state {
                            ps.in_hyperlink_depth = ps.in_hyperlink_depth.saturating_sub(1);
                        }
                    }
                    b"drawing" | b"pict" | b"object" => {
                        if let ParseState::Paragraph(ps) = &mut state {
                            ps.drawing_depth = ps.drawing_depth.saturating_sub(1);
                            if ps.drawing_depth == 0
                                && let Some(rid) = ps.pending_image_rid.take()
                            {
                                let basename = image_rels
                                    .get(&rid)
                                    .map(|t| t.rsplit('/').next().unwrap_or(t).to_string())
                                    .unwrap_or_else(|| format!("image ({rid})"));
                                image_count += 1;
                                ps.runs.push(Run {
                                    text: format!("[Image: {basename}]"),
                                    italic: true,
                                    ..Run::default()
                                });
                            }
                        }
                    }
                    b"p" => {
                        let para = match std::mem::replace(&mut state, ParseState::Top) {
                            ParseState::Paragraph(ps) => finish_paragraph(ps),
                            other => {
                                state = other;
                                continue;
                            }
                        };
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
                    b"tc" => {
                        if let Some(t) = tbl_stack.last_mut() {
                            t.in_cell = false;
                            let cell_paragraphs = std::mem::take(&mut t.cell_paragraphs);
                            if let Some(row) = t.current_row.as_mut() {
                                row.push(merge_paragraphs(cell_paragraphs));
                            }
                        }
                    }
                    b"tr" => {
                        if let Some(t) = tbl_stack.last_mut()
                            && let Some(row) = t.current_row.take()
                        {
                            t.rows.push(row);
                        }
                    }
                    b"tbl" => {
                        if let Some(t) = tbl_stack.pop() {
                            blocks.push(Block::Table(t.rows));
                        }
                    }
                    _ => {}
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
// In-flight parser state
// ---------------------------------------------------------------------------

enum ParseState {
    Top,
    Paragraph(ParaState),
}

#[derive(Default)]
#[allow(non_snake_case)]
struct ParaState {
    heading_level: Option<u8>,
    list: bool,
    list_indent: u8,
    in_pPr: bool,
    in_hyperlink_depth: usize,
    drawing_depth: usize,
    /// Image rId scraped from the most-recent `<a:blip r:embed="…"/>`
    /// inside the open drawing; resolved on </w:drawing> end.
    pending_image_rid: Option<String>,
    cur_run: Option<RunState>,
    runs: Vec<Run>,
}

#[derive(Default)]
#[allow(non_snake_case)]
struct RunState {
    style: Run,
    in_rPr: bool,
    collecting_text: bool,
    runs: Vec<Run>,
}

#[derive(Default)]
struct TableState {
    rows: Vec<Vec<Paragraph>>,
    current_row: Option<Vec<Paragraph>>,
    in_cell: bool,
    cell_paragraphs: Vec<Paragraph>,
}

fn finish_paragraph(ps: ParaState) -> Paragraph {
    Paragraph {
        heading_level: ps.heading_level,
        list_marker: ps.list.then(|| "\u{2022}".to_string()),
        indent_level: ps.list_indent,
        runs: ps.runs,
    }
}

// ---------------------------------------------------------------------------
// Per-element helpers
// ---------------------------------------------------------------------------

fn set_run_flag(
    state: &mut ParseState,
    e: &quick_xml::events::BytesStart<'_>,
    mut f: impl FnMut(&mut RunState, bool),
) {
    if let ParseState::Paragraph(ps) = state
        && let Some(rs) = ps.cur_run.as_mut()
        && rs.in_rPr
    {
        let val = attr_val(e, b"val");
        let on = match val.as_deref() {
            None => true,
            Some(v) => !matches!(v, "0" | "false" | "off"),
        };
        f(rs, on);
    }
}

fn scan_drawing_open(_pending: &mut Option<String>, _e: &quick_xml::events::BytesStart<'_>) {
    // No work on open — the `<a:blip r:embed="…"/>` event is what
    // surfaces the rId. Hook kept as an extension point for future
    // anchor/inline fallback scraping.
}

fn local_name(name: QName<'_>) -> Vec<u8> {
    name.local_name().as_ref().to_vec()
}

fn attr_val(e: &quick_xml::events::BytesStart<'_>, want_local: &[u8]) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.local_name().as_ref() == want_local {
            return attr.unescape_value().ok().map(|s| s.into_owned());
        }
    }
    None
}

/// Like [`attr_val`], but checks the attribute's local name only —
/// good for namespaced attributes like `r:embed` where the prefix
/// differs across files.
fn attr_val_ns(e: &quick_xml::events::BytesStart<'_>, want_local: &[u8]) -> Option<String> {
    attr_val(e, want_local)
}

fn heading_level_from_style(style: &str) -> Option<u8> {
    let stripped = style.strip_prefix("Heading")?;
    let n: u8 = stripped.parse().ok()?;
    (1..=6).contains(&n).then_some(n)
}

fn parse_hex_rgb(s: &str) -> Option<[u8; 3]> {
    if s.is_empty() || s.eq_ignore_ascii_case("auto") {
        return None;
    }
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some([r, g, b])
}

// ---------------------------------------------------------------------------
// docProps/core.xml — flat Dublin Core / cp metadata
// ---------------------------------------------------------------------------

fn parse_core_xml(xml: &str) -> DocumentMetadata {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = DocumentMetadata::default();

    let mut current_field: Option<CoreField> = None;
    let mut current_text = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                current_field = core_field_from_qname(e.name());
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
                        assign_core(&mut out, field, value);
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
    out
}

#[derive(Clone, Copy)]
enum CoreField {
    Title,
    Creator,
    Subject,
    Description,
    Keywords,
    Created,
    Modified,
}

fn core_field_from_qname(name: QName<'_>) -> Option<CoreField> {
    // Match by full prefixed name so `dc:title` doesn't collide with
    // a hypothetical `cp:title`.
    Some(match name.as_ref() {
        b"dc:title" => CoreField::Title,
        b"dc:creator" => CoreField::Creator,
        b"dc:subject" => CoreField::Subject,
        b"dc:description" => CoreField::Description,
        b"cp:keywords" => CoreField::Keywords,
        b"dcterms:created" => CoreField::Created,
        b"dcterms:modified" => CoreField::Modified,
        _ => return None,
    })
}

fn assign_core(meta: &mut DocumentMetadata, field: CoreField, value: String) {
    let slot = match field {
        CoreField::Title => &mut meta.title,
        CoreField::Creator => &mut meta.creator,
        CoreField::Subject => &mut meta.subject,
        CoreField::Description => &mut meta.description,
        CoreField::Keywords => &mut meta.keywords,
        CoreField::Created => &mut meta.created,
        CoreField::Modified => &mut meta.modified,
    };
    if slot.is_none() {
        *slot = Some(value);
    }
}

// ---------------------------------------------------------------------------
// word/_rels/document.xml.rels — image rId → target path
// ---------------------------------------------------------------------------

fn parse_image_rels(xml: &str) -> HashMap<String, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = HashMap::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e))
                if local_name(e.name()) == b"Relationship" =>
            {
                let mut id = None;
                let mut target = None;
                let mut ty = None;
                for attr in e.attributes().flatten() {
                    match attr.key.as_ref() {
                        b"Id" => id = attr.unescape_value().ok().map(|s| s.into_owned()),
                        b"Target" => target = attr.unescape_value().ok().map(|s| s.into_owned()),
                        b"Type" => ty = attr.unescape_value().ok().map(|s| s.into_owned()),
                        _ => {}
                    }
                }
                if let (Some(id), Some(target), Some(ty)) = (id, target, ty)
                    && ty.contains("image")
                {
                    out.insert(id, target);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}
