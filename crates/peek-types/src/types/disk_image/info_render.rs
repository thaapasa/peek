//! Disk-image info section rendering. ISO 9660 today; future formats
//! plug into this same section header by adding their own block here
//! and a matching arm in `gather_extras`.

use crate::info::{format_size_human, push_field, push_section_header, thousands_sep};
use crate::theme::PeekTheme;
use crate::types::disk_image::info::{
    DiskImageInfo, DiskImageMeta, DmgChecksumKind, DmgMeta, DmgPartition, DmgVariant, IsoDateTime,
    IsoVolumeMeta, MbrPartition, RawImageMeta,
};
use crate::types::disk_image::mbr;

pub fn render_section(lines: &mut Vec<String>, info: &DiskImageInfo, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Disk Image", theme);
    push_field(lines, "Format", &theme.paint_value(info.format_name), theme);

    if let Some(err) = &info.error {
        push_field(lines, "Status", &theme.paint_warning(err), theme);
        return;
    }

    match &info.meta {
        Some(DiskImageMeta::Iso(iso)) => render_iso(lines, iso, theme),
        Some(DiskImageMeta::Dmg(dmg)) => render_dmg(lines, dmg, theme),
        Some(DiskImageMeta::Raw(raw)) => render_raw(lines, raw, theme),
        None => {}
    }
}

fn render_raw(lines: &mut Vec<String>, raw: &RawImageMeta, theme: &PeekTheme) {
    let Some(table) = &raw.mbr else {
        push_field(
            lines,
            "Layout",
            &theme.paint_value(
                "no recognised partition table — appears to be a flat filesystem dump",
            ),
            theme,
        );
        return;
    };
    push_field(
        lines,
        "Layout",
        &theme.paint_value(&format!(
            "MBR ({} partition{})",
            table.partitions.len(),
            if table.partitions.len() == 1 { "" } else { "s" }
        )),
        theme,
    );
    for (i, p) in table.partitions.iter().enumerate() {
        let label = format!("Part {}", i + 1);
        push_field(lines, &label, &paint_partition(p, theme), theme);
    }
}

fn paint_partition(p: &MbrPartition, theme: &PeekTheme) -> String {
    // Sectors are 512 bytes per IBM PC convention. Real disks may
    // use 4 KiB sectors but the MBR table itself is rooted in the
    // 512-byte assumption — honour that here.
    let bytes = (p.sectors as u64) * 512;
    let kind = mbr::type_label(p.type_code)
        .map(String::from)
        .unwrap_or_else(|| format!("0x{:02X}", p.type_code));
    let boot = if p.bootable { " *" } else { "" };
    format!(
        "{}{boot}  start LBA {}  ({} sectors, {} bytes)",
        theme.paint_value(&kind),
        thousands_sep(p.start_lba as u64),
        thousands_sep(p.sectors as u64),
        thousands_sep(bytes),
    )
}

fn render_iso(lines: &mut Vec<String>, iso: &IsoVolumeMeta, theme: &PeekTheme) {
    if let Some(label) = &iso.volume_label {
        push_field(lines, "Volume", &theme.paint_value(label), theme);
    }
    if let Some(set) = &iso.volume_set_id {
        push_field(lines, "Volume set", &theme.paint_value(set), theme);
    }
    if let Some(sys) = &iso.system_id {
        push_field(lines, "System", &theme.paint_value(sys), theme);
    }
    if let Some(p) = &iso.publisher {
        push_field(lines, "Publisher", &theme.paint_value(p), theme);
    }
    if let Some(p) = &iso.data_preparer {
        push_field(lines, "Data preparer", &theme.paint_value(p), theme);
    }
    if let Some(a) = &iso.application {
        push_field(lines, "Application", &theme.paint_value(a), theme);
    }

    let total_bytes = iso.block_count as u64 * iso.block_size as u64;
    push_field(
        lines,
        "Volume size",
        &theme.paint_value(&format!(
            "{} bytes ({} × {} blocks)",
            thousands_sep(total_bytes),
            thousands_sep(iso.block_count as u64),
            iso.block_size,
        )),
        theme,
    );

    if let Some(dt) = &iso.creation {
        push_field(lines, "Created", &theme.paint_value(&format_dt(dt)), theme);
    }
    if let Some(dt) = &iso.modification {
        push_field(lines, "Modified", &theme.paint_value(&format_dt(dt)), theme);
    }
    if let Some(dt) = &iso.expiration {
        push_field(lines, "Expires", &theme.paint_value(&format_dt(dt)), theme);
    }
    if let Some(dt) = &iso.effective {
        push_field(
            lines,
            "Effective",
            &theme.paint_value(&format_dt(dt)),
            theme,
        );
    }

    let extensions = format_extensions(iso);
    push_field(lines, "Extensions", &theme.paint_value(&extensions), theme);

    if iso.el_torito
        && let Some(id) = &iso.el_torito_id
    {
        push_field(lines, "Boot loader", &theme.paint_value(id), theme);
    }
}

fn render_dmg(lines: &mut Vec<String>, dmg: &DmgMeta, theme: &PeekTheme) {
    push_field(
        lines,
        "UDIF version",
        &theme.paint_value(&dmg.udif_version.to_string()),
        theme,
    );
    push_field(
        lines,
        "Variant",
        &theme.paint_value(variant_label(dmg.variant)),
        theme,
    );
    push_field(
        lines,
        "Volume size",
        &theme.paint_value(&format!("{} bytes", thousands_sep(dmg.total_size_bytes))),
        theme,
    );
    push_field(
        lines,
        "Data fork",
        &theme.paint_value(&format!("{} bytes", thousands_sep(dmg.data_fork_length))),
        theme,
    );
    push_field(
        lines,
        "Plist",
        &theme.paint_value(&plist_label(dmg.plist_present, dmg.plist_length)),
        theme,
    );
    if dmg.segment_count > 1 {
        push_field(
            lines,
            "Segments",
            &theme.paint_value(&format!("{} of {}", dmg.segment_number, dmg.segment_count)),
            theme,
        );
    }
    push_field(
        lines,
        "Data checksum",
        &theme.paint_value(checksum_label(dmg.data_checksum_type)),
        theme,
    );
    push_field(
        lines,
        "Master checksum",
        &theme.paint_value(checksum_label(dmg.master_checksum_type)),
        theme,
    );
    let flags = format_dmg_flags(dmg.flags);
    push_field(lines, "Flags", &theme.paint_value(&flags), theme);
    render_dmg_partitions(lines, &dmg.partitions, theme);
}

/// Render the decoded partition map. Filesystems each get a detail block;
/// the format scaffolding (MBR / GPT structures / free-space gaps)
/// collapses into one "Partition scheme" block, one terse line each — no
/// entry is ever hidden. Skipped entirely when no partitions decoded.
fn render_dmg_partitions(lines: &mut Vec<String>, parts: &[DmgPartition], theme: &PeekTheme) {
    if parts.is_empty() {
        return;
    }
    let (filesystems, scheme): (Vec<&DmgPartition>, Vec<&DmgPartition>) =
        parts.iter().partition(|p| !is_structural(p));

    let mut summary = pluralise(filesystems.len(), "filesystem");
    if !scheme.is_empty() {
        summary.push_str(&format!(", {} scheme", scheme.len()));
    }
    push_field(
        lines,
        "Partitions",
        &theme.paint_value(&format!("{} ({summary})", parts.len())),
        theme,
    );

    for p in &filesystems {
        render_partition_block(lines, p, theme);
    }
    if !scheme.is_empty() {
        render_scheme_block(lines, &scheme, theme);
    }
}

/// Full detail block for one filesystem partition.
fn render_partition_block(lines: &mut Vec<String>, p: &DmgPartition, theme: &PeekTheme) {
    lines.push(String::new());
    let title = match &p.fs_type {
        Some(t) => format!("Partition \u{b7} {}", friendly_type(t)),
        None => "Partition".to_string(),
    };
    push_section_header(lines, &title, theme);

    push_field(lines, "Name", &theme.paint_value(&p.name), theme);
    if let Some(t) = &p.fs_type {
        let friendly = friendly_type(t);
        let val = if friendly == *t {
            t.clone()
        } else {
            format!("{friendly} ({t})")
        };
        push_field(lines, "Type", &theme.paint_value(&val), theme);
    }
    push_field(
        lines,
        "Logical size",
        &theme.paint_value(&format_size_human(p.size_bytes)),
        theme,
    );
    push_field(lines, "Stored", &theme.paint_value(&stored_desc(p)), theme);
    push_field(
        lines,
        "Compression",
        &theme.paint_value(&compression_desc(p)),
        theme,
    );
    push_field(lines, "Chunks", &theme.paint_value(&chunks_desc(p)), theme);
    push_field(
        lines,
        "Image offset",
        &theme.paint_value(&offset_desc(p)),
        theme,
    );
}

/// Compact block for the format scaffolding — one line per entry, each
/// still carrying its size, codec, and image offset.
fn render_scheme_block(lines: &mut Vec<String>, scheme: &[&DmgPartition], theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Partition scheme", theme);
    for p in scheme {
        let label = p
            .fs_type
            .as_deref()
            .map(friendly_type)
            .unwrap_or_else(|| p.name.clone());
        let codec = if !p.compression.is_empty() {
            p.compression.join("+")
        } else if p.stored_bytes == 0 {
            "sparse".to_string()
        } else {
            "raw".to_string()
        };
        let offset = thousands_sep(p.start_sector.saturating_mul(512));
        let value = format!(
            "{} \u{b7} {codec} \u{b7} @ {offset} B",
            format_size_human(p.size_bytes)
        );
        push_field(lines, &label, &theme.paint_value(&value), theme);
    }
}

/// `"159.64 MiB (27% of logical)"`, or `"0 (sparse)"` for a zero-fill
/// partition.
fn stored_desc(p: &DmgPartition) -> String {
    if p.stored_bytes == 0 {
        return "0 (sparse)".to_string();
    }
    let pct = if p.size_bytes > 0 {
        (p.stored_bytes as f64 / p.size_bytes as f64 * 100.0).round() as u64
    } else {
        0
    };
    format!("{} ({pct}% of logical)", format_size_human(p.stored_bytes))
}

/// `"zlib · 3.7×"`, `"none (sparse)"`, or `"none (uncompressed)"`.
fn compression_desc(p: &DmgPartition) -> String {
    if !p.compression.is_empty() && p.stored_bytes > 0 {
        let ratio = p.size_bytes as f64 / p.stored_bytes as f64;
        format!("{} \u{b7} {ratio:.1}\u{d7}", p.compression.join("+"))
    } else if p.stored_bytes == 0 {
        "none (sparse)".to_string()
    } else {
        "none (uncompressed)".to_string()
    }
}

/// `"408 (1 zero-fill, 2 raw, 405 zlib)"` — count plus run-type histogram.
fn chunks_desc(p: &DmgPartition) -> String {
    if p.chunk_count == 0 {
        return "0".to_string();
    }
    let hist = p
        .run_histogram
        .iter()
        .map(|(label, n)| format!("{n} {label}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{} ({hist})", p.chunk_count)
}

/// `"36,864 B (sector 72)"` — the byte offset external tools select on.
fn offset_desc(p: &DmgPartition) -> String {
    format!(
        "{} B (sector {})",
        thousands_sep(p.start_sector.saturating_mul(512)),
        thousands_sep(p.start_sector)
    )
}

/// Format scaffolding vs a real filesystem. Classifies by the Apple type
/// token; an unknown / missing token errs toward "filesystem" (full
/// block) so novelty surfaces more detail, never less.
fn is_structural(p: &DmgPartition) -> bool {
    match &p.fs_type {
        Some(t) => {
            let lower = t.to_ascii_lowercase();
            t == "MBR"
                || t == "DDM"
                || lower.contains("gpt")
                || lower.contains("free")
                || lower.contains("partition_map")
                || lower.contains("partition map")
                || lower.contains("driver descriptor")
        }
        None => false,
    }
}

/// Map an Apple type token to a readable filesystem name, or return it
/// unchanged when there's nothing friendlier to say.
fn friendly_type(token: &str) -> String {
    match token {
        "Apple_HFS" => "HFS+",
        "Apple_HFSX" => "HFSX",
        "Apple_APFS" => "APFS",
        "Apple_UFS" => "UFS",
        "Apple_Free" => "free space",
        "Apple_partition_map" => "Apple partition map",
        other => other,
    }
    .to_string()
}

/// `"1 filesystem"` / `"2 filesystems"` / `"0 filesystems"`.
fn pluralise(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

fn variant_label(variant: DmgVariant) -> &'static str {
    match variant {
        DmgVariant::Device => "device image",
        DmgVariant::Partition => "partition",
        DmgVariant::MountedSystem => "mounted system",
        DmgVariant::Other(_) => "unknown",
    }
}

fn checksum_label(kind: DmgChecksumKind) -> &'static str {
    match kind {
        DmgChecksumKind::None => "none",
        DmgChecksumKind::Crc32 => "CRC-32",
        DmgChecksumKind::Md5 => "MD5",
        DmgChecksumKind::Sha1 => "SHA-1",
        DmgChecksumKind::Sha256 => "SHA-256",
        DmgChecksumKind::Sha512 => "SHA-512",
        DmgChecksumKind::Other(_) => "unknown",
    }
}

fn plist_label(present: bool, length: u64) -> String {
    if !present {
        return "none".to_string();
    }
    format!("{} bytes (XML partition map)", thousands_sep(length))
}

/// Decode the few flag bits Apple actually documents for the UDIF
/// trailer. Unknown bits are reported as a hex tail so unfamiliar
/// images don't get silently flattened to "(none)".
fn format_dmg_flags(raw: u32) -> String {
    let mut parts: Vec<String> = Vec::new();
    if raw & 0x1 != 0 {
        parts.push("flattened".into());
    }
    if raw & 0x4 != 0 {
        parts.push("internet-enabled".into());
    }
    let unknown = raw & !0x5;
    if unknown != 0 {
        parts.push(format!("0x{unknown:x}"));
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(", ")
    }
}

fn format_dt(dt: &IsoDateTime) -> String {
    let offset = format_offset(dt.gmt_offset_quarters);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {offset}",
        dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second,
    )
}

fn format_offset(quarters: i8) -> String {
    let total_minutes = quarters as i32 * 15;
    let sign = if total_minutes >= 0 { '+' } else { '-' };
    let abs = total_minutes.unsigned_abs();
    let h = abs / 60;
    let m = abs % 60;
    format!("{sign}{h:02}:{m:02}")
}

fn format_extensions(iso: &IsoVolumeMeta) -> String {
    let mut parts: Vec<&'static str> = Vec::new();
    if iso.joliet {
        parts.push("Joliet");
    }
    if iso.el_torito {
        parts.push("El Torito");
    }
    if parts.is_empty() {
        "ISO 9660 only".to_string()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_zero_renders_plus_zero() {
        assert_eq!(format_offset(0), "+00:00");
    }

    #[test]
    fn offset_positive_quarter_hour() {
        // +5:30 (Indian Standard Time) → +22 quarter-hours
        assert_eq!(format_offset(22), "+05:30");
    }

    #[test]
    fn offset_negative() {
        // -5:00 (US Eastern) → -20 quarters
        assert_eq!(format_offset(-20), "-05:00");
    }

    #[test]
    fn datetime_format_is_iso_like() {
        let dt = IsoDateTime {
            year: 2025,
            month: 1,
            day: 15,
            hour: 14,
            minute: 30,
            second: 0,
            gmt_offset_quarters: 0,
        };
        assert_eq!(format_dt(&dt), "2025-01-15 14:30:00 +00:00");
    }

    #[test]
    fn dmg_flags_zero_is_none() {
        assert_eq!(format_dmg_flags(0), "none");
    }

    #[test]
    fn dmg_flags_known_bits() {
        assert_eq!(format_dmg_flags(0x1), "flattened");
        assert_eq!(format_dmg_flags(0x5), "flattened, internet-enabled");
    }

    #[test]
    fn dmg_flags_unknown_bits_surface_as_hex() {
        assert_eq!(format_dmg_flags(0x12), "0x12");
        assert_eq!(format_dmg_flags(0x11), "flattened, 0x10");
    }

    fn part(fs_type: Option<&str>) -> DmgPartition {
        DmgPartition {
            name: "x".to_string(),
            fs_type: fs_type.map(str::to_string),
            start_sector: 0,
            size_bytes: 0,
            stored_bytes: 0,
            compression: Vec::new(),
            chunk_count: 0,
            run_histogram: Vec::new(),
        }
    }

    #[test]
    fn filesystems_split_from_scaffolding() {
        assert!(!is_structural(&part(Some("Apple_HFS"))));
        assert!(!is_structural(&part(Some("Apple_APFS"))));
        // Unknown type errs toward a filesystem block (more detail).
        assert!(!is_structural(&part(Some("Some_New_FS"))));
        assert!(!is_structural(&part(None)));

        assert!(is_structural(&part(Some("MBR"))));
        assert!(is_structural(&part(Some("Primary GPT Header"))));
        assert!(is_structural(&part(Some("Backup GPT Table"))));
        assert!(is_structural(&part(Some("Apple_Free"))));
        assert!(is_structural(&part(Some("DDM"))));
        assert!(is_structural(&part(Some("Apple_partition_map"))));
    }

    #[test]
    fn friendly_type_maps_known_and_passes_through() {
        assert_eq!(friendly_type("Apple_HFS"), "HFS+");
        assert_eq!(friendly_type("Apple_APFS"), "APFS");
        assert_eq!(friendly_type("Apple_Free"), "free space");
        // Unmapped token is returned verbatim.
        assert_eq!(friendly_type("Primary GPT Header"), "Primary GPT Header");
    }

    #[test]
    fn chunks_desc_shows_count_and_histogram() {
        let mut p = part(Some("Apple_HFS"));
        p.chunk_count = 408;
        p.run_histogram = vec![("raw", 1), ("ignore", 4), ("zlib", 403)];
        assert_eq!(chunks_desc(&p), "408 (1 raw, 4 ignore, 403 zlib)");
    }

    #[test]
    fn pluralise_filesystem_count() {
        assert_eq!(pluralise(0, "filesystem"), "0 filesystems");
        assert_eq!(pluralise(1, "filesystem"), "1 filesystem");
        assert_eq!(pluralise(3, "filesystem"), "3 filesystems");
    }

    #[test]
    fn stored_desc_percent_and_sparse() {
        let mut p = part(Some("Apple_HFS"));
        p.size_bytes = 1000;
        p.stored_bytes = 270;
        assert_eq!(stored_desc(&p), "270 B (27% of logical)");

        p.stored_bytes = 0;
        assert_eq!(stored_desc(&p), "0 (sparse)");
    }

    #[test]
    fn compression_desc_ratio_sparse_uncompressed() {
        let mut p = part(Some("Apple_HFS"));
        p.size_bytes = 1000;
        p.stored_bytes = 270;
        p.compression = vec!["zlib"];
        assert_eq!(compression_desc(&p), "zlib \u{b7} 3.7\u{d7}");

        p.compression = Vec::new();
        p.stored_bytes = 0;
        assert_eq!(compression_desc(&p), "none (sparse)");

        p.stored_bytes = 500;
        assert_eq!(compression_desc(&p), "none (uncompressed)");
    }

    #[test]
    fn offset_desc_is_byte_offset_with_sector() {
        let mut p = part(Some("Apple_HFS"));
        p.start_sector = 40;
        assert_eq!(offset_desc(&p), "20,480 B (sector 40)");
        p.start_sector = 0;
        assert_eq!(offset_desc(&p), "0 B (sector 0)");
    }

    fn test_theme() -> PeekTheme {
        let t = crate::theme::load_embedded_theme(
            crate::theme::PeekThemeName::IdeaDark.tmtheme_source(),
        );
        PeekTheme::from_syntect(&t)
    }

    #[test]
    fn render_routes_filesystem_to_block_and_scaffolding_to_scheme() {
        let fs = DmgPartition {
            name: "disk image (Apple_HFS : 4)".to_string(),
            fs_type: Some("Apple_HFS".to_string()),
            start_sector: 40,
            size_bytes: 2048 * 512,
            stored_bytes: 159 * 1024 * 1024,
            compression: vec!["zlib"],
            chunk_count: 408,
            run_histogram: vec![("raw", 1), ("zlib", 407)],
        };
        let mbr = DmgPartition {
            name: "Protective Master Boot Record (MBR : 0)".to_string(),
            fs_type: Some("MBR".to_string()),
            start_sector: 0,
            size_bytes: 512,
            stored_bytes: 30,
            compression: vec!["zlib"],
            chunk_count: 1,
            run_histogram: vec![("zlib", 1)],
        };

        let theme = test_theme();
        let mut lines = Vec::new();
        render_dmg_partitions(&mut lines, &[mbr, fs], &theme);
        let blob = lines.join("\n");

        assert!(
            blob.contains("2 (1 filesystem, 1 scheme)"),
            "summary line: {blob}"
        );
        // Filesystem gets a full block (heading + Name field).
        assert!(blob.contains("Partition \u{b7} HFS+"), "fs block heading");
        assert!(blob.contains("disk image (Apple_HFS : 4)"), "fs Name field");
        // Scaffolding collapses into the scheme block — the MBR's full
        // name never appears (it'd only render in a full block).
        assert!(blob.contains("Partition scheme"), "scheme heading");
        assert!(
            !blob.contains("Protective Master Boot Record"),
            "MBR must not get a full block: {blob}"
        );
    }
}
