//! Archive info-view extras: gather TOC stats, render the Archive
//! section. On listing failure the format name is preserved and the
//! error is surfaced as a warning row.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use serde_json::json;

use super::reader::list_entries;
use crate::info::{Extras, InfoNode, Role, Value, Warn, thousands_sep};
use crate::viewer::listing::Stats;
use peek_detect::ArchiveFormat;
use peek_io::InputSource;
use peek_theme::PeekTheme;

pub struct ArchiveStats {
    pub format_name: &'static str,
    pub entry_count: usize,
    pub file_count: usize,
    pub dir_count: usize,
    pub total_uncompressed_size: u64,
    /// Set when listing failed (e.g. corrupt archive). When present,
    /// the info view shows this in place of stats.
    pub error: Option<String>,
    /// Present when an `ar` archive's members are object files — i.e. a
    /// static library — summarising the object payload.
    pub static_lib: Option<StaticLibSummary>,
    /// True when the `ar` archive exceeded [`STATIC_LIB_SUMMARY_CAP`] and
    /// the object-member probe was skipped — the summary may exist but
    /// wasn't read. Surfaced as a note row so the absence isn't silent.
    pub static_lib_skipped: bool,
    /// True when the TOC hit the entry cap and the counts below cover only
    /// the first [`MAX_ENTRIES`](super::backends::MAX_ENTRIES) — surfaced as
    /// a note so the stats don't read as complete.
    pub truncated: bool,
}

/// Summary of a static library (`.a` / `.lib`): how many members are
/// object files and the architecture they target.
pub struct StaticLibSummary {
    pub object_members: usize,
    /// Architecture label of the first object member, if parseable.
    pub architecture: Option<String>,
}

pub fn gather_extras(source: &InputSource, format: ArchiveFormat) -> Extras {
    match list_entries(source, format) {
        Ok((entries, truncated)) => {
            let stats = Stats::from_root(format.label(), &entries);
            let (static_lib, static_lib_skipped) = static_lib_summary(source, format);
            Box::new(ArchiveStats {
                format_name: stats.format_name,
                entry_count: stats.entry_count,
                file_count: stats.file_count,
                dir_count: stats.dir_count,
                total_uncompressed_size: stats.total_size,
                error: None,
                static_lib,
                static_lib_skipped,
                truncated,
            })
        }
        Err(e) => Box::new(ArchiveStats {
            format_name: format.label(),
            entry_count: 0,
            file_count: 0,
            dir_count: 0,
            total_uncompressed_size: 0,
            error: Some(format!("{e:#}")),
            static_lib: None,
            static_lib_skipped: false,
            truncated: false,
        }),
    }
}

/// Whole-archive read cap for the object-member summary. The buffer is
/// materialized whole (the `ar` parser wants random-access member
/// slices) but held only for this one summary pass with no expansion —
/// and real static libraries (libLLVM.a, ML framework bundles) routinely
/// run hundreds of MB, where the sidecar budget would drop the section
/// for legitimate inputs. So this aliases the bulk-walk budget; over the
/// cap the info view shows a "summary skipped" note instead.
const STATIC_LIB_SUMMARY_CAP: u64 = peek_io::limits::BULK_WALK_BYTES;

/// Probe an `ar` archive's object members. `(None, false)` for non-`ar`
/// formats and for `ar` archives with no object members (e.g. a `.deb`,
/// whose members are tarballs); `(None, true)` when the archive exceeds
/// [`STATIC_LIB_SUMMARY_CAP`] and the probe was skipped.
fn static_lib_summary(
    source: &InputSource,
    format: ArchiveFormat,
) -> (Option<StaticLibSummary>, bool) {
    if format != ArchiveFormat::Ar {
        return (None, false);
    }
    match source.byte_len() {
        Ok(len) if len > STATIC_LIB_SUMMARY_CAP => return (None, true),
        Ok(_) => {}
        Err(_) => return (None, false),
    }
    (parse_static_lib(source), false)
}

/// The whole-file probe behind [`static_lib_summary`]: read the archive
/// once for random-access member slices — the same whole-file cost the
/// object-file viewer pays. Only the first object member is fully parsed
/// (for its architecture); the per-member object check is a cheap
/// `FileKind` magic read. Callers gate the read at
/// [`STATIC_LIB_SUMMARY_CAP`].
fn parse_static_lib(source: &InputSource) -> Option<StaticLibSummary> {
    let bytes = source
        .read_bytes(peek_io::limits::Budget::Unbounded(
            "gated by STATIC_LIB_SUMMARY_CAP in caller",
        ))
        .ok()?;
    let archive = object::read::archive::ArchiveFile::parse(&*bytes).ok()?;
    let mut object_members = 0usize;
    let mut architecture = None;
    for member in archive.members() {
        let Ok(member) = member else { continue };
        let Ok(data) = member.data(&*bytes) else {
            continue;
        };
        if !is_object_member(data) {
            continue;
        }
        object_members += 1;
        if architecture.is_none()
            && let Ok(file) = object::File::parse(data)
        {
            use object::Object;
            architecture = Some(crate::types::objfile::info_render::arch_label(
                file.architecture(),
            ));
        }
    }
    (object_members > 0).then_some(StaticLibSummary {
        object_members,
        architecture,
    })
}

/// True when a `ar` member's bytes open on a recognised object-file
/// container — the cheap magic check (`FileKind` reads only the head).
fn is_object_member(data: &[u8]) -> bool {
    use object::FileKind::*;
    matches!(
        object::FileKind::parse(data),
        Ok(Elf32
            | Elf64
            | MachO32
            | MachO64
            | MachOFat32
            | MachOFat64
            | Coff
            | CoffBig
            | Pe32
            | Pe64
            | Wasm
            | Xcoff32
            | Xcoff64)
    )
}

crate::info_section!(ArchiveStats, ArchiveView, "archive");

#[derive(Serialize, crate::info::InfoView)]
struct ArchiveView {
    #[info(nest)]
    #[serde(flatten)]
    main: ArchiveMain,
    #[info(nest)]
    #[serde(rename = "static_lib", skip_serializing_if = "Option::is_none")]
    static_lib: Option<StaticLib>,
}

#[derive(Serialize, crate::info::InfoView)]
#[info(title = "Archive")]
struct ArchiveMain {
    #[info(label = "Format")]
    format: &'static str,
    #[info(label = "Status", skip_if = "Option::is_none")]
    #[serde(rename = "error", skip_serializing_if = "Option::is_none")]
    error: Option<Warn>,
    // On a listing error these drop from both outputs.
    #[info(label = "Entries")]
    #[serde(rename = "entry_count", skip_serializing_if = "Option::is_none")]
    entry_count: Option<Value>,
    #[info(label = "Files")]
    #[serde(rename = "file_count", skip_serializing_if = "Option::is_none")]
    file_count: Option<Value>,
    #[info(label = "Directories")]
    #[serde(rename = "dir_count", skip_serializing_if = "Option::is_none")]
    dir_count: Option<Value>,
    #[info(label = "Total size")]
    #[serde(
        rename = "total_uncompressed_size",
        skip_serializing_if = "Option::is_none"
    )]
    total_size: Option<Value>,
    // Present only when the static-library probe was skipped over the
    // read cap — absence of the summary block would otherwise be silent.
    #[info(label = "Static library", skip_if = "Option::is_none")]
    #[serde(rename = "static_lib_skipped", skip_serializing_if = "Option::is_none")]
    static_lib_note: Option<Value>,
    // Present only when the TOC was capped — the counts above are partial.
    #[info(label = "Listing", skip_if = "Option::is_none")]
    #[serde(rename = "entries_truncated", skip_serializing_if = "Option::is_none")]
    truncated_note: Option<Value>,
}

impl From<&ArchiveStats> for ArchiveView {
    fn from(s: &ArchiveStats) -> Self {
        let ok = s.error.is_none();
        ArchiveView {
            main: ArchiveMain {
                format: s.format_name,
                error: s.error.clone().map(Warn),
                entry_count: ok.then(|| Value::count(s.entry_count as u64)),
                file_count: ok.then(|| Value::count(s.file_count as u64)),
                dir_count: ok.then(|| Value::count(s.dir_count as u64)),
                total_size: ok.then(|| {
                    Value::split(
                        format!("{} bytes", thousands_sep(s.total_uncompressed_size)),
                        Role::Value,
                        json!(s.total_uncompressed_size),
                    )
                }),
                static_lib_note: s.static_lib_skipped.then(|| {
                    Value::split(
                        format!(
                            "summary skipped (archive > {} MB)",
                            STATIC_LIB_SUMMARY_CAP / (1024 * 1024)
                        ),
                        Role::Muted,
                        json!(true),
                    )
                }),
                truncated_note: s.truncated.then(|| {
                    Value::split(
                        format!(
                            "truncated to first {} entries",
                            thousands_sep(super::backends::MAX_ENTRIES as u64)
                        ),
                        Role::Warn,
                        json!(true),
                    )
                }),
            },
            static_lib: s.static_lib.as_ref().map(|lib| StaticLib {
                object_members: lib.object_members,
                architecture: lib.architecture.clone(),
            }),
        }
    }
}

/// Static-library summary. Print: a `Static library` block. JSON: a
/// `static_lib` object.
struct StaticLib {
    object_members: usize,
    architecture: Option<String>,
}
impl crate::info::InfoView for StaticLib {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        let mut body = vec![InfoNode::Row {
            label: "Objects".into(),
            value: crate::info::paint_count(self.object_members, theme),
        }];
        if let Some(arch) = &self.architecture {
            body.push(InfoNode::Row {
                label: "Architecture".into(),
                value: theme.paint_value(arch),
            });
        }
        vec![InfoNode::Block {
            title: "Static library".to_string(),
            body,
        }]
    }
}
impl Serialize for StaticLib {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let len = 1 + self.architecture.is_some() as usize;
        let mut st = ser.serialize_struct("static_lib", len)?;
        st.serialize_field("object_members", &self.object_members)?;
        if let Some(arch) = &self.architecture {
            st.serialize_field("architecture", arch)?;
        }
        st.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> InputSource {
        let mut p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        p.push("test-data");
        p.push(name);
        InputSource::File(p)
    }

    /// A static library's object members are counted and the target
    /// architecture is read from the first one.
    #[test]
    fn static_lib_summary_counts_objects() {
        let (lib, skipped) = static_lib_summary(&fixture("tiny.a"), ArchiveFormat::Ar);
        let lib = lib.expect("tiny.a is a static library");
        assert!(!skipped);
        assert_eq!(lib.object_members, 3);
        assert_eq!(lib.architecture.as_deref(), Some("AArch64"));
    }

    /// An `ar` archive whose members are not objects (a `.deb` carries
    /// tarballs) gets no static-library summary.
    #[test]
    fn non_object_ar_has_no_summary() {
        assert!(
            static_lib_summary(&fixture("hello.deb"), ArchiveFormat::Ar)
                .0
                .is_none()
        );
    }

    /// Non-`ar` formats are never treated as static libraries.
    #[test]
    fn non_ar_format_skipped() {
        assert!(
            static_lib_summary(&fixture("archive.zip"), ArchiveFormat::Zip)
                .0
                .is_none()
        );
    }

    /// An over-cap probe skip is surfaced, not silent: a note row in
    /// print and `static_lib_skipped: true` in JSON.
    #[test]
    fn skipped_probe_emits_note() {
        let stats = ArchiveStats {
            format_name: "ar",
            entry_count: 1,
            file_count: 1,
            dir_count: 0,
            total_uncompressed_size: 1,
            error: None,
            static_lib: None,
            static_lib_skipped: true,
            truncated: false,
        };
        let (_, json) = json_section(&stats);
        assert_eq!(json["static_lib_skipped"], serde_json::json!(true));
        let theme = PeekTheme::from_syntect(&peek_theme::load_embedded_theme(
            peek_theme::PeekThemeName::IdeaDark.tmtheme_source(),
        ));
        let mut lines = Vec::new();
        render_section(&mut lines, &stats, &theme);
        assert!(
            lines.iter().any(|l| l.contains("summary skipped")),
            "got: {lines:?}"
        );
    }
}
