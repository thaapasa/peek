//! Disk-image info section rendering. ISO 9660 / DMG / raw images plug into
//! one section header (`Disk Image`), variant-dispatched on [`DiskImageMeta`].
//!
//! The ISO and DMG variants drive *both* outputs — themed print and `--info
//! --json` — from one [`InfoRow`] list per variant (`iso_rows`,
//! `dmg_main_rows`, `partition_rows`), the way `cert` / `font` do.
//! [`push_rows`] emits the print lines; [`rows_to_json`] the JSON object. That
//! kills the former parallel `*_json` listers. A `Value::split` covers each
//! leaf whose print form diverges from its JSON value: a `159.64 MiB` size vs
//! a raw byte count, a `device image` label vs a `device` token, a composite
//! `Volume size` line whose `block_size` / `block_count` are separate JSON
//! keys.
//!
//! The block *framing* is still built by hand per output, because print and
//! JSON genuinely nest differently: print appends one `InfoNode::Block` per
//! DMG filesystem partition plus a collapsed scheme block, while JSON nests
//! the variant under an `iso` / `dmg` / `raw` key with a flat `partitions`
//! array. `info_nodes` and `json_section` are those framers.
//!
//! The raw (MBR) variant stays hand-built: its print rows mix a themed type
//! label with plain numbers in one cell (partial painting no `Value` can
//! express), and one print line maps to a four-field JSON object — neither
//! fits the row model, so `raw_rows` (print) and `raw_json` (JSON) remain
//! separate.

use crate::info::{
    InfoNode, InfoRow, Role, Value, format_size_human, push_rows, render_info, rows_to_json,
    thousands_sep,
};
use crate::types::disk_image::info::{
    DiskImageInfo, DiskImageMeta, DmgChecksumKind, DmgMeta, DmgPartition, DmgVariant, IsoDateTime,
    IsoVolumeMeta, MbrPartition, RawImageMeta,
};
use crate::types::disk_image::mbr;
use peek_theme::PeekTheme;
use serde_json::json;

/// Convenience for a `label  value` row.
fn row(label: &'static str, value: String) -> InfoNode {
    InfoNode::Row {
        label: label.into(),
        value,
    }
}

/// Themed terminal Disk Image section.
pub fn render_section(lines: &mut Vec<String>, info: &DiskImageInfo, theme: &PeekTheme) {
    render_info(lines, &DiskImageView(info), theme);
}

/// Typed `--info --json` view of the Disk Image section, nested under
/// `"disk_image"`. The variant payload nests under `iso` / `dmg` / `raw`,
/// built from the same row lists the print framer uses.
pub fn json_section(info: &DiskImageInfo) -> (&'static str, serde_json::Value) {
    let mut obj = json!({ "format": info.format_name });
    if let Some(err) = &info.error {
        obj["error"] = json!(err);
    }
    match &info.meta {
        Some(DiskImageMeta::Iso(iso)) => {
            obj["iso"] = serde_json::Value::Object(rows_to_json(&iso_rows(iso)))
        }
        Some(DiskImageMeta::Dmg(dmg)) => obj["dmg"] = dmg_json(dmg),
        Some(DiskImageMeta::Raw(raw)) => obj["raw"] = raw_json(raw),
        None => {}
    }
    ("disk_image", obj)
}

struct DiskImageView<'a>(&'a DiskImageInfo);

impl crate::info::InfoView for DiskImageView<'_> {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        let info = self.0;
        let mut body = vec![row("Format", theme.paint_value(info.format_name))];
        if let Some(err) = &info.error {
            body.push(row("Status", theme.paint_warning(err)));
            return vec![InfoNode::Block {
                title: "Disk Image".to_string(),
                body,
            }];
        }
        // Blocks beyond the main one (DMG partition / scheme detail).
        let mut extra = Vec::new();
        match &info.meta {
            Some(DiskImageMeta::Iso(iso)) => push_row_lines(&mut body, &iso_rows(iso), theme),
            Some(DiskImageMeta::Dmg(dmg)) => {
                push_row_lines(&mut body, &dmg_main_rows(dmg), theme);
                dmg_partition_nodes(&mut body, &mut extra, &dmg.partitions, theme);
            }
            Some(DiskImageMeta::Raw(raw)) => raw_rows(&mut body, raw, theme),
            None => {}
        }
        let mut nodes = vec![InfoNode::Block {
            title: "Disk Image".to_string(),
            body,
        }];
        nodes.extend(extra);
        nodes
    }
}

/// Render a row list's print rows into `body` as verbatim `Line` nodes; the
/// enclosing block is framed by the caller. `push_rows` emits the same
/// `push_field` lines an `InfoNode::Row` would, so the output is unchanged.
fn push_row_lines(body: &mut Vec<InfoNode>, rows: &[InfoRow], theme: &PeekTheme) {
    let mut lines = Vec::new();
    push_rows(&mut lines, rows, theme);
    body.extend(lines.into_iter().map(InfoNode::Line));
}

fn raw_rows(main: &mut Vec<InfoNode>, raw: &RawImageMeta, theme: &PeekTheme) {
    let Some(table) = &raw.mbr else {
        main.push(row(
            "Layout",
            theme.paint_value(
                "no recognised partition table — appears to be a flat filesystem dump",
            ),
        ));
        return;
    };
    main.push(row(
        "Layout",
        theme.paint_value(&format!(
            "MBR ({} partition{})",
            table.partitions.len(),
            if table.partitions.len() == 1 { "" } else { "s" }
        )),
    ));
    for (i, p) in table.partitions.iter().enumerate() {
        main.push(InfoNode::Row {
            label: format!("Part {}", i + 1).into(),
            value: paint_partition(p, theme),
        });
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

/// One ISO 9660 volume's rows, driving both print and JSON. The composite
/// `Volume size` / `Extensions` print lines map to flat JSON keys
/// (`block_size`+`block_count`, `joliet`+`el_torito`), so those are split into
/// a print-only row plus the JSON-only keys.
fn iso_rows(iso: &IsoVolumeMeta) -> Vec<InfoRow> {
    let mut r = Vec::new();
    if let Some(v) = &iso.volume_label {
        r.push(InfoRow::text("Volume", "volume_label", v));
    }
    if let Some(v) = &iso.volume_set_id {
        r.push(InfoRow::text("Volume set", "volume_set_id", v));
    }
    if let Some(v) = &iso.system_id {
        r.push(InfoRow::text("System", "system_id", v));
    }
    if let Some(v) = &iso.publisher {
        r.push(InfoRow::text("Publisher", "publisher", v));
    }
    if let Some(v) = &iso.data_preparer {
        r.push(InfoRow::text("Data preparer", "data_preparer", v.clone()));
    }
    if let Some(v) = &iso.application {
        r.push(InfoRow::text("Application", "application", v.clone()));
    }
    let total_bytes = iso.block_count as u64 * iso.block_size as u64;
    r.push(InfoRow::print_only(
        "Volume size",
        Value::text(format!(
            "{} bytes ({} × {} blocks)",
            thousands_sep(total_bytes),
            thousands_sep(iso.block_count as u64),
            iso.block_size,
        )),
    ));
    r.push(InfoRow::json_int("block_size", iso.block_size as i64));
    r.push(InfoRow::json_int("block_count", iso.block_count as i64));
    if let Some(dt) = &iso.creation {
        r.push(InfoRow::text("Created", "creation", format_dt(dt)));
    }
    if let Some(dt) = &iso.modification {
        r.push(InfoRow::text("Modified", "modification", format_dt(dt)));
    }
    if let Some(dt) = &iso.expiration {
        r.push(InfoRow::text("Expires", "expiration", format_dt(dt)));
    }
    if let Some(dt) = &iso.effective {
        r.push(InfoRow::text("Effective", "effective", format_dt(dt)));
    }
    r.push(InfoRow::print_only(
        "Extensions",
        Value::text(format_extensions(iso)),
    ));
    r.push(InfoRow::json_bool("joliet", iso.joliet));
    r.push(InfoRow::json_bool("el_torito", iso.el_torito));
    if iso.el_torito
        && let Some(id) = &iso.el_torito_id
    {
        r.push(InfoRow::print_only("Boot loader", Value::text(id.clone())));
    }
    if let Some(id) = &iso.el_torito_id {
        r.push(InfoRow::json_text("el_torito_id", id.clone()));
    }
    r
}

/// The DMG trailer rows (everything but the partition map), driving both
/// outputs. Labelled enums (`Variant`, the checksums) print a human label and
/// serialize a token; the byte-count lines print `N bytes` and serialize raw
/// numbers; `Flags` prints the decoded list and serializes the raw bitfield.
fn dmg_main_rows(dmg: &DmgMeta) -> Vec<InfoRow> {
    let mut r = vec![
        InfoRow::new(
            "UDIF version",
            "udif_version",
            Value::int_plain(dmg.udif_version as i64),
        ),
        InfoRow::new(
            "Variant",
            "variant",
            Value::labelled(variant_label(dmg.variant), variant_token(dmg.variant)),
        ),
        InfoRow::print_only(
            "Volume size",
            Value::text(format!("{} bytes", thousands_sep(dmg.total_size_bytes))),
        ),
        InfoRow::json_only("total_size_bytes", Value::size(dmg.total_size_bytes)),
        InfoRow::print_only(
            "Data fork",
            Value::text(format!("{} bytes", thousands_sep(dmg.data_fork_length))),
        ),
        InfoRow::json_only("data_fork_length", Value::size(dmg.data_fork_length)),
        InfoRow::print_only(
            "Plist",
            Value::text(plist_label(dmg.plist_present, dmg.plist_length)),
        ),
        InfoRow::json_bool("plist_present", dmg.plist_present),
        InfoRow::json_only("plist_length", Value::size(dmg.plist_length)),
        InfoRow::json_only("plist_offset", Value::size(dmg.plist_offset)),
    ];
    if dmg.segment_count > 1 {
        r.push(InfoRow::print_only(
            "Segments",
            Value::text(format!("{} of {}", dmg.segment_number, dmg.segment_count)),
        ));
    }
    r.push(InfoRow::json_int(
        "segment_number",
        dmg.segment_number as i64,
    ));
    r.push(InfoRow::json_int("segment_count", dmg.segment_count as i64));
    r.push(InfoRow::new(
        "Data checksum",
        "data_checksum_type",
        Value::labelled(
            checksum_label(dmg.data_checksum_type),
            checksum_token(dmg.data_checksum_type),
        ),
    ));
    r.push(InfoRow::new(
        "Master checksum",
        "master_checksum_type",
        Value::labelled(
            checksum_label(dmg.master_checksum_type),
            checksum_token(dmg.master_checksum_type),
        ),
    ));
    r.push(InfoRow::print_only(
        "Flags",
        Value::text(format_dmg_flags(dmg.flags)),
    ));
    r.push(InfoRow::json_int("flags", dmg.flags as i64));
    r
}

/// The DMG JSON object: trailer rows plus the flat `partitions` array (every
/// partition, filesystem and scheme alike, from the same `partition_rows`).
fn dmg_json(dmg: &DmgMeta) -> serde_json::Value {
    let partitions: Vec<serde_json::Value> = dmg
        .partitions
        .iter()
        .map(|p| serde_json::Value::Object(rows_to_json(&partition_rows(p))))
        .collect();
    let mut obj = rows_to_json(&dmg_main_rows(dmg));
    obj.insert(
        "partitions".to_string(),
        serde_json::Value::Array(partitions),
    );
    serde_json::Value::Object(obj)
}

/// One DMG partition's rows. The print rows (the filesystem detail block body)
/// carry derived human strings; the JSON-only rows carry the raw numbers and
/// arrays those strings are computed from. Drives the print filesystem block
/// (via `partition_block`) and every element of the JSON `partitions` array.
fn partition_rows(p: &DmgPartition) -> Vec<InfoRow> {
    let mut r = vec![InfoRow::text("Name", "name", p.name.clone())];
    if let Some(t) = &p.fs_type {
        let friendly = friendly_type(t);
        let val = if friendly == *t {
            t.clone()
        } else {
            format!("{friendly} ({t})")
        };
        r.push(InfoRow::print_only("Type", Value::text(val)));
    }
    r.push(InfoRow::print_only(
        "Logical size",
        Value::text(format_size_human(p.size_bytes)),
    ));
    r.push(InfoRow::print_only("Stored", Value::text(stored_desc(p))));
    r.push(InfoRow::print_only(
        "Compression",
        Value::text(compression_desc(p)),
    ));
    r.push(InfoRow::print_only("Chunks", Value::text(chunks_desc(p))));
    r.push(InfoRow::print_only(
        "Image offset",
        Value::text(offset_desc(p)),
    ));
    // The machine view carries the unformatted fields the print rows above
    // derive their human strings from.
    r.push(InfoRow::json_only(
        "start_sector",
        Value::size(p.start_sector),
    ));
    r.push(InfoRow::json_only("size_bytes", Value::size(p.size_bytes)));
    r.push(InfoRow::json_only(
        "stored_bytes",
        Value::size(p.stored_bytes),
    ));
    r.push(InfoRow::json_only(
        "compression",
        json_blob(json!(p.compression)),
    ));
    r.push(InfoRow::json_only(
        "chunk_count",
        Value::size(p.chunk_count as u64),
    ));
    r.push(InfoRow::json_only(
        "run_histogram",
        json_blob(run_histogram_json(p)),
    ));
    if let Some(t) = &p.fs_type {
        r.push(InfoRow::json_text("fs_type", t.clone()));
    }
    r
}

/// Decode the partition map. Filesystems each get a detail block; the format
/// scaffolding (MBR / GPT structures / free-space gaps) collapses into one
/// "Partition scheme" block. The `Partitions` summary row joins `main`; the
/// detail blocks go to `extra`. No-op when no partitions decoded.
fn dmg_partition_nodes(
    main: &mut Vec<InfoNode>,
    extra: &mut Vec<InfoNode>,
    parts: &[DmgPartition],
    theme: &PeekTheme,
) {
    if parts.is_empty() {
        return;
    }
    let (filesystems, scheme): (Vec<&DmgPartition>, Vec<&DmgPartition>) =
        parts.iter().partition(|p| !is_structural(p));

    let mut summary = pluralise(filesystems.len(), "filesystem");
    if !scheme.is_empty() {
        summary.push_str(&format!(", {} scheme", scheme.len()));
    }
    main.push(row(
        "Partitions",
        theme.paint_value(&format!("{} ({summary})", parts.len())),
    ));

    for p in &filesystems {
        extra.push(partition_block(p, theme));
    }
    if !scheme.is_empty() {
        extra.push(scheme_block(&scheme, theme));
    }
}

/// Full detail block for one filesystem partition — the print rows of
/// [`partition_rows`] under a friendly-typed header.
fn partition_block(p: &DmgPartition, theme: &PeekTheme) -> InfoNode {
    let title = match &p.fs_type {
        Some(t) => format!("Partition \u{b7} {}", friendly_type(t)),
        None => "Partition".to_string(),
    };
    let mut lines = Vec::new();
    push_rows(&mut lines, &partition_rows(p), theme);
    InfoNode::Block {
        title,
        body: lines.into_iter().map(InfoNode::Line).collect(),
    }
}

/// Compact block for the format scaffolding — one row per entry.
fn scheme_block(scheme: &[&DmgPartition], theme: &PeekTheme) -> InfoNode {
    let body = scheme
        .iter()
        .map(|p| {
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
            InfoNode::Row {
                label: label.into(),
                value: theme.paint_value(&value),
            }
        })
        .collect();
    InfoNode::Block {
        title: "Partition scheme".to_string(),
        body,
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

/// The partition's run-type histogram as a JSON object `{label: count}`.
fn run_histogram_json(p: &DmgPartition) -> serde_json::Value {
    let mut hist = serde_json::Map::new();
    for (label, n) in &p.run_histogram {
        hist.insert((*label).to_string(), json!(n));
    }
    serde_json::Value::Object(hist)
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

fn raw_json(raw: &RawImageMeta) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    if let Some(table) = &raw.mbr {
        let partitions: Vec<serde_json::Value> = table
            .partitions
            .iter()
            .map(|p| {
                json!({
                    "bootable": p.bootable,
                    "type_code": p.type_code,
                    "start_lba": p.start_lba,
                    "sectors": p.sectors,
                })
            })
            .collect();
        obj.insert("mbr".to_string(), json!({ "partitions": partitions }));
    }
    serde_json::Value::Object(obj)
}

fn variant_token(variant: DmgVariant) -> &'static str {
    match variant {
        DmgVariant::Device => "device",
        DmgVariant::Partition => "partition",
        DmgVariant::MountedSystem => "mounted-system",
        DmgVariant::Other(_) => "other",
    }
}

fn checksum_token(kind: DmgChecksumKind) -> &'static str {
    match kind {
        DmgChecksumKind::None => "none",
        DmgChecksumKind::Crc32 => "crc32",
        DmgChecksumKind::Md5 => "md5",
        DmgChecksumKind::Sha1 => "sha1",
        DmgChecksumKind::Sha256 => "sha256",
        DmgChecksumKind::Sha512 => "sha512",
        DmgChecksumKind::Other(_) => "other",
    }
}

/// A JSON-only cell carrying a composite (array/object) value verbatim. The
/// row has no print label, so the placeholder text is never rendered.
fn json_blob(json: serde_json::Value) -> Value {
    Value::split(String::new(), Role::Value, json)
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
        let t =
            peek_theme::load_embedded_theme(peek_theme::PeekThemeName::IdeaDark.tmtheme_source());
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
        let mut main = Vec::new();
        let mut extra = Vec::new();
        dmg_partition_nodes(&mut main, &mut extra, &[mbr, fs], &theme);
        // Render the produced nodes (summary row + detail blocks) to lines.
        struct Nodes(Vec<InfoNode>);
        impl crate::info::InfoView for Nodes {
            fn info_nodes(&self, _t: &PeekTheme) -> Vec<InfoNode> {
                self.0.clone()
            }
        }
        let mut all = main;
        all.extend(extra);
        let mut lines = Vec::new();
        let nodes = Nodes(all);
        crate::info::render_info(&mut lines, &nodes, &theme);
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

    fn sample_dmg() -> DmgMeta {
        DmgMeta {
            udif_version: 4,
            flags: 0x1,
            variant: DmgVariant::Device,
            total_size_bytes: 200 * 1024 * 1024,
            data_fork_length: 60 * 1024 * 1024,
            plist_present: true,
            plist_length: 4096,
            plist_offset: 1000,
            partitions: vec![DmgPartition {
                name: "disk image (Apple_HFS : 4)".to_string(),
                fs_type: Some("Apple_HFS".to_string()),
                start_sector: 40,
                size_bytes: 2048 * 512,
                stored_bytes: 270,
                compression: vec!["zlib"],
                chunk_count: 408,
                run_histogram: vec![("raw", 1), ("zlib", 407)],
            }],
            segment_number: 1,
            segment_count: 1,
            data_checksum_type: DmgChecksumKind::Crc32,
            master_checksum_type: DmgChecksumKind::Sha256,
        }
    }

    /// The DMG JSON object: labelled enums serialize as tokens, the byte-count
    /// print lines serialize as raw numbers, `flags` is the raw bitfield, and
    /// the human print rows (`Volume size`, `Flags`) carry no JSON key.
    #[test]
    fn dmg_json_shape() {
        let (key, value) = json_section(&DiskImageInfo {
            format_name: "DMG",
            meta: Some(DiskImageMeta::Dmg(sample_dmg())),
            error: None,
        });
        assert_eq!(key, "disk_image");
        assert_eq!(value["format"], json!("DMG"));
        let dmg = &value["dmg"];
        assert_eq!(dmg["udif_version"], json!(4));
        // Labelled enums → tokens, not the `device image` / `CRC-32` print labels.
        assert_eq!(dmg["variant"], json!("device"));
        assert_eq!(dmg["data_checksum_type"], json!("crc32"));
        assert_eq!(dmg["master_checksum_type"], json!("sha256"));
        // Byte counts as raw numbers; the `Volume size` print row is keyless.
        assert_eq!(dmg["total_size_bytes"], json!(200 * 1024 * 1024));
        assert_eq!(dmg["data_fork_length"], json!(60 * 1024 * 1024));
        assert!(
            dmg.get("Volume size").is_none(),
            "print label leaked: {dmg}"
        );
        // Flags as the raw bitfield, not the decoded `flattened` string.
        assert_eq!(dmg["flags"], json!(1));
        assert!(dmg.get("Flags").is_none(), "print label leaked: {dmg}");
        // Partition JSON-only raw fields and the run_histogram object.
        let part = &dmg["partitions"][0];
        assert_eq!(part["name"], json!("disk image (Apple_HFS : 4)"));
        assert_eq!(part["fs_type"], json!("Apple_HFS"));
        assert_eq!(part["start_sector"], json!(40));
        assert_eq!(part["size_bytes"], json!(2048 * 512));
        assert_eq!(part["stored_bytes"], json!(270));
        assert_eq!(part["chunk_count"], json!(408));
        assert_eq!(part["compression"], json!(["zlib"]));
        assert_eq!(part["run_histogram"], json!({ "raw": 1, "zlib": 407 }));
        // The derived human print rows have no JSON keys.
        assert!(part.get("Logical size").is_none(), "leak: {part}");
        assert!(part.get("Stored").is_none(), "leak: {part}");
        assert!(part.get("Chunks").is_none(), "leak: {part}");
    }

    fn sample_iso() -> IsoVolumeMeta {
        IsoVolumeMeta {
            system_id: Some("LINUX".to_string()),
            volume_label: Some("MY_DISC".to_string()),
            volume_set_id: None,
            publisher: None,
            data_preparer: None,
            application: None,
            block_size: 2048,
            block_count: 81720,
            creation: None,
            modification: None,
            expiration: None,
            effective: None,
            joliet: true,
            el_torito: false,
            el_torito_id: None,
        }
    }

    /// The ISO JSON object: the composite `Volume size` / `Extensions` print
    /// rows resolve to flat numeric / bool keys, and the print labels are absent.
    #[test]
    fn iso_json_shape() {
        let (_, value) = json_section(&DiskImageInfo {
            format_name: "ISO 9660",
            meta: Some(DiskImageMeta::Iso(sample_iso())),
            error: None,
        });
        let iso = &value["iso"];
        assert_eq!(iso["volume_label"], json!("MY_DISC"));
        assert_eq!(iso["system_id"], json!("LINUX"));
        // `Volume size` print line splits into flat block_size / block_count.
        assert_eq!(iso["block_size"], json!(2048));
        assert_eq!(iso["block_count"], json!(81720));
        assert!(
            iso.get("Volume size").is_none(),
            "print label leaked: {iso}"
        );
        // `Extensions` print line splits into the joliet / el_torito bools.
        assert_eq!(iso["joliet"], json!(true));
        assert_eq!(iso["el_torito"], json!(false));
        assert!(iso.get("Extensions").is_none(), "print label leaked: {iso}");
    }

    /// An errored image: the `error` surfaces, no variant payload is built.
    #[test]
    fn error_json_shape() {
        let (_, value) = json_section(&DiskImageInfo {
            format_name: "DMG",
            meta: None,
            error: Some("truncated trailer".to_string()),
        });
        assert_eq!(value["format"], json!("DMG"));
        assert_eq!(value["error"], json!("truncated trailer"));
        assert!(value.get("dmg").is_none());
    }
}
