//! Disk-image info section rendering. ISO 9660 today; future formats
//! plug into this same section header by adding their own block here
//! and a matching arm in `gather_extras`.

use crate::info::{
    DiskImageMeta, DmgChecksumKind, DmgMeta, DmgVariant, IsoDateTime, IsoVolumeMeta, push_field,
    push_section_header, thousands_sep,
};
use crate::theme::PeekTheme;

pub fn render_section(
    lines: &mut Vec<String>,
    format_name: &str,
    meta: Option<&DiskImageMeta>,
    error: Option<&str>,
    theme: &PeekTheme,
) {
    lines.push(String::new());
    push_section_header(lines, "Disk Image", theme);
    push_field(lines, "Format", &theme.paint_value(format_name), theme);

    if let Some(err) = error {
        push_field(lines, "Status", &theme.paint_warning(err), theme);
        return;
    }

    match meta {
        Some(DiskImageMeta::Iso(iso)) => render_iso(lines, iso, theme),
        Some(DiskImageMeta::Dmg(dmg)) => render_dmg(lines, dmg, theme),
        None => {}
    }
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
}
