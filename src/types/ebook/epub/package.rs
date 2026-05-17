//! EPUB package metadata + spine resolution.
//!
//! An EPUB is a ZIP whose entry layout is fixed by the spec:
//!
//! - `mimetype` — uncompressed, contents `application/epub+zip`
//! - `META-INF/container.xml` — points at the OPF (`<rootfile full-path="…"/>`)
//! - the OPF — `<metadata>` (Dublin Core), `<manifest>` (id → href map),
//!   `<spine>` (ordered list of idref → reading order)
//!
//! [`Package`] resolves the spine to absolute ZIP paths so chapter
//! reads can pull bytes by entry name without rerunning the
//! container/OPF dance.

use std::io::Read;

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::reader::Reader;
use zip::ZipArchive;

use crate::input::InputSource;
use crate::types::archive::reader::{ReadSeek, open_seekable};
use crate::types::ebook::Metadata;

/// Bookkeeping for one EPUB. Built once per file open; chapter bodies
/// are still pulled lazily via [`read_entry`].
pub(crate) struct Package {
    pub metadata: Metadata,
    pub chapters: Vec<Chapter>,
}

/// One spine entry resolved through the manifest. `full_path` is the
/// absolute ZIP entry name (OPF directory + manifest href), already
/// normalized for `ZipArchive::by_name`.
#[derive(Clone)]
pub(crate) struct Chapter {
    pub full_path: String,
}

/// Parse the EPUB structure from `source`. Returns the metadata plus
/// the resolved spine. Does not load chapter bodies.
pub(crate) fn open(source: &InputSource) -> Result<Package> {
    let reader = open_seekable(source).context("failed to open EPUB container")?;
    let mut zip = ZipArchive::new(reader).context("failed to read EPUB ZIP")?;
    let opf_path = read_container_opf_path(&mut zip)?;
    let opf_bytes = read_entry(&mut zip, &opf_path)
        .with_context(|| format!("failed to read OPF at {opf_path}"))?;
    let opf_dir = parent_dir(&opf_path);
    let parsed = parse_opf(&opf_bytes)?;
    let chapters = resolve_spine(&parsed, opf_dir);
    Ok(Package {
        metadata: parsed.metadata,
        chapters,
    })
}

/// Read one entry from the EPUB ZIP into a fresh buffer.
pub(crate) fn read_entry(zip: &mut ZipArchive<Box<dyn ReadSeek>>, path: &str) -> Result<Bytes> {
    let mut file = zip
        .by_name(path)
        .with_context(|| format!("EPUB entry {path:?} not found"))?;
    let mut buf = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut buf)?;
    Ok(Bytes::from(buf))
}

/// Open a fresh ZIP handle over the source. Each chapter read takes
/// one — keeping a single archive across calls would require carrying
/// a mutable reader through the mode, which doesn't pay for itself
/// for the chapter cadence.
pub(crate) fn open_zip(source: &InputSource) -> Result<ZipArchive<Box<dyn ReadSeek>>> {
    let reader = open_seekable(source)?;
    Ok(ZipArchive::new(reader)?)
}

// ---------------------------------------------------------------------------
// container.xml — locate the OPF
// ---------------------------------------------------------------------------

fn read_container_opf_path(zip: &mut ZipArchive<Box<dyn ReadSeek>>) -> Result<String> {
    let bytes =
        read_entry(zip, "META-INF/container.xml").context("EPUB missing META-INF/container.xml")?;
    let mut reader = Reader::from_reader(bytes.as_ref());
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Empty(e) | Event::Start(e) if e.name() == QName(b"rootfile") => {
                for attr in e.attributes().flatten() {
                    if attr.key == QName(b"full-path") {
                        return Ok(attr.unescape_value()?.into_owned());
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Err(anyhow!("EPUB container.xml has no <rootfile full-path=…/>"))
}

// ---------------------------------------------------------------------------
// OPF — metadata + manifest + spine
// ---------------------------------------------------------------------------

struct ParsedOpf {
    metadata: Metadata,
    manifest: Vec<(String, String)>, // (id, href)
    spine: Vec<String>,              // idrefs in reading order
}

fn parse_opf(bytes: &[u8]) -> Result<ParsedOpf> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut metadata = Metadata::default();
    let mut manifest = Vec::new();
    let mut spine = Vec::new();

    let mut in_metadata = false;
    let mut current_dc_field: Option<DcField> = None;
    let mut current_text = String::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => {
                let local = local_name(e.name());
                match local.as_slice() {
                    b"metadata" => in_metadata = true,
                    _ if in_metadata => {
                        current_dc_field = dc_field_from_local(&local);
                        current_text.clear();
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                let local = local_name(e.name());
                match local.as_slice() {
                    b"metadata" => in_metadata = false,
                    _ if in_metadata => {
                        if let Some(f) = current_dc_field.take() {
                            assign_dc(&mut metadata, f, current_text.trim().to_string());
                        }
                        current_text.clear();
                    }
                    _ => {}
                }
            }
            Event::Text(t)
                if in_metadata
                    && current_dc_field.is_some()
                    && let Ok(decoded) = t.xml_content() =>
            {
                current_text.push_str(&decoded);
            }
            Event::Empty(e) => {
                let local = local_name(e.name());
                match local.as_slice() {
                    b"item" => {
                        let mut id = None;
                        let mut href = None;
                        for attr in e.attributes().flatten() {
                            match attr.key.local_name().as_ref() {
                                b"id" => id = attr.unescape_value().ok().map(|c| c.into_owned()),
                                b"href" => {
                                    href = attr.unescape_value().ok().map(|c| c.into_owned())
                                }
                                _ => {}
                            }
                        }
                        if let (Some(id), Some(href)) = (id, href) {
                            manifest.push((id, href));
                        }
                    }
                    b"itemref" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"idref"
                                && let Ok(v) = attr.unescape_value()
                            {
                                spine.push(v.into_owned());
                            }
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

    Ok(ParsedOpf {
        metadata,
        manifest,
        spine,
    })
}

#[derive(Clone, Copy)]
enum DcField {
    Title,
    Creator,
    Language,
    Publisher,
    Date,
    Identifier,
    Description,
}

fn dc_field_from_local(local: &[u8]) -> Option<DcField> {
    Some(match local {
        b"title" => DcField::Title,
        b"creator" => DcField::Creator,
        b"language" => DcField::Language,
        b"publisher" => DcField::Publisher,
        b"date" => DcField::Date,
        b"identifier" => DcField::Identifier,
        b"description" => DcField::Description,
        _ => return None,
    })
}

fn assign_dc(meta: &mut Metadata, field: DcField, value: String) {
    if value.is_empty() {
        return;
    }
    let slot = match field {
        DcField::Title => &mut meta.title,
        DcField::Creator => &mut meta.creator,
        DcField::Language => &mut meta.language,
        DcField::Publisher => &mut meta.publisher,
        DcField::Date => &mut meta.date,
        DcField::Identifier => &mut meta.identifier,
        DcField::Description => &mut meta.description,
    };
    if slot.is_none() {
        *slot = Some(value);
    }
}

fn local_name(name: QName<'_>) -> Vec<u8> {
    name.local_name().as_ref().to_vec()
}

fn resolve_spine(parsed: &ParsedOpf, opf_dir: &str) -> Vec<Chapter> {
    let mut out = Vec::with_capacity(parsed.spine.len());
    for idref in &parsed.spine {
        let Some((_, href)) = parsed.manifest.iter().find(|(id, _)| id == idref) else {
            // Spine references a missing manifest id — skip rather
            // than fail the whole open. Real-world EPUBs occasionally
            // ship dangling idrefs.
            continue;
        };
        let full_path = if opf_dir.is_empty() {
            href.clone()
        } else {
            format!("{opf_dir}/{href}")
        };
        out.push(Chapter { full_path });
    }
    out
}

fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}
