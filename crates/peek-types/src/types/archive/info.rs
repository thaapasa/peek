//! Archive info-view extras: gather TOC stats, render the Archive
//! section. On listing failure the format name is preserved and the
//! error is surfaced as a warning row.

use super::reader::list_entries;
use crate::info::{Extras, paint_count, push_field, push_section_header, thousands_sep};
use crate::input::InputSource;
use crate::input::detect::ArchiveFormat;
use crate::theme::PeekTheme;
use crate::viewer::listing::Stats;

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
        Ok(entries) => {
            let stats = Stats::from_root(format.label(), &entries);
            Box::new(ArchiveStats {
                format_name: stats.format_name,
                entry_count: stats.entry_count,
                file_count: stats.file_count,
                dir_count: stats.dir_count,
                total_uncompressed_size: stats.total_size,
                error: None,
                static_lib: static_lib_summary(source, format),
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
        }),
    }
}

/// Summarise an `ar` archive's object members. Returns `None` for
/// non-`ar` formats and for `ar` archives with no object members (e.g. a
/// `.deb`, whose members are tarballs).
///
/// This reads the whole archive once for random-access member slices —
/// the same whole-file cost the object-file viewer pays, and acceptable
/// for the same reason (static libraries are not multi-GB streams). Only
/// the first object member is fully parsed (for its architecture); the
/// per-member object check is a cheap `FileKind` magic read.
fn static_lib_summary(source: &InputSource, format: ArchiveFormat) -> Option<StaticLibSummary> {
    if format != ArchiveFormat::Ar {
        return None;
    }
    let bytes = source.read_bytes().ok()?;
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

pub fn render_section(lines: &mut Vec<String>, stats: &ArchiveStats, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Archive", theme);
    push_field(
        lines,
        "Format",
        &theme.paint_value(stats.format_name),
        theme,
    );

    if let Some(err) = &stats.error {
        push_field(lines, "Status", &theme.paint_warning(err), theme);
        return;
    }

    push_field(
        lines,
        "Entries",
        &paint_count(stats.entry_count, theme),
        theme,
    );
    push_field(lines, "Files", &paint_count(stats.file_count, theme), theme);
    push_field(
        lines,
        "Directories",
        &paint_count(stats.dir_count, theme),
        theme,
    );
    push_field(
        lines,
        "Total size",
        &theme.paint_value(&format!(
            "{} bytes",
            thousands_sep(stats.total_uncompressed_size)
        )),
        theme,
    );

    if let Some(lib) = &stats.static_lib {
        lines.push(String::new());
        push_section_header(lines, "Static library", theme);
        push_field(
            lines,
            "Objects",
            &paint_count(lib.object_members, theme),
            theme,
        );
        if let Some(arch) = &lib.architecture {
            push_field(lines, "Architecture", &theme.paint_value(arch), theme);
        }
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
        let lib = static_lib_summary(&fixture("tiny.a"), ArchiveFormat::Ar)
            .expect("tiny.a is a static library");
        assert_eq!(lib.object_members, 3);
        assert_eq!(lib.architecture.as_deref(), Some("AArch64"));
    }

    /// An `ar` archive whose members are not objects (a `.deb` carries
    /// tarballs) gets no static-library summary.
    #[test]
    fn non_object_ar_has_no_summary() {
        assert!(static_lib_summary(&fixture("hello.deb"), ArchiveFormat::Ar).is_none());
    }

    /// Non-`ar` formats are never treated as static libraries.
    #[test]
    fn non_ar_format_skipped() {
        assert!(static_lib_summary(&fixture("archive.zip"), ArchiveFormat::Zip).is_none());
    }
}
