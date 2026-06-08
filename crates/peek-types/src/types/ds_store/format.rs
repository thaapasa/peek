//! Human-facing rendering of `.DS_Store` records: the friendly name for
//! each four-character structure id, and a decoded display string for
//! each value. Pure presentation — the byte parsing lives in
//! [`super::reader`].

use super::reader::{DsRecord, DsValue};

/// Friendly label for a structure id, or `None` for codes we don't name
/// (the caller falls back to the raw four-character code). Covers the
/// common Finder settings; the long tail stays raw.
pub fn property_label(code: &str) -> Option<&'static str> {
    Some(match code {
        "Iloc" => "Icon location",
        "fwi0" => "Window frame",
        "fwsw" => "Sidebar width",
        "fwvh" => "Window height",
        "vstl" => "View style",
        "vSrn" => "View options",
        "icvo" | "ICVO" => "Icon view options",
        "lsvo" | "LSVO" => "List view options",
        "icvp" => "Icon view settings",
        "icvt" => "Icon text size",
        "lsvp" | "lsvP" => "List view settings",
        "clvp" => "Column view settings",
        "bwsp" => "Window settings",
        "BKGD" => "Background",
        "pBBk" => "Background image",
        "pBB0" => "Background image id",
        "dscl" => "Open in list view",
        "modD" | "moDD" => "Date modified",
        "phys" | "ph1S" => "Physical size",
        "logS" | "lg1S" => "Logical size",
        "cmmt" => "Spotlight comment",
        "GRP0" => "Group",
        "extn" => "Extension",
        "info" => "Finder info",
        "dilc" => "Desktop icon location",
        "clip" => "Clipping",
        "icgo" => "Icon grid options",
        "icsp" => "Icon scroll position",
        "ptbL" | "ptbN" => "Trash put-back",
        _ => return None,
    })
}

/// Decode a record's value to a display string, applying code-specific
/// structure where the raw type tag alone (`blob`, `type`, …) doesn't
/// say enough.
pub fn format_value(record: &DsRecord) -> String {
    match (&record.value, record.code.as_str()) {
        (DsValue::Blob(b), "Iloc" | "dilc") => format_iloc(b),
        (DsValue::Blob(b), "fwi0") => format_fwi0(b),
        (DsValue::Blob(b), "BKGD") => format_bkgd(b),
        (DsValue::Blob(b), "modD" | "moDD") => format_date_blob(b),
        (DsValue::Type(t), "vstl") => view_style_label(t).to_string(),
        (DsValue::Type(t), _) => t.clone(),
        (DsValue::Blob(b), _) => format_blob(b),
        (DsValue::Int(n), _) => n.to_string(),
        (DsValue::Long(n), _) => n.to_string(),
        (DsValue::Date(n), _) => format!("{n} (raw)"),
        (DsValue::Bool(v), _) => if *v { "yes" } else { "no" }.to_string(),
        (DsValue::Str(s), _) => s.clone(),
    }
}

/// Icon location blob: two big-endian u32 coordinates, `0xFFFFFFFF` for
/// "let the Finder place it".
fn format_iloc(b: &[u8]) -> String {
    if b.len() < 8 {
        return format_blob(b);
    }
    let x = be_u32(&b[0..4]);
    let y = be_u32(&b[4..8]);
    if x == 0xFFFF_FFFF && y == 0xFFFF_FFFF {
        "auto".to_string()
    } else {
        format!("({x}, {y})")
    }
}

/// Finder window frame: four signed 16-bit edges (top, left, bottom,
/// right) followed by the view-style four-character code.
fn format_fwi0(b: &[u8]) -> String {
    if b.len() < 12 {
        return format_blob(b);
    }
    let top = be_i16(&b[0..2]);
    let left = be_i16(&b[2..4]);
    let bottom = be_i16(&b[4..6]);
    let right = be_i16(&b[6..8]);
    let view = ascii4(&b[8..12]);
    format!(
        "({left}, {top}) – ({right}, {bottom}), {}",
        view_style_label(&view)
    )
}

/// Background blob: a four-character kind tag, then kind-specific data.
fn format_bkgd(b: &[u8]) -> String {
    if b.len() < 4 {
        return format_blob(b);
    }
    match &b[0..4] {
        b"DefB" => "default".to_string(),
        b"ClrB" if b.len() >= 10 => {
            // Three 16-bit channels, full-scale 0xFFFF → 0xFF.
            let r = (be_u16(&b[4..6]) / 257) as u8;
            let g = (be_u16(&b[6..8]) / 257) as u8;
            let bl = (be_u16(&b[8..10]) / 257) as u8;
            format!("color #{r:02X}{g:02X}{bl:02X}")
        }
        b"ClrB" => "color".to_string(),
        b"PctB" => "picture".to_string(),
        other => ascii4(other),
    }
}

/// Modification-date blob: an 8-byte little-endian IEEE-754 double
/// holding a `CFAbsoluteTime` (seconds since 2001-01-01 UTC). Renders as
/// a UTC date; falls back to a byte count if the value is out of range.
fn format_date_blob(b: &[u8]) -> String {
    if b.len() < 8 {
        return format_blob(b);
    }
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[0..8]);
    // CFAbsoluteTime epoch (2001-01-01) → Unix epoch (1970-01-01).
    let unix = f64::from_le_bytes(a) + 978_307_200.0;
    // Guard against NaN / negative / absurdly-far-future values before
    // casting — keep only plausible timestamps (through ~year 2200).
    if unix.is_finite() && unix > 0.0 && unix < 7_300_000_000.0 {
        crate::info::format_archive_mtime_zoned(unix as u64, true)
    } else {
        format_blob(b)
    }
}

/// Opaque blob: report length, flagging an embedded binary plist (the
/// usual payload of `bwsp` / `lsvp` / `icvp`).
fn format_blob(b: &[u8]) -> String {
    if b.len() >= 6 && &b[0..6] == b"bplist" {
        format!("binary plist, {} bytes", b.len())
    } else {
        format!("{} bytes", b.len())
    }
}

/// Map a Finder view-style four-character code to its menu name.
pub fn view_style_label(code: &str) -> &str {
    match code {
        "icnv" => "Icon view",
        "clmv" => "Column view",
        "Nlsv" => "List view",
        "glyv" => "Gallery view",
        "Flwv" => "Cover Flow",
        other => other,
    }
}

fn ascii4(b: &[u8]) -> String {
    b.iter().take(4).map(|&c| c as char).collect()
}

fn be_u32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn be_u16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

fn be_i16(b: &[u8]) -> i16 {
    i16::from_be_bytes([b[0], b[1]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ds_store::reader::{DsRecord, DsValue};

    fn rec(code: &str, value: DsValue) -> DsRecord {
        DsRecord {
            name: "x".to_string(),
            code: code.to_string(),
            value,
        }
    }

    #[test]
    fn iloc_decodes_coordinates() {
        let mut blob = vec![0u8; 16];
        blob[0..4].copy_from_slice(&175u32.to_be_bytes());
        blob[4..8].copy_from_slice(&46u32.to_be_bytes());
        assert_eq!(format_value(&rec("Iloc", DsValue::Blob(blob))), "(175, 46)");
    }

    #[test]
    fn iloc_all_ones_is_auto() {
        let blob = vec![0xFFu8; 16];
        assert_eq!(format_value(&rec("Iloc", DsValue::Blob(blob))), "auto");
    }

    #[test]
    fn vstl_maps_to_view_name() {
        assert_eq!(
            format_value(&rec("vstl", DsValue::Type("Nlsv".to_string()))),
            "List view"
        );
    }

    #[test]
    fn bkgd_color_renders_hex() {
        let mut blob = vec![0u8; 12];
        blob[0..4].copy_from_slice(b"ClrB");
        blob[4..6].copy_from_slice(&0xFFFFu16.to_be_bytes()); // R = 255
        blob[6..8].copy_from_slice(&0u16.to_be_bytes()); // G = 0
        blob[8..10].copy_from_slice(&0xFFFFu16.to_be_bytes()); // B = 255
        assert_eq!(
            format_value(&rec("BKGD", DsValue::Blob(blob))),
            "color #FF00FF"
        );
    }

    #[test]
    fn modd_decodes_cfabsolutetime_to_utc_date() {
        // CFAbsoluteTime for 2026-01-23 06:15:25 UTC, stored little-endian
        // (bytes dc a2 f8 be a4 91 c7 41, as seen on disk).
        let blob = 0xdca2f8bea491c741u64.to_be_bytes().to_vec();
        assert_eq!(
            format_value(&rec("moDD", DsValue::Blob(blob))),
            "2026-01-23 06:15 Z"
        );
    }

    #[test]
    fn modd_garbage_falls_back_to_byte_count() {
        // All-0xFF is NaN as a double → not a plausible date.
        let blob = vec![0xFFu8; 8];
        assert_eq!(format_value(&rec("moDD", DsValue::Blob(blob))), "8 bytes");
    }

    #[test]
    fn unknown_blob_reports_length_and_plist() {
        let plist = [b"bplist00".as_slice(), &[0u8; 20]].concat();
        assert_eq!(
            format_value(&rec("bwsp", DsValue::Blob(plist))),
            "binary plist, 28 bytes"
        );
        assert_eq!(
            format_value(&rec("zzzz", DsValue::Blob(vec![0u8; 4]))),
            "4 bytes"
        );
    }
}
