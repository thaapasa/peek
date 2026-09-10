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

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use peek_io::InputSource;
use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::reader::Reader;
use zip::ZipArchive;

use crate::types::archive::reader::{self, ReadSeek};
use crate::types::ebook::Metadata;
use crate::xml::local_name;

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
    let mut zip = open_zip(source)?;
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

/// Read one entry from the EPUB ZIP into a fresh buffer. Cap-gated
/// against zip bombs via the shared archive helper.
pub(crate) fn read_entry(zip: &mut ZipArchive<Box<dyn ReadSeek>>, path: &str) -> Result<Bytes> {
    reader::read_zip_entry(zip, path, "EPUB")
}

/// Open a fresh ZIP handle over the source. Each chapter read takes
/// one — keeping a single archive across calls would require carrying
/// a mutable reader through the mode, which doesn't pay for itself
/// for the chapter cadence.
pub(crate) fn open_zip(source: &InputSource) -> Result<ZipArchive<Box<dyn ReadSeek>>> {
    reader::open_zip(source, "EPUB")
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
            Event::Empty(e) | Event::Start(e) if e.name() == QName("rootfile") => {
                for attr in e.attributes().flatten() {
                    if attr.key == QName("full-path")
                        && let Some(v) = crate::xml::unescape_attr_value(&attr)
                    {
                        return Ok(v);
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
                match local {
                    "metadata" => in_metadata = true,
                    _ if in_metadata => {
                        current_dc_field = dc_field_from_local(local);
                        current_text.clear();
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                let local = local_name(e.name());
                match local {
                    "metadata" => in_metadata = false,
                    _ if in_metadata => {
                        if let Some(f) = current_dc_field.take() {
                            assign_dc(&mut metadata, f, current_text.trim().to_string());
                        }
                        current_text.clear();
                    }
                    _ => {}
                }
            }
            Event::Text(t) if in_metadata && current_dc_field.is_some() => {
                current_text.push_str(&t.xml10_content());
            }
            Event::Empty(e) => {
                let local = local_name(e.name());
                match local {
                    "item" => {
                        let mut id = None;
                        let mut href = None;
                        for attr in e.attributes().flatten() {
                            match attr.key.local_name().as_ref() {
                                "id" => id = crate::xml::unescape_attr_value(&attr),
                                "href" => href = crate::xml::unescape_attr_value(&attr),
                                _ => {}
                            }
                        }
                        if let (Some(id), Some(href)) = (id, href) {
                            manifest.push((id, href));
                        }
                    }
                    "itemref" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == "idref"
                                && let Some(v) = crate::xml::unescape_attr_value(&attr)
                            {
                                spine.push(v);
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

fn dc_field_from_local(local: &str) -> Option<DcField> {
    Some(match local {
        "title" => DcField::Title,
        "creator" => DcField::Creator,
        "language" => DcField::Language,
        "publisher" => DcField::Publisher,
        "date" => DcField::Date,
        "identifier" => DcField::Identifier,
        "description" => DcField::Description,
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
