//! The object-file info section, driven by one [`ObjectView`] that derives
//! both `serde::Serialize` (JSON) and [`InfoView`](crate::info::InfoView)
//! (themed print). [`ObjectInfo`] stays the gather struct; the view projects
//! it. On a parse error only the `Status` row shows (JSON: an `error` key).
//!
//! The `object`-crate enums are foreign; the scalar fields project each to a
//! [`Value::split`] (print label + JSON token). Only the composite fields
//! (`Symbols`, `Universal`, `BuildId`) — whose JSON is an object — keep a
//! typed `Serialize` struct, so editing print can't desync their shape.

use object::{Architecture, BinaryFormat, Endianness, ObjectKind};
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::json;

use super::info::{BuildIdKind, ObjectInfo};
use crate::info::{InfoNode, InfoValue, Role, Value, Warn, render_info, thousands_sep};
use crate::theme::PeekTheme;

/// Themed terminal object-file section.
pub fn render_section(lines: &mut Vec<String>, info: &ObjectInfo, theme: &PeekTheme) {
    render_info(lines, &ObjectView::from(info), theme);
}

/// Typed `--info --json` view of the Object File section, nested under
/// `"objfile"`.
pub fn json_section(info: &ObjectInfo) -> (&'static str, serde_json::Value) {
    (
        "objfile",
        serde_json::to_value(ObjectView::from(info)).expect("objfile info view serializes"),
    )
}

#[derive(Serialize, crate::info::InfoView)]
#[info(title = "Object File")]
struct ObjectView {
    #[info(label = "Status", skip_if = "Option::is_none")]
    #[serde(skip)]
    status: Option<Warn>,
    #[info(skip)]
    #[serde(rename = "error", skip_serializing_if = "Option::is_none")]
    error: Option<String>,

    #[info(label = "Format")]
    #[serde(rename = "format", skip_serializing_if = "Option::is_none")]
    format: Option<Value>,
    #[info(label = "Architecture")]
    #[serde(rename = "architecture", skip_serializing_if = "Option::is_none")]
    architecture: Option<Value>,
    #[info(label = "Universal", skip_if = "Option::is_none")]
    #[serde(flatten)]
    universal: Option<Universal>,
    #[info(label = "Type")]
    #[serde(rename = "kind", skip_serializing_if = "Option::is_none")]
    kind: Option<Value>,
    #[info(label = "Class")]
    #[serde(rename = "is_64", skip_serializing_if = "Option::is_none")]
    class: Option<Value>,
    #[info(label = "Endianness")]
    #[serde(rename = "endianness", skip_serializing_if = "Option::is_none")]
    endianness: Option<Value>,
    #[info(label = "Entry point")]
    #[serde(rename = "entry", skip_serializing_if = "Option::is_none")]
    entry: Option<Value>,
    #[info(label = "Sections")]
    #[serde(rename = "section_count", skip_serializing_if = "Option::is_none")]
    sections: Option<Value>,
    #[info(label = "Symbols", skip_if = "Option::is_none")]
    #[serde(flatten)]
    symbols: Option<Symbols>,
    #[info(label = "Debug info")]
    #[serde(rename = "has_debug_info", skip_serializing_if = "Option::is_none")]
    debug: Option<Value>,
    #[info(nest)]
    #[serde(rename = "build_id", skip_serializing_if = "Option::is_none")]
    build_id: Option<BuildId>,
    #[info(label = "Linked libs", skip_if = "Option::is_none")]
    #[serde(rename = "linked_libraries", skip_serializing_if = "Option::is_none")]
    linked: Option<Value>,
}

impl From<&ObjectInfo> for ObjectView {
    fn from(info: &ObjectInfo) -> Self {
        let Some(meta) = &info.meta else {
            return ObjectView {
                status: Some(Warn(
                    info.error
                        .clone()
                        .unwrap_or_else(|| "could not parse object file".to_string()),
                )),
                error: info.error.clone(),
                format: None,
                architecture: None,
                universal: None,
                kind: None,
                class: None,
                endianness: None,
                entry: None,
                sections: None,
                symbols: None,
                debug: None,
                build_id: None,
                linked: None,
            };
        };
        ObjectView {
            status: None,
            error: None,
            format: Some(Value::labelled(
                format_label(meta.format),
                format_token(meta.format),
            )),
            architecture: Some(Value::labelled(
                arch_label(meta.architecture),
                arch_token(meta.architecture),
            )),
            universal: (!meta.universal.is_empty()).then(|| Universal {
                archs: meta.universal.clone(),
                selected: meta.universal_selected,
            }),
            kind: Some(Value::labelled(
                kind_label(meta.kind),
                kind_token(meta.kind),
            )),
            class: Some(Value::split(
                if meta.is_64 { "64-bit" } else { "32-bit" },
                Role::Value,
                json!(meta.is_64),
            )),
            endianness: Some(Value::labelled(
                endianness_label(meta.endianness),
                endianness_token(meta.endianness),
            )),
            entry: meta
                .entry
                .map(|e| Value::split(format!("0x{e:x}"), Role::Value, json!(e))),
            sections: Some(Value::int(meta.section_count as i64)),
            symbols: Some(Symbols {
                symbols: meta.symbol_count,
                dynamic: meta.dynamic_symbol_count,
            }),
            debug: Some(Value::split(
                if meta.has_debug_info {
                    "present"
                } else {
                    "none"
                },
                Role::Value,
                json!(meta.has_debug_info),
            )),
            build_id: meta.build_id.as_ref().map(|(kind, bytes)| BuildId {
                kind: build_id_token(kind),
                label: build_id_label(kind),
                value: if matches!(kind, BuildIdKind::GnuBuildId) {
                    hex(bytes)
                } else {
                    uuid(bytes)
                },
                json_value: hex(bytes),
            }),
            linked: (!meta.linked_libraries.is_empty()).then(|| {
                Value::split(
                    meta.linked_libraries.join(", "),
                    Role::Value,
                    json!(meta.linked_libraries),
                )
            }),
        }
    }
}

// --- composite fields: typed `Serialize` structs (JSON is an object) -------

/// Symbol tally. Print: `N (+M dynamic)` / `none (stripped)`. JSON:
/// `symbol_count` + `dynamic_symbol_count`.
struct Symbols {
    symbols: usize,
    dynamic: usize,
}
impl InfoValue for Symbols {
    fn render_value(&self, theme: &PeekTheme) -> String {
        let text = if self.symbols == 0 && self.dynamic == 0 {
            "none (stripped)".to_string()
        } else {
            let mut s = thousands_sep(self.symbols as u64);
            if self.dynamic > 0 {
                s.push_str(&format!(
                    " (+{} dynamic)",
                    thousands_sep(self.dynamic as u64)
                ));
            }
            s
        };
        theme.paint_value(&text)
    }
}
impl Serialize for Symbols {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut st = ser.serialize_struct("symbols", 2)?;
        st.serialize_field("symbol_count", &self.symbols)?;
        st.serialize_field("dynamic_symbol_count", &self.dynamic)?;
        st.end()
    }
}

/// Universal (fat) Mach-O slices. Print: `a, b (showing a)`. JSON:
/// `universal` (token array) + `universal_selected`.
struct Universal {
    archs: Vec<Architecture>,
    selected: usize,
}
impl InfoValue for Universal {
    fn render_value(&self, theme: &PeekTheme) -> String {
        let list = self
            .archs
            .iter()
            .map(|a| arch_label(*a))
            .collect::<Vec<_>>()
            .join(", ");
        let selected = self
            .archs
            .get(self.selected)
            .map(|a| arch_label(*a))
            .unwrap_or_else(|| "?".to_string());
        theme.paint_value(&format!("{list} (showing {selected})"))
    }
}
impl Serialize for Universal {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let tokens: Vec<String> = self.archs.iter().map(|a| arch_token(*a)).collect();
        let mut st = ser.serialize_struct("universal", 2)?;
        st.serialize_field("universal", &tokens)?;
        st.serialize_field("universal_selected", &self.selected)?;
        st.end()
    }
}

/// Build-identity blob. Print: one row whose label and value depend on the
/// kind. JSON: `{ kind, value }` under `build_id`.
struct BuildId {
    kind: &'static str,
    label: &'static str,
    value: String,
    json_value: String,
}
impl crate::info::InfoView for BuildId {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        vec![InfoNode::Row {
            label: self.label.into(),
            value: theme.paint_value(&self.value),
        }]
    }
}
impl Serialize for BuildId {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut st = ser.serialize_struct("build_id", 2)?;
        st.serialize_field("kind", self.kind)?;
        st.serialize_field("value", &self.json_value)?;
        st.end()
    }
}

/// Continuous lowercase hex — for variable-length build IDs.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Canonical 8-4-4-4-12 UUID form. Falls back to plain hex if the blob isn't
/// 16 bytes.
fn uuid(bytes: &[u8]) -> String {
    if bytes.len() != 16 {
        return hex(bytes);
    }
    let h = hex(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

fn build_id_label(kind: &BuildIdKind) -> &'static str {
    match kind {
        BuildIdKind::GnuBuildId => "Build ID",
        BuildIdKind::MachUuid => "UUID",
        BuildIdKind::PdbGuid => "PDB GUID",
    }
}

fn format_label(f: BinaryFormat) -> &'static str {
    match f {
        BinaryFormat::Coff => "COFF",
        BinaryFormat::Elf => "ELF",
        BinaryFormat::MachO => "Mach-O",
        BinaryFormat::Pe => "PE",
        BinaryFormat::Wasm => "WebAssembly",
        BinaryFormat::Xcoff => "XCOFF",
        _ => "unknown",
    }
}

fn kind_label(k: ObjectKind) -> &'static str {
    match k {
        ObjectKind::Relocatable => "relocatable object",
        ObjectKind::Executable => "executable",
        ObjectKind::Dynamic => "dynamic library",
        ObjectKind::Core => "core dump",
        _ => "unknown",
    }
}

fn endianness_label(e: Endianness) -> &'static str {
    match e {
        Endianness::Little => "little-endian",
        Endianness::Big => "big-endian",
    }
}

fn format_token(f: BinaryFormat) -> &'static str {
    match f {
        BinaryFormat::Coff => "coff",
        BinaryFormat::Elf => "elf",
        BinaryFormat::MachO => "macho",
        BinaryFormat::Pe => "pe",
        BinaryFormat::Wasm => "wasm",
        BinaryFormat::Xcoff => "xcoff",
        _ => "unknown",
    }
}

fn kind_token(k: ObjectKind) -> &'static str {
    match k {
        ObjectKind::Relocatable => "relocatable",
        ObjectKind::Executable => "executable",
        ObjectKind::Dynamic => "dynamic",
        ObjectKind::Core => "core",
        _ => "unknown",
    }
}

fn endianness_token(e: Endianness) -> &'static str {
    match e {
        Endianness::Little => "little",
        Endianness::Big => "big",
    }
}

fn build_id_token(kind: &BuildIdKind) -> &'static str {
    match kind {
        BuildIdKind::GnuBuildId => "gnu-build-id",
        BuildIdKind::MachUuid => "mach-uuid",
        BuildIdKind::PdbGuid => "pdb-guid",
    }
}

fn arch_token(a: Architecture) -> String {
    match a {
        Architecture::X86_64 => "x86-64".to_string(),
        Architecture::I386 => "i386".to_string(),
        Architecture::Aarch64 => "aarch64".to_string(),
        Architecture::Arm => "arm".to_string(),
        Architecture::Wasm32 => "wasm32".to_string(),
        Architecture::Wasm64 => "wasm64".to_string(),
        Architecture::Unknown => "unknown".to_string(),
        other => format!("{other:?}").to_lowercase(),
    }
}

/// Friendly label for the common architectures; anything else falls back to
/// the `object` enum's debug name.
pub(crate) fn arch_label(a: Architecture) -> String {
    match a {
        Architecture::X86_64 => "x86-64".to_string(),
        Architecture::I386 => "x86 (i386)".to_string(),
        Architecture::Aarch64 => "AArch64".to_string(),
        Architecture::Arm => "ARM".to_string(),
        Architecture::Wasm32 => "WebAssembly (32-bit)".to_string(),
        Architecture::Wasm64 => "WebAssembly (64-bit)".to_string(),
        Architecture::Unknown => "unknown".to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_continuous_lowercase() {
        assert_eq!(hex(&[0x0a, 0xff, 0x00]), "0aff00");
    }

    #[test]
    fn uuid_uses_canonical_grouping() {
        let bytes: Vec<u8> = (0u8..16).collect();
        assert_eq!(uuid(&bytes), "00010203-0405-0607-0809-0a0b0c0d0e0f");
    }

    #[test]
    fn uuid_falls_back_to_hex_when_not_16_bytes() {
        assert_eq!(uuid(&[0xde, 0xad]), "dead");
    }
}
