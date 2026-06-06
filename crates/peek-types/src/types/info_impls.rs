//! [`InfoExtras`](crate::info::InfoExtras) impls for every per-type stats
//! struct — the registry that replaced the old `FileExtras` enum.
//!
//! Each row wires one stats struct to its module's free
//! `render_section(lines, &self, theme)` via [`impl_info_extras!`], and —
//! in the three-argument form — its `json_section(&self) -> (key, Value)`
//! encoder for `--info --json`. The impls live here (not in `info/`)
//! because the structs are owned by the `types/` layer: the trait is
//! upstream, the type is local, so the impl is orphan-legal in this crate
//! — and stays legal once `types/` is split into its own crate below
//! `info`. Both arguments name the real `crate::types::…` paths (no detour
//! through `info` re-exports), so this file is the only `types → info`
//! edge: the `InfoExtras` trait itself. Adding a file type means adding one
//! row here.

use crate::impl_info_extras;

impl_info_extras!(
    crate::types::image::info::ImageStats,
    crate::types::image::info_render::render_section,
    crate::types::image::info_render::json_section
);
impl_info_extras!(
    crate::types::text::info::TextStats,
    crate::types::text::info_render::render_section,
    crate::types::text::info_render::json_section
);
impl_info_extras!(
    crate::types::svg::info::SvgStats,
    crate::types::svg::info_render::render_section,
    crate::types::svg::info_render::json_section
);
impl_info_extras!(
    crate::types::structured::info::StructuredInfo,
    crate::types::structured::info::render_section,
    crate::types::structured::info::json_section
);
impl_info_extras!(
    crate::types::markdown::info::MarkdownInfo,
    crate::types::markdown::info_render::render_section,
    crate::types::markdown::info_render::json_section
);
impl_info_extras!(
    crate::types::notebook::NotebookInfo,
    crate::types::notebook::info_render::render_section,
    crate::types::notebook::info_render::json_section
);
impl_info_extras!(
    crate::types::email::EmailInfo,
    crate::types::email::info_render::render_section,
    crate::types::email::info_render::json_section
);
impl_info_extras!(
    crate::types::sql::info::SqlInfo,
    crate::types::sql::info_render::render_section,
    crate::types::sql::info_render::json_section
);
impl_info_extras!(
    crate::types::css::info::CssInfo,
    crate::types::css::info_render::render_section,
    crate::types::css::info_render::json_section
);
impl_info_extras!(
    crate::types::binary::info::BinaryInfo,
    crate::types::binary::info::render_section,
    crate::types::binary::info::json_section
);
impl_info_extras!(
    crate::types::objfile::info::ObjectInfo,
    crate::types::objfile::info_render::render_section,
    crate::types::objfile::info_render::json_section
);
impl_info_extras!(
    crate::types::classfile::info::ClassfileInfo,
    crate::types::classfile::info_render::render_section,
    crate::types::classfile::info_render::json_section
);
impl_info_extras!(
    crate::types::archive::info::ArchiveStats,
    crate::types::archive::info::render_section,
    crate::types::archive::info::json_section
);
impl_info_extras!(
    crate::types::disk_image::info::DiskImageInfo,
    crate::types::disk_image::info_render::render_section,
    crate::types::disk_image::info_render::json_section
);
impl_info_extras!(
    crate::types::directory::info::DirectoryStats,
    crate::types::directory::info::render_section,
    crate::types::directory::info::json_section
);
impl_info_extras!(
    crate::types::ebook::EbookStats,
    crate::types::ebook::epub::info_render::render_section,
    crate::types::ebook::epub::info_render::json_section
);
impl_info_extras!(
    crate::types::comic::ComicStats,
    crate::types::comic::cbz::info_render::render_section,
    crate::types::comic::cbz::info_render::json_section
);
impl_info_extras!(
    crate::types::document::DocumentStats,
    crate::types::document::info_render::render_section,
    crate::types::document::info_render::json_section
);
impl_info_extras!(
    crate::types::pdf::PdfStats,
    crate::types::pdf::info_render::render_section,
    crate::types::pdf::info_render::json_section
);
impl_info_extras!(
    crate::types::eps::EpsInfo,
    crate::types::eps::info_render::render_section,
    crate::types::eps::info_render::json_section
);
impl_info_extras!(
    crate::types::spreadsheet::SpreadsheetInfo,
    crate::types::spreadsheet::info_render::render_section,
    crate::types::spreadsheet::info_render::json_section
);
impl_info_extras!(
    crate::types::audio::AudioStats,
    crate::types::audio::info_render::render_section,
    crate::types::audio::info_render::json_section
);
impl_info_extras!(
    crate::types::csv::CsvStats,
    crate::types::csv::info_render::render_section,
    crate::types::csv::info_render::json_section
);
impl_info_extras!(
    crate::types::sqlite::info::SqliteInfo,
    crate::types::sqlite::info_render::render_section,
    crate::types::sqlite::info_render::json_section
);
impl_info_extras!(
    crate::types::cert::info::CertInfo,
    crate::types::cert::info_render::render_section,
    crate::types::cert::info_render::json_section
);
impl_info_extras!(
    crate::types::font::info::FontInfo,
    crate::types::font::info_render::render_section,
    crate::types::font::info_render::json_section
);
impl_info_extras!(
    crate::types::vobject::VObjectInfo,
    crate::types::vobject::info::render_section,
    crate::types::vobject::info::json_section
);
