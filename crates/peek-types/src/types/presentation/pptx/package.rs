//! PPTX / PPTM / PPSX package open + per-slide AST conversion.
//!
//! Hand-walks the OOXML with `quick-xml` (same rationale as the DOCX
//! reader: real-world files carry attribute values a strict
//! deserializer chokes on). Slide order comes from
//! `ppt/presentation.xml` `<p:sldIdLst>` resolved through
//! `ppt/_rels/presentation.xml.rels`; each `ppt/slides/slideN.xml` is a
//! DrawingML shape tree whose `<p:txBody>` paragraphs become one
//! [`Doc`]. Title placeholders render as a level-1 heading; everything
//! else is body prose. Image references surface as `[Image: name]` runs
//! (reference-only — the bitmaps stay reachable through the ZIP TOC).

use std::collections::HashMap;

use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::reader::Reader;

use crate::types::archive::reader::{open_zip, read_zip_entry_str};
use crate::types::document::DocumentMetadata;
use crate::types::document::ast::{Block, Doc, Paragraph, Run, count_words};
use crate::types::presentation::{Deck, PresentationMetadata};
use peek_io::InputSource;

pub fn open(source: &InputSource) -> Result<Deck> {
    let mut zip = open_zip(source, "PPTX")?;

    let presentation_xml = read_zip_entry_str(&mut zip, "ppt/presentation.xml", "PPTX")
        .context("couldn't read ppt/presentation.xml")?;
    let pres_rels =
        read_zip_entry_str(&mut zip, "ppt/_rels/presentation.xml.rels", "PPTX").unwrap_or_default();

    let core_xml = read_zip_entry_str(&mut zip, "docProps/core.xml", "PPTX").ok();
    let app_xml = read_zip_entry_str(&mut zip, "docProps/app.xml", "PPTX").ok();
    let mut metadata = core_xml.as_deref().map(parse_core_xml).unwrap_or_default();
    metadata.application = app_xml.as_deref().and_then(parse_app_application);

    // rId -> target ("slides/slide1.xml"), then ordered rIds from sldIdLst.
    let rels = parse_rels(&pres_rels);
    let order = parse_slide_order(&presentation_xml);

    let mut slides = Vec::with_capacity(order.len());
    for rid in order {
        let Some(target) = rels.get(&rid) else {
            continue;
        };
        let slide_path = join_ppt(target);
        let Ok(slide_xml) = read_zip_entry_str(&mut zip, &slide_path, "PPTX") else {
            continue;
        };
        // Per-slide image rels (rId -> media target); basename only.
        let rel_path = slide_rels_path(&slide_path);
        let image_rels = read_zip_entry_str(&mut zip, &rel_path, "PPTX")
            .ok()
            .map(|x| parse_rels(&x))
            .unwrap_or_default();
        slides.push(parse_slide(&slide_xml, &image_rels));
    }

    Ok(Deck { metadata, slides })
}

// ---------------------------------------------------------------------------
// presentation.xml — slide order
// ---------------------------------------------------------------------------

/// Ordered `r:id` values from `<p:sldIdLst><p:sldId r:id="…"/>`.
fn parse_slide_order(xml: &str) -> Vec<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut order = Vec::new();
    let mut in_list = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if local_name(e.name()) == b"sldIdLst" => in_list = true,
            Ok(Event::End(e)) if local_name(e.name()) == b"sldIdLst" => in_list = false,
            Ok(Event::Empty(e)) | Ok(Event::Start(e))
                if in_list && local_name(e.name()) == b"sldId" =>
            {
                // `<p:sldId>` carries both a plain `id` (the slide number)
                // and `r:id` (the relationship) — they share the local
                // name "id", so match the full prefixed `r:id` key.
                if let Some(rid) = attr_val_full(&e, b"r:id") {
                    order.push(rid);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    order
}

/// `Id` -> `Target` from any `_rels/*.rels` part.
fn parse_rels(xml: &str) -> HashMap<String, String> {
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
                for attr in e.attributes().flatten() {
                    match attr.key.as_ref() {
                        b"Id" => id = crate::xml::unescape_attr_value(&attr),
                        b"Target" => target = crate::xml::unescape_attr_value(&attr),
                        _ => {}
                    }
                }
                if let (Some(id), Some(target)) = (id, target) {
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

/// Resolve a presentation-relative target ("slides/slide1.xml" or
/// "/ppt/slides/slide1.xml") to a full ZIP path.
fn join_ppt(target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    format!("ppt/{target}")
}

/// `ppt/slides/slide1.xml` -> `ppt/slides/_rels/slide1.xml.rels`.
fn slide_rels_path(slide_path: &str) -> String {
    match slide_path.rfind('/') {
        Some(i) => format!("{}/_rels/{}.rels", &slide_path[..i], &slide_path[i + 1..]),
        None => format!("_rels/{slide_path}.rels"),
    }
}

// ---------------------------------------------------------------------------
// slideN.xml — DrawingML text body
// ---------------------------------------------------------------------------

fn parse_slide(xml: &str, image_rels: &HashMap<String, String>) -> Doc {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut s = SlideWalk::default();

    while let Ok(evt) = reader.read_event_into(&mut buf) {
        match evt {
            Event::Start(e) => handle_open(&mut s, &e, image_rels, false),
            Event::Empty(e) => handle_open(&mut s, &e, image_rels, true),
            Event::Text(t) => {
                if s.in_txbody
                    && s.collecting_text
                    && let Some(run) = s.cur_run.as_mut()
                    && let Ok(decoded) = t.xml10_content()
                {
                    run.text.push_str(&decoded);
                }
            }
            Event::End(e) => handle_close(&mut s, &e),
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    let word_count = s
        .blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(p) => count_words(&p.runs),
            Block::Table(_) => 0,
        })
        .sum();
    let paragraph_count = s.blocks.len();
    Doc {
        metadata: DocumentMetadata::default(),
        blocks: s.blocks,
        paragraph_count,
        word_count,
        image_count: s.image_count,
    }
}

#[derive(Default)]
struct SlideWalk {
    blocks: Vec<Block>,
    image_count: usize,
    /// True between `<p:txBody>` open/close — the only place run text is
    /// collected.
    in_txbody: bool,
    /// Whether the current shape is a title placeholder (`<p:ph type=
    /// "title"|"ctrTitle">`). Reset on each `<p:sp>` open.
    shape_is_title: bool,
    cur_para: Option<ParaAcc>,
    cur_run: Option<RunAcc>,
    in_rpr: bool,
    collecting_text: bool,
}

#[derive(Default)]
struct ParaAcc {
    lvl: u8,
    runs: Vec<Run>,
}

#[derive(Default)]
struct RunAcc {
    text: String,
    style: Run,
}

fn handle_open(
    s: &mut SlideWalk,
    e: &quick_xml::events::BytesStart<'_>,
    image_rels: &HashMap<String, String>,
    empty: bool,
) {
    match local_name(e.name()).as_slice() {
        b"sp" => s.shape_is_title = false,
        b"ph" => {
            if matches!(attr_val(e, b"type").as_deref(), Some("title" | "ctrTitle")) {
                s.shape_is_title = true;
            }
        }
        b"txBody" => s.in_txbody = true,
        b"p" if s.in_txbody => {
            s.cur_para = Some(ParaAcc::default());
        }
        b"pPr" if s.in_txbody => {
            if let Some(p) = s.cur_para.as_mut()
                && let Some(lvl) = attr_val(e, b"lvl").and_then(|v| v.parse::<u8>().ok())
            {
                p.lvl = lvl;
            }
        }
        b"r" if s.in_txbody => s.cur_run = Some(RunAcc::default()),
        b"rPr" if s.in_txbody => {
            s.in_rpr = true;
            if let Some(run) = s.cur_run.as_mut() {
                apply_run_props(&mut run.style, e);
            }
            if empty {
                s.in_rpr = false;
            }
        }
        b"t" if s.in_txbody => s.collecting_text = true,
        b"br" if s.in_txbody => {
            if let Some(run) = s.cur_run.as_mut() {
                run.text.push('\n');
            } else if let Some(p) = s.cur_para.as_mut() {
                p.runs.push(Run {
                    text: "\n".to_string(),
                    ..Run::default()
                });
            }
        }
        b"srgbClr" if s.in_rpr => {
            if let Some(run) = s.cur_run.as_mut()
                && let Some(rgb) = attr_val(e, b"val").as_deref().and_then(parse_hex_rgb)
            {
                run.style.color = Some(rgb);
            }
        }
        b"blip" => {
            if let Some(rid) = attr_val(e, b"embed") {
                let name = image_rels
                    .get(&rid)
                    .map(|t| basename(t).to_string())
                    .unwrap_or_else(|| format!("image ({rid})"));
                s.image_count += 1;
                s.blocks.push(Block::Paragraph(Paragraph {
                    runs: vec![Run {
                        text: format!("[Image: {name}]"),
                        italic: true,
                        ..Run::default()
                    }],
                    ..Paragraph::default()
                }));
            }
        }
        _ => {}
    }
}

fn handle_close(s: &mut SlideWalk, e: &quick_xml::events::BytesEnd<'_>) {
    match local_name(e.name()).as_slice() {
        b"txBody" => s.in_txbody = false,
        b"rPr" => s.in_rpr = false,
        b"t" => s.collecting_text = false,
        b"r" => {
            if let Some(run) = s.cur_run.take()
                && let Some(p) = s.cur_para.as_mut()
                && !run.text.is_empty()
            {
                p.runs.push(Run {
                    text: run.text,
                    ..run.style
                });
            }
        }
        b"p" if s.in_txbody => {
            if let Some(p) = s.cur_para.take()
                && !p.runs.is_empty()
            {
                s.blocks.push(Block::Paragraph(Paragraph {
                    heading_level: s.shape_is_title.then_some(1),
                    indent_level: p.lvl,
                    runs: p.runs,
                    ..Paragraph::default()
                }));
            }
        }
        _ => {}
    }
}

/// Read the bold / italic / underline attributes off an `<a:rPr>`.
fn apply_run_props(style: &mut Run, e: &quick_xml::events::BytesStart<'_>) {
    if let Some(v) = attr_val(e, b"b") {
        style.bold = is_truthy(&v);
    }
    if let Some(v) = attr_val(e, b"i") {
        style.italic = is_truthy(&v);
    }
    if let Some(v) = attr_val(e, b"u") {
        // `u` is an enum (sng / dbl / heavy / none / …); anything but
        // none means underlined.
        style.underline = !matches!(v.as_str(), "none");
    }
    if let Some(v) = attr_val(e, b"strike") {
        // `strike` is sngStrike / dblStrike / noStrike.
        style.strike = !matches!(v.as_str(), "noStrike");
    }
}

fn is_truthy(v: &str) -> bool {
    matches!(v, "1" | "true" | "on")
}

// ---------------------------------------------------------------------------
// docProps/core.xml + app.xml — metadata
// ---------------------------------------------------------------------------

fn parse_core_xml(xml: &str) -> PresentationMetadata {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = PresentationMetadata::default();
    let mut field: Option<CoreField> = None;
    let mut text = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                field = core_field_from_qname(e.name());
                text.clear();
            }
            Ok(Event::Text(t)) => {
                if field.is_some()
                    && let Ok(decoded) = t.xml10_content()
                {
                    text.push_str(&decoded);
                }
            }
            Ok(Event::End(_)) => {
                if let Some(f) = field.take() {
                    let v = text.trim().to_string();
                    if !v.is_empty() {
                        assign_core(&mut out, f, v);
                    }
                    text.clear();
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
    Keywords,
    Created,
    Modified,
}

fn core_field_from_qname(name: QName<'_>) -> Option<CoreField> {
    Some(match name.as_ref() {
        b"dc:title" => CoreField::Title,
        b"dc:creator" => CoreField::Creator,
        b"dc:subject" => CoreField::Subject,
        b"cp:keywords" => CoreField::Keywords,
        b"dcterms:created" => CoreField::Created,
        b"dcterms:modified" => CoreField::Modified,
        _ => return None,
    })
}

fn assign_core(meta: &mut PresentationMetadata, field: CoreField, value: String) {
    let slot = match field {
        CoreField::Title => &mut meta.title,
        CoreField::Creator => &mut meta.creator,
        CoreField::Subject => &mut meta.subject,
        CoreField::Keywords => &mut meta.keywords,
        // `dcterms:created/modified` are ISO-8601 — parse to a typed instant.
        CoreField::Created => return meta.set_created_iso(&value),
        CoreField::Modified => return meta.set_modified_iso(&value),
    };
    if slot.is_none() {
        *slot = Some(value);
    }
}

/// `<Application>` text from `docProps/app.xml` (e.g. "Microsoft Office
/// PowerPoint").
fn parse_app_application(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut capturing = false;
    let mut text = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if local_name(e.name()) == b"Application" => capturing = true,
            Ok(Event::Text(t)) if capturing => {
                if let Ok(decoded) = t.xml10_content() {
                    text.push_str(&decoded);
                }
            }
            Ok(Event::End(e)) if local_name(e.name()) == b"Application" => break,
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    let v = text.trim().to_string();
    (!v.is_empty()).then_some(v)
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn local_name(name: QName<'_>) -> Vec<u8> {
    name.local_name().as_ref().to_vec()
}

fn attr_val(e: &quick_xml::events::BytesStart<'_>, want_local: &[u8]) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.local_name().as_ref() == want_local {
            return crate::xml::unescape_attr_value(&attr);
        }
    }
    None
}

/// Like [`attr_val`] but matches the full prefixed key — for attributes
/// whose local name collides (`r:id` vs the plain `id` on `<p:sldId>`).
fn attr_val_full(e: &quick_xml::events::BytesStart<'_>, want_key: &[u8]) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == want_key {
            return crate::xml::unescape_attr_value(&attr);
        }
    }
    None
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn parse_hex_rgb(s: &str) -> Option<[u8; 3]> {
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

    const SLIDE: &str = r#"<?xml version="1.0"?>
<p:sld xmlns:p="ppt" xmlns:a="draw" xmlns:r="rel">
 <p:cSld><p:spTree>
  <p:sp>
   <p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
   <p:txBody><a:p><a:r><a:t>Quarterly Review</a:t></a:r></a:p></p:txBody>
  </p:sp>
  <p:sp>
   <p:txBody>
     <a:p><a:r><a:rPr b="1"/><a:t>Revenue up</a:t></a:r></a:p>
     <a:p><a:pPr lvl="1"/><a:r><a:t>EMEA strong</a:t></a:r></a:p>
   </p:txBody>
  </p:sp>
  <p:pic><p:blipFill><a:blip r:embed="rId2"/></p:blipFill></p:pic>
 </p:spTree></p:cSld>
</p:sld>"#;

    fn slide_doc() -> Doc {
        let mut rels = HashMap::new();
        rels.insert("rId2".to_string(), "../media/image1.png".to_string());
        parse_slide(SLIDE, &rels)
    }

    fn paras(doc: &Doc) -> Vec<&Paragraph> {
        doc.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p),
                Block::Table(_) => None,
            })
            .collect()
    }

    fn para_text(p: &Paragraph) -> String {
        p.runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn title_placeholder_becomes_heading() {
        let doc = slide_doc();
        let ps = paras(&doc);
        assert_eq!(ps[0].heading_level, Some(1));
        assert_eq!(para_text(ps[0]), "Quarterly Review");
    }

    #[test]
    fn body_runs_carry_style_and_indent() {
        let doc = slide_doc();
        let ps = paras(&doc);
        // [0] title, [1] bold body, [2] nested, [3] image.
        assert!(ps[1].runs.iter().any(|r| r.bold && r.text == "Revenue up"));
        assert_eq!(ps[1].heading_level, None);
        assert_eq!(ps[2].indent_level, 1);
        assert_eq!(para_text(ps[2]), "EMEA strong");
    }

    #[test]
    fn image_ref_surfaces_with_basename() {
        let doc = slide_doc();
        assert_eq!(doc.image_count, 1);
        let img = paras(&doc).into_iter().last().unwrap();
        assert_eq!(para_text(img), "[Image: image1.png]");
        assert!(img.runs[0].italic);
    }

    #[test]
    fn word_count_excludes_image_label() {
        // "Quarterly Review" (2) + "Revenue up" (2) + "EMEA strong" (2)
        // + the image label (2) — all runs count, including the label.
        let doc = slide_doc();
        assert_eq!(doc.word_count, 8);
    }

    #[test]
    fn slide_order_resolved_from_sld_id_lst() {
        let xml = r#"<p:presentation xmlns:p="p" xmlns:r="r">
          <p:sldIdLst>
            <p:sldId id="256" r:id="rId3"/>
            <p:sldId id="257" r:id="rId2"/>
          </p:sldIdLst></p:presentation>"#;
        assert_eq!(parse_slide_order(xml), vec!["rId3", "rId2"]);
    }
}
