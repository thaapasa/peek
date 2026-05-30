//! Fixture-based tests covering the full detect + gather pipeline against
//! the real test files in `test-images/` and `test-data/`. These complement
//! the synthetic tests in `text` (which exercise streaming-pass edge cases)
//! by anchoring the format-specific extras to known on-disk content.

use std::path::PathBuf;

use super::super::FileExtras;
use super::gather;
use crate::input::InputSource;
use crate::input::detect;
use crate::input::detect::{FileType, PdfFlavor, PostScriptFormat, SpreadsheetFormat};
use crate::types::eps::dos_eps::PreviewKind;
use crate::types::eps::gs;
use crate::types::image::info::{AnimationStats, LoopCount};
use crate::types::structured::info::TopLevelKind;
use crate::types::text::info::{Encoding, IndentStyle, LineEndings};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn gather_fixture(rel: &str) -> super::super::FileInfo {
    let path = fixture(rel);
    assert!(path.exists(), "fixture missing: {}", path.display());
    let source = InputSource::File(path);
    let detected = detect::detect(&source).expect("detect");
    gather(&source, &detected).expect("gather")
}

// ---------------------------------------------------------------------------
// Image fixtures
// ---------------------------------------------------------------------------

#[test]
fn jpeg_cozy_room_has_dimensions_icc_and_exif() {
    let info = gather_fixture("test-images/cozy-room.jpg");
    let FileExtras::Image(stats) = &info.extras else {
        panic!("expected Image extras");
    };
    assert_eq!(stats.width, 1500);
    assert_eq!(stats.height, 1000);
    assert_eq!(stats.bit_depth, 8);
    assert!(
        stats.icc_profile.as_deref().unwrap_or("").contains("sRGB"),
        "expected sRGB ICC profile, got {:?}",
        stats.icc_profile,
    );
    assert!(
        stats.exif.iter().any(|(k, _)| k == "X Resolution"),
        "expected EXIF X Resolution",
    );
    assert!(stats.animation.is_none(), "JPEG should not be animated");
    assert!(stats.hdr_format.is_none(), "cozy-room is SDR");
}

#[test]
fn jpeg_river_woods_is_ultra_hdr_with_camera_metadata() {
    let info = gather_fixture("test-images/river-woods-hdr.jpg");
    let FileExtras::Image(stats) = &info.extras else {
        panic!("expected Image extras");
    };
    assert_eq!(
        stats.hdr_format.as_deref(),
        Some("Ultra HDR (gain map)"),
        "expected Ultra HDR marker",
    );
    let make = stats
        .exif
        .iter()
        .find(|(k, _)| k == "Camera Make")
        .map(|(_, v)| v.as_str());
    assert!(
        matches!(make, Some(v) if v.contains("Google")),
        "expected Google EXIF Camera Make, got {make:?}",
    );
    assert!(
        stats.exif.iter().any(|(k, _)| k == "GPS Latitude"),
        "expected GPS coordinates in EXIF",
    );
}

#[test]
fn png_clover_has_dimensions() {
    let info = gather_fixture("test-images/clover.png");
    let FileExtras::Image(stats) = &info.extras else {
        panic!("expected Image extras");
    };
    assert_eq!(stats.width, 640);
    assert_eq!(stats.height, 599);
    assert!(stats.animation.is_none(), "static PNG must not be animated");
}

#[test]
fn gif_lightning_animation_stats() {
    let info = gather_fixture("test-images/lightning.gif");
    let FileExtras::Image(stats) = &info.extras else {
        panic!("expected animated GIF extras");
    };
    let Some(AnimationStats {
        frame_count,
        total_duration_ms,
        loop_count,
    }) = &stats.animation
    else {
        panic!("expected GIF animation stats");
    };
    assert_eq!(*frame_count, Some(10));
    let dur = total_duration_ms.expect("duration");
    assert!(dur > 0, "duration should be positive");
    assert!(
        matches!(loop_count, Some(LoopCount::Infinite)),
        "GIF should loop forever, got {loop_count:?}",
    );
}

#[test]
fn webp_rickroll_animation_stats() {
    let info = gather_fixture("test-images/rickroll.webp");
    let FileExtras::Image(stats) = &info.extras else {
        panic!("expected animated WebP extras");
    };
    let Some(AnimationStats {
        frame_count,
        total_duration_ms,
        loop_count,
    }) = &stats.animation
    else {
        panic!("expected WebP animation stats");
    };
    assert_eq!(*frame_count, Some(16));
    assert!(total_duration_ms.is_some_and(|d| d > 0));
    assert!(matches!(loop_count, Some(LoopCount::Infinite)));
}

#[test]
fn svg_calendar_extras() {
    let info = gather_fixture("test-images/calendar.svg");
    let FileExtras::Svg(stats) = &info.extras else {
        panic!("expected SVG extras");
    };
    assert_eq!(stats.view_box.as_deref(), Some("-1 -1 18 18"));
    assert_eq!(stats.declared_width.as_deref(), Some("50"));
    assert_eq!(stats.declared_height.as_deref(), Some("50"));
    assert_eq!(stats.path_count, 2);
    assert!(!stats.has_script);
    assert!(!stats.has_external_href);
    // SVG carries text stats too
    assert!(stats.text.line_count > 0);
}

// ---------------------------------------------------------------------------
// Structured fixtures
// ---------------------------------------------------------------------------

#[test]
fn json_config_top_level_object() {
    let info = gather_fixture("test-data/config.json");
    let FileExtras::Structured(info) = &info.extras else {
        panic!("expected Structured JSON extras with stats");
    };
    let stats = info.stats.as_ref().expect("expected stats");
    assert_eq!(info.format_name, "JSON");
    assert!(matches!(stats.top_level_kind, TopLevelKind::Object));
    assert_eq!(stats.top_level_count, 8);
    assert!(stats.max_depth >= 3);
    assert!(stats.total_nodes > 0);
}

#[test]
fn yaml_servers_is_object() {
    let info = gather_fixture("test-data/servers.yaml");
    let FileExtras::Structured(info) = &info.extras else {
        panic!("expected YAML stats");
    };
    let stats = info.stats.as_ref().expect("expected stats");
    assert_eq!(info.format_name, "YAML");
    assert!(matches!(stats.top_level_kind, TopLevelKind::Object));
    assert!(stats.top_level_count >= 1);
}

#[test]
fn toml_project_is_table() {
    let info = gather_fixture("test-data/project.toml");
    let FileExtras::Structured(info) = &info.extras else {
        panic!("expected TOML stats");
    };
    let stats = info.stats.as_ref().expect("expected stats");
    assert_eq!(info.format_name, "TOML");
    assert!(matches!(stats.top_level_kind, TopLevelKind::Table));
    assert!(stats.top_level_count >= 1);
}

#[test]
fn xml_bookstore_root_element_and_namespaces_empty() {
    let info = gather_fixture("test-data/bookstore.xml");
    let FileExtras::Structured(info) = &info.extras else {
        panic!("expected XML stats");
    };
    let stats = info.stats.as_ref().expect("expected stats");
    assert_eq!(info.format_name, "XML");
    assert_eq!(stats.xml_root.as_deref(), Some("bookstore"));
    assert!(stats.total_nodes > 0);
}

#[test]
fn xml_feed_records_namespaces() {
    let info = gather_fixture("test-data/feed.xml");
    let FileExtras::Structured(info) = &info.extras else {
        panic!("expected XML stats");
    };
    let stats = info.stats.as_ref().expect("expected stats");
    assert_eq!(stats.xml_root.as_deref(), Some("rss"));
    assert!(
        stats.xml_namespaces.iter().any(|n| n.contains("atom=")),
        "expected atom namespace, got {:?}",
        stats.xml_namespaces,
    );
}

#[test]
fn html_dashboard_parses_with_lenient_xml() {
    // dashboard.html is HTML, not strict XML — the lenient parser should
    // still return stats with `html` as the root element rather than
    // bailing out entirely.
    let info = gather_fixture("test-data/dashboard.html");
    let FileExtras::Structured(info) = &info.extras else {
        panic!("expected XML stats for dashboard.html");
    };
    let stats = info.stats.as_ref().expect("expected stats");
    assert_eq!(stats.xml_root.as_deref(), Some("html"));
    assert!(stats.total_nodes > 0);
}

// ---------------------------------------------------------------------------
// Text fixtures
// ---------------------------------------------------------------------------

/// End-to-end text stats on a fixture pinned to LF in
/// `.gitattributes` (`*.rs ... eol=lf`). `*.py` without an explicit
/// `eol=lf` would auto-CRLF on Windows under default
/// `core.autocrlf=true`, breaking the `LineEndings::Lf` assertion;
/// the .rs fixture stays portable.
#[test]
fn rust_theme_text_metrics() {
    let info = gather_fixture("test-data/theme.rs");
    let FileExtras::Text(stats) = &info.extras else {
        panic!("expected Text extras");
    };
    assert_eq!(stats.line_count, 104);
    assert!(matches!(stats.line_endings, LineEndings::Lf));
    assert!(matches!(stats.indent_style, Some(IndentStyle::Spaces(4))));
    assert!(matches!(stats.encoding, Encoding::Utf8));
}

#[test]
fn typescript_event_bus_uses_two_space_indent() {
    let info = gather_fixture("test-data/event-bus.ts");
    let FileExtras::Text(stats) = &info.extras else {
        panic!("expected Text extras");
    };
    assert!(matches!(stats.indent_style, Some(IndentStyle::Spaces(2))));
    assert!(stats.line_count > 0);
}

#[test]
fn java_http_server_indent_eight_spaces() {
    let info = gather_fixture("test-data/HttpServer.java");
    let FileExtras::Text(stats) = &info.extras else {
        panic!("expected Text extras");
    };
    assert!(matches!(stats.indent_style, Some(IndentStyle::Spaces(8))));
}

#[test]
fn tsconfig_json5_routed_as_structured() {
    let info = gather_fixture("test-data/tsconfig.json5");
    let FileExtras::Structured(info) = &info.extras else {
        panic!(
            "expected Structured extras, got {:?}",
            std::mem::discriminant(&info.extras)
        );
    };
    assert_eq!(info.format_name, "JSON5");
}

#[test]
fn css_styles_sidecar_stats() {
    let info = gather_fixture("test-data/styles.css");
    let FileExtras::Css(css) = &info.extras else {
        panic!("expected Css extras");
    };
    let stats = &css.stats;
    // Three `@media` blocks, three `@keyframes`; the two `@container`
    // rules must not be miscounted as media queries.
    assert_eq!(stats.media_query_count, 3);
    assert_eq!(stats.keyframes_count, 3);
    // Single `@import`, a relative path — not flagged external.
    assert_eq!(stats.imports.len(), 1);
    assert!(!stats.imports[0].external);
    assert_eq!(stats.imports[0].url, "./reset.css");
    // Rule count includes CSS-nested rules.
    assert_eq!(stats.rule_count, 69);
    assert_eq!(stats.selector_count, 86);
    assert!(stats.selector_kinds.class > 0);
    assert!(stats.selector_kinds.element > 0);
    assert!(stats.selector_kinds.pseudo > 0);
    assert!(stats.custom_property_count > 0);
    // `--color-accent: #ff6b9d` is in the palette; the `red` inside the
    // `content:` string and comments must not appear as a colour.
    assert!(stats.palette.iter().any(|c| c.hex == "#ff6b9d"));
    assert_eq!(stats.palette.len(), stats.total_colors);
}

// ---------------------------------------------------------------------------
// SQLite fixture
// ---------------------------------------------------------------------------

/// library.sqlite (Project Gutenberg catalogue) runs the full detect →
/// gather pipeline: it must classify as SQLite and scrape the catalogue
/// counts + file pragmas. Guards against an `infer` / `rusqlite` bump
/// silently breaking detection or pragma scraping.
#[test]
fn sqlite_library_catalogue_stats() {
    let path = fixture("test-data/library.sqlite");
    let source = InputSource::File(path);
    let detected = detect::detect(&source).expect("detect");
    assert!(
        matches!(detected.file_type, FileType::Sqlite(_)),
        "expected FileType::Sqlite, got {:?}",
        detected.file_type,
    );

    let info = gather(&source, &detected).expect("gather");
    let FileExtras::Sqlite(sqlite) = &info.extras else {
        panic!("expected Sqlite extras");
    };
    let stats = sqlite.stats.as_ref().expect("scrape succeeded");
    // 8 user tables, 1 view, 5 user indexes (sqlite_* shadow entities
    // are filtered out by the catalogue walker).
    assert_eq!(stats.table_count, 8);
    assert_eq!(stats.view_count, 1);
    assert_eq!(stats.index_count, 5);
    // Sum of COUNT(*) across the user tables.
    assert_eq!(stats.total_rows, 25_895);
    // File-level pragmas.
    assert_eq!(stats.page_size, 4096);
    assert_eq!(stats.encoding, "UTF-8");
    assert!(stats.integrity_ok, "fixture passes integrity_check");
}

#[test]
fn java_classfile_sample_metadata() {
    let info = gather_fixture("test-data/Sample.class");
    let FileExtras::Classfile(cf) = &info.extras else {
        panic!("expected Classfile extras");
    };
    let meta = cf.meta.as_ref().expect("classfile parsed");
    assert_eq!(meta.class_name, "Sample");
    assert_eq!(meta.super_class.as_deref(), Some("java.lang.Object"));
    assert!(
        meta.interfaces.iter().any(|i| i == "java.lang.Comparable"),
        "expected Comparable interface, got {:?}",
        meta.interfaces,
    );
    assert_eq!(meta.major_version, 61, "fixture compiled with JDK 17");
    assert_eq!(meta.source_file.as_deref(), Some("Sample.java"));
    assert_eq!(meta.field_count, 4);
    // 6 declared methods + the synthetic compareTo(Object) bridge.
    assert!(meta.method_count >= 6);
}

// ---------------------------------------------------------------------------
// EPS / PostScript / Illustrator fixtures
//
// These exercise detection + DSC parse + embedded-preview decode, none of
// which need Ghostscript. The only gs-dependent fact — whether the Render
// view is offered — is asserted against `gs::find()` so the suite passes
// identically with or without an interpreter installed.
// ---------------------------------------------------------------------------

/// Plain multi-page `.ps`: detects as PostScript, parses the full DSC
/// header, and carries no embedded preview.
#[test]
fn postscript_sample_ps_dsc_and_no_preview() {
    let path = fixture("test-images/postscript-sample.ps");
    let source = InputSource::File(path);
    let detected = detect::detect(&source).expect("detect");
    assert_eq!(
        detected.file_type,
        FileType::PostScript(PostScriptFormat::Ps),
        "expected plain PostScript",
    );

    let info = gather(&source, &detected).expect("gather");
    let FileExtras::Eps(eps) = &info.extras else {
        panic!("expected Eps extras");
    };
    assert_eq!(eps.format, PostScriptFormat::Ps);
    assert_eq!(eps.dsc.title.as_deref(), Some("peek PostScript sample"));
    assert_eq!(eps.dsc.creator.as_deref(), Some("peek test suite"));
    assert_eq!(eps.dsc.pages.as_deref(), Some("2"));
    assert_eq!(eps.dsc.language_level.as_deref(), Some("2"));
    assert_eq!(eps.dsc.bounding_box.as_deref(), Some("0 0 612 792"));
    assert!(eps.preview.is_none(), "plain .ps has no embedded preview");
    // gs availability is environment-dependent — only assert it tracks
    // what the bridge actually finds, never that it's present.
    assert_eq!(eps.gs_available, gs::find().is_some());
}

/// Binary DOS-EPS whose embedded preview is a palette TIFF the image
/// crate can't decode: detected as EPS, preview present but undecodable
/// (recorded, dimensions `None`), DSC fully parsed.
#[test]
fn tropical_jungle_eps_has_undecodable_tiff_preview() {
    let info = gather_fixture("test-images/tropical-jungle.eps");
    let FileExtras::Eps(eps) = &info.extras else {
        panic!("expected Eps extras");
    };
    assert_eq!(eps.format, PostScriptFormat::Eps);
    assert_eq!(eps.dsc.creator.as_deref(), Some("Adobe Illustrator(R) 12"));
    // The bare-`\r` line ending bug used to swallow `%%For` — guard it.
    assert_eq!(eps.dsc.for_whom.as_deref(), Some("Mili Skobic"));
    assert_eq!(eps.dsc.pages.as_deref(), Some("1"));
    let preview = eps.preview.as_ref().expect("DOS-EPS preview present");
    assert_eq!(preview.kind, PreviewKind::Tiff);
    assert!(preview.bytes > 0);
    assert!(
        preview.dimensions.is_none(),
        "palette TIFF isn't decodable by the image crate",
    );
}

/// DOS-EPS with a re-encoded RGB TIFF preview the image crate *can*
/// decode: preview present with real pixel dimensions.
#[test]
fn tropical_jungle_rgbpreview_eps_decodes_preview() {
    let info = gather_fixture("test-images/tropical-jungle-rgbpreview.eps");
    let FileExtras::Eps(eps) = &info.extras else {
        panic!("expected Eps extras");
    };
    assert_eq!(eps.format, PostScriptFormat::Eps);
    let preview = eps.preview.as_ref().expect("preview present");
    assert_eq!(preview.kind, PreviewKind::Tiff);
    assert_eq!(preview.dimensions, Some((222, 256)));
}

/// EPS reduced to its PostScript section: detected as EPS via extension,
/// DSC intact, no preview.
#[test]
fn tropical_jungle_nopreview_eps_has_no_preview() {
    let info = gather_fixture("test-images/tropical-jungle-nopreview.eps");
    let FileExtras::Eps(eps) = &info.extras else {
        panic!("expected Eps extras");
    };
    assert_eq!(eps.format, PostScriptFormat::Eps);
    assert_eq!(eps.dsc.title.as_deref(), Some("tropical-jungle.eps"));
    assert!(eps.preview.is_none());
}

/// Modern Illustrator `.ai` is a PDF: detected as the Illustrator PDF
/// flavour, no extension-mismatch warning, real page/version stats.
/// Pure PDF metadata — no Ghostscript involved.
#[test]
fn bonfire_nature_ai_is_illustrator_pdf() {
    let path = fixture("test-images/bonfire-nature.ai");
    let source = InputSource::File(path);
    let detected = detect::detect(&source).expect("detect");
    assert_eq!(
        detected.file_type,
        FileType::Pdf(PdfFlavor::Illustrator),
        "expected Illustrator PDF flavour",
    );

    let info = gather(&source, &detected).expect("gather");
    assert!(
        !info.warnings.iter().any(|w| w.contains("extension")),
        "`.ai` over %PDF magic must not warn, got {:?}",
        info.warnings,
    );
    let FileExtras::Pdf(pdf) = &info.extras else {
        panic!("expected Pdf extras");
    };
    assert_eq!(pdf.flavor, PdfFlavor::Illustrator);
    assert_eq!(pdf.page_count, 1);
    assert_eq!(pdf.pdf_version, "1.4");
}

// ---------------------------------------------------------------------------
// Spreadsheet fixtures (no Ghostscript / external tools needed)
// ---------------------------------------------------------------------------

/// `.xlsx` detects as the Excel spreadsheet flavour, lists its sheets,
/// pulls core-properties metadata from `docProps/core.xml`, and doesn't
/// warn on the zip-magic-vs-`.xlsx`-extension mismatch.
#[test]
fn xlsx_people_workbook_lists_sheets_and_metadata() {
    let path = fixture("test-data/people.xlsx");
    let source = InputSource::File(path);
    let detected = detect::detect(&source).expect("detect");
    assert_eq!(
        detected.file_type,
        FileType::Spreadsheet(SpreadsheetFormat::Xlsx),
    );

    let info = gather(&source, &detected).expect("gather");
    assert!(
        !info.warnings.iter().any(|w| w.contains("extension")),
        "`.xlsx` over application/zip magic must not warn, got {:?}",
        info.warnings,
    );
    let FileExtras::Spreadsheet(wb) = &info.extras else {
        panic!("expected Spreadsheet extras");
    };
    assert_eq!(wb.sheets, vec!["people".to_string(), "totals".to_string()]);
    assert_eq!(wb.metadata.creator.as_deref(), Some("openpyxl"));
}

/// `.ods` detects as the OpenDocument spreadsheet flavour and lists its
/// sheets.
#[test]
fn ods_people_workbook_lists_sheets() {
    let path = fixture("test-data/people.ods");
    let source = InputSource::File(path);
    let detected = detect::detect(&source).expect("detect");
    assert_eq!(
        detected.file_type,
        FileType::Spreadsheet(SpreadsheetFormat::Ods),
    );

    let info = gather(&source, &detected).expect("gather");
    let FileExtras::Spreadsheet(wb) = &info.extras else {
        panic!("expected Spreadsheet extras");
    };
    assert_eq!(wb.sheets, vec!["people".to_string(), "totals".to_string()]);
}
