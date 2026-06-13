//! ODP (OpenDocument Presentation) package open + per-slide AST.
//!
//! Walks `content.xml`, segmenting on `<draw:page>` — one page is one
//! slide. Inside a page, `<text:p>` / `<text:h>` paragraphs (and their
//! `<text:span>` children) become prose; a frame carrying
//! `presentation:class="title"` marks its paragraphs as the slide title
//! (level-1 heading). Speaker notes (`<presentation:notes>`) are
//! skipped. Run-level styling is left plain in this first cut — ODF
//! styling is indirect (style-name → automatic-styles) and the slide
//! read view reads fine without it. Metadata comes from `meta.xml`.

use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::reader::Reader;

use crate::input::InputSource;
use crate::types::archive::reader::{open_zip, read_zip_entry_str};
use crate::types::document::DocumentMetadata;
use crate::types::document::ast::{Block, Doc, Paragraph, Run, count_words};
use crate::types::presentation::{Deck, PresentationMetadata};

pub fn open(source: &InputSource) -> Result<Deck> {
    let mut zip = open_zip(source, "ODP")?;
    let content_xml =
        read_zip_entry_str(&mut zip, "content.xml", "ODP").context("couldn't read content.xml")?;
    let meta_xml = read_zip_entry_str(&mut zip, "meta.xml", "ODP").ok();
    let metadata = meta_xml.as_deref().map(parse_meta).unwrap_or_default();
    let slides = parse_content(&content_xml);
    Ok(Deck { metadata, slides })
}

// ---------------------------------------------------------------------------
// content.xml — page-segmented body walk
// ---------------------------------------------------------------------------

fn parse_content(xml: &str) -> Vec<Doc> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut slides: Vec<Doc> = Vec::new();
    let mut page: Option<PageAcc> = None;
    let mut notes_depth = 0usize;
    let mut frame_is_title = false;
    let mut para: Option<ParaAcc> = None;

    while let Ok(evt) = reader.read_event_into(&mut buf) {
        match evt {
            // Empty elements (`<text:line-break/>`, `<draw:image/>`)
            // arrive as `Event::Empty`; fold both into one arm.
            Event::Start(e) | Event::Empty(e) => match local_name(e.name()).as_slice() {
                b"page" => page = Some(PageAcc::default()),
                b"notes" => notes_depth += 1,
                b"frame" if notes_depth == 0 => {
                    frame_is_title = matches!(
                        attr_val(&e, b"class").as_deref(),
                        Some("title" | "subtitle")
                    );
                }
                b"p" | b"h" if page.is_some() && notes_depth == 0 => {
                    para = Some(ParaAcc {
                        heading: frame_is_title || local_name(e.name()).as_slice() == b"h",
                        text: String::new(),
                    });
                }
                b"line-break" | b"tab" => {
                    if let Some(p) = para.as_mut() {
                        p.text.push(if local_name(e.name()).as_slice() == b"tab" {
                            '\t'
                        } else {
                            '\n'
                        });
                    }
                }
                b"image" if page.is_some() && notes_depth == 0 => {
                    if let (Some(pg), Some(href)) = (page.as_mut(), attr_val(&e, b"href")) {
                        pg.image_count += 1;
                        pg.blocks.push(Block::Paragraph(Paragraph {
                            runs: vec![Run {
                                text: format!("[Image: {}]", basename(&href)),
                                italic: true,
                                ..Run::default()
                            }],
                            ..Paragraph::default()
                        }));
                    }
                }
                _ => {}
            },
            Event::Text(t) => {
                if let Some(p) = para.as_mut()
                    && let Ok(decoded) = t.xml_content()
                {
                    p.text.push_str(&decoded);
                }
            }
            Event::End(e) => match local_name(e.name()).as_slice() {
                b"notes" => notes_depth = notes_depth.saturating_sub(1),
                b"frame" if notes_depth == 0 => frame_is_title = false,
                b"p" | b"h" => {
                    if let (Some(pg), Some(p)) = (page.as_mut(), para.take())
                        && !p.text.trim().is_empty()
                    {
                        pg.blocks.push(Block::Paragraph(Paragraph {
                            heading_level: p.heading.then_some(1),
                            runs: vec![Run {
                                text: p.text,
                                ..Run::default()
                            }],
                            ..Paragraph::default()
                        }));
                    }
                }
                b"page" => {
                    if let Some(pg) = page.take() {
                        slides.push(pg.into_doc());
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    slides
}

#[derive(Default)]
struct PageAcc {
    blocks: Vec<Block>,
    image_count: usize,
}

impl PageAcc {
    fn into_doc(self) -> Doc {
        let word_count = self
            .blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph(p) => count_words(&p.runs),
                Block::Table(_) => 0,
            })
            .sum();
        let paragraph_count = self.blocks.len();
        Doc {
            metadata: DocumentMetadata::default(),
            blocks: self.blocks,
            paragraph_count,
            word_count,
            image_count: self.image_count,
        }
    }
}

struct ParaAcc {
    heading: bool,
    text: String,
}

// ---------------------------------------------------------------------------
// meta.xml — Dublin Core metadata
// ---------------------------------------------------------------------------

fn parse_meta(xml: &str) -> PresentationMetadata {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut out = PresentationMetadata::default();
    let mut field: Option<MetaField> = None;
    let mut text = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                field = meta_field_from_qname(e.name());
                text.clear();
            }
            Ok(Event::Text(t)) => {
                if field.is_some()
                    && let Ok(decoded) = t.xml_content()
                {
                    text.push_str(&decoded);
                }
            }
            Ok(Event::End(_)) => {
                if let Some(f) = field.take() {
                    let v = text.trim().to_string();
                    if !v.is_empty() {
                        assign_meta(&mut out, f, v);
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
enum MetaField {
    Title,
    Creator,
    Subject,
    Keyword,
    Created,
    Modified,
    Generator,
}

fn meta_field_from_qname(name: QName<'_>) -> Option<MetaField> {
    Some(match name.as_ref() {
        b"dc:title" => MetaField::Title,
        b"dc:creator" => MetaField::Creator,
        b"dc:subject" => MetaField::Subject,
        b"meta:keyword" => MetaField::Keyword,
        b"meta:creation-date" => MetaField::Created,
        b"dc:date" => MetaField::Modified,
        b"meta:generator" => MetaField::Generator,
        _ => return None,
    })
}

fn assign_meta(meta: &mut PresentationMetadata, field: MetaField, value: String) {
    let slot = match field {
        MetaField::Title => &mut meta.title,
        MetaField::Creator => &mut meta.creator,
        MetaField::Subject => &mut meta.subject,
        MetaField::Keyword => &mut meta.keywords,
        MetaField::Created => &mut meta.created,
        MetaField::Modified => &mut meta.modified,
        MetaField::Generator => &mut meta.application,
    };
    if slot.is_none() {
        *slot = Some(value);
    }
}

// ---------------------------------------------------------------------------
// Helpers
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

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTENT: &str = r#"<?xml version="1.0"?>
<office:document-content xmlns:office="o" xmlns:draw="d" xmlns:text="t"
    xmlns:presentation="p" xmlns:xlink="x">
 <office:body><office:presentation>
  <draw:page>
   <draw:frame presentation:class="title"><draw:text-box>
     <text:p>Roadmap</text:p></draw:text-box></draw:frame>
   <draw:frame><draw:text-box>
     <text:p>Ship v1</text:p><text:p>Then v2</text:p></draw:text-box></draw:frame>
   <draw:frame><draw:image xlink:href="Pictures/diagram.png"/></draw:frame>
  </draw:page>
  <draw:page>
   <draw:frame><draw:text-box><text:p>Thanks</text:p></draw:text-box></draw:frame>
   <presentation:notes><draw:frame><draw:text-box>
     <text:p>speaker note, hidden</text:p></draw:text-box></draw:frame></presentation:notes>
  </draw:page>
 </office:presentation></office:body>
</office:document-content>"#;

    fn para_text(p: &Paragraph) -> String {
        p.runs.iter().map(|r| r.text.as_str()).collect()
    }

    fn texts(doc: &Doc) -> Vec<String> {
        doc.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(para_text(p)),
                Block::Table(_) => None,
            })
            .collect()
    }

    #[test]
    fn pages_segment_into_slides() {
        let slides = parse_content(CONTENT);
        assert_eq!(slides.len(), 2);
    }

    #[test]
    fn title_frame_marks_heading_and_body_follows() {
        let slides = parse_content(CONTENT);
        let s0 = &slides[0];
        let headings: Vec<_> = s0
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => p.heading_level,
                _ => None,
            })
            .collect();
        assert_eq!(headings, vec![1]);
        assert_eq!(
            texts(s0),
            vec!["Roadmap", "Ship v1", "Then v2", "[Image: diagram.png]"]
        );
        assert_eq!(s0.image_count, 1);
    }

    #[test]
    fn speaker_notes_are_excluded() {
        let slides = parse_content(CONTENT);
        assert_eq!(texts(&slides[1]), vec!["Thanks"]);
    }

    #[test]
    fn meta_maps_generator_to_application() {
        let xml = r#"<office:document-meta xmlns:office="o" xmlns:dc="dc" xmlns:meta="m">
          <office:meta>
            <dc:title>Deck</dc:title>
            <meta:generator>LibreOffice/24.2</meta:generator>
          </office:meta></office:document-meta>"#;
        let m = parse_meta(xml);
        assert_eq!(m.title.as_deref(), Some("Deck"));
        assert_eq!(m.application.as_deref(), Some("LibreOffice/24.2"));
    }
}
