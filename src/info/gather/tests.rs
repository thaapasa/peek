//! Fixture-based tests covering the full detect + gather pipeline against
//! the real test files in `test-images/` and `test-data/`. These complement
//! the synthetic tests in `text` (which exercise streaming-pass edge cases)
//! by anchoring the format-specific extras to known on-disk content.

use std::path::PathBuf;

use super::super::FileExtras;
use super::gather;
use crate::input::InputSource;
use crate::input::detect;
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

#[test]
fn python_fibonacci_text_metrics() {
    let info = gather_fixture("test-data/fibonacci.py");
    let FileExtras::Text(stats) = &info.extras else {
        panic!("expected Text extras");
    };
    assert_eq!(stats.line_count, 80);
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
fn rust_theme_uses_four_space_indent() {
    let info = gather_fixture("test-data/theme.rs");
    let FileExtras::Text(stats) = &info.extras else {
        panic!("expected Text extras");
    };
    assert!(matches!(stats.indent_style, Some(IndentStyle::Spaces(4))));
    assert!(matches!(stats.line_endings, LineEndings::Lf));
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
