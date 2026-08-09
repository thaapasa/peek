//! The Java classfile info section, driven by one [`ClassfileView`] that
//! derives both `serde::Serialize` (JSON) and
//! [`InfoView`](crate::info::InfoView) (themed print). [`ClassfileInfo`] stays
//! the gather struct; the view projects it.
//!
//! Two fields diverge between the outputs and so carry custom impls: the class
//! `Kind` prints as a Java-declaration phrase (`public final class`) but JSON
//! flattens it to a `kind` token plus boolean modifier flags; `Version` prints
//! as a JDK label but JSON flattens it to raw `major`/`minor` numbers. On a
//! parse error only the `Status` row shows (JSON: an `error` key).

use cafebabe::ClassAccessFlags;
use peek_theme::PeekTheme;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use super::info::ClassfileInfo;
use crate::info::{InfoValue, Value, Warn};

crate::info_section!(ClassfileInfo, ClassfileView, "classfile");

#[derive(Serialize, crate::info::InfoView)]
#[info(title = "Class File")]
struct ClassfileView {
    // Parse failure: print shows a warning Status row; JSON an `error` key.
    #[info(label = "Status", skip_if = "Option::is_none")]
    #[serde(skip)]
    status: Option<Warn>,
    #[info(skip)]
    #[serde(rename = "error", skip_serializing_if = "Option::is_none")]
    error: Option<String>,

    #[info(label = "Class")]
    #[serde(rename = "class_name", skip_serializing_if = "Option::is_none")]
    class_name: Option<String>,
    #[info(label = "Extends")]
    #[serde(rename = "super_class", skip_serializing_if = "Option::is_none")]
    super_class: Option<String>,
    #[info(label = "Implements")]
    #[serde(rename = "interfaces", skip_serializing_if = "Option::is_none")]
    interfaces: Option<NameList>,
    // Print: the declaration phrase. JSON: flattened kind token + bool flags.
    #[info(label = "Kind", skip_if = "Option::is_none")]
    #[serde(flatten)]
    kind: Option<ClassKind>,
    // Print: the JDK label. JSON: flattened major/minor numbers.
    #[info(label = "Version", skip_if = "Option::is_none")]
    #[serde(flatten)]
    version: Option<ClassVersion>,
    #[info(label = "Source")]
    #[serde(rename = "source_file", skip_serializing_if = "Option::is_none")]
    source_file: Option<String>,
    #[info(label = "Fields")]
    #[serde(rename = "field_count", skip_serializing_if = "Option::is_none")]
    field_count: Option<Value>,
    #[info(label = "Methods")]
    #[serde(rename = "method_count", skip_serializing_if = "Option::is_none")]
    method_count: Option<Value>,
}

impl From<&ClassfileInfo> for ClassfileView {
    fn from(info: &ClassfileInfo) -> Self {
        let Some(meta) = &info.meta else {
            return ClassfileView {
                status: Some(Warn(
                    info.error
                        .clone()
                        .unwrap_or_else(|| "could not parse classfile".to_string()),
                )),
                error: info.error.clone(),
                class_name: None,
                super_class: None,
                interfaces: None,
                kind: None,
                version: None,
                source_file: None,
                field_count: None,
                method_count: None,
            };
        };
        ClassfileView {
            status: None,
            error: None,
            class_name: Some(meta.class_name.clone()),
            super_class: meta.super_class.clone(),
            interfaces: (!meta.interfaces.is_empty()).then(|| NameList(meta.interfaces.clone())),
            kind: Some(ClassKind(meta.access_flags)),
            version: Some(ClassVersion {
                major: meta.major_version,
                minor: meta.minor_version,
            }),
            source_file: meta.source_file.clone(),
            // Counts use plain value colour + thousands separators (Value::Int).
            field_count: Some(Value::int(meta.field_count as i64)),
            method_count: Some(Value::int(meta.method_count as i64)),
        }
    }
}

/// Implemented interfaces: a JSON array, but a comma-joined value-coloured
/// print row.
struct NameList(Vec<String>);

impl InfoValue for NameList {
    fn render_value(&self, theme: &PeekTheme) -> String {
        theme.paint_value(&self.0.join(", "))
    }
}

impl Serialize for NameList {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(ser)
    }
}

/// Class kind + access flags. Print: a Java-declaration phrase. JSON: a `kind`
/// token plus `is_*` booleans (flattened into the parent object).
struct ClassKind(ClassAccessFlags);

impl InfoValue for ClassKind {
    fn render_value(&self, theme: &PeekTheme) -> String {
        theme.paint_value(&kind_label(self.0))
    }
}

impl Serialize for ClassKind {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let f = self.0;
        let mut st = ser.serialize_struct("kind", 5)?;
        st.serialize_field("kind", class_kind_token(f))?;
        st.serialize_field("is_public", &f.contains(ClassAccessFlags::PUBLIC))?;
        st.serialize_field("is_final", &f.contains(ClassAccessFlags::FINAL))?;
        st.serialize_field("is_abstract", &f.contains(ClassAccessFlags::ABSTRACT))?;
        st.serialize_field("is_synthetic", &f.contains(ClassAccessFlags::SYNTHETIC))?;
        st.end()
    }
}

/// Classfile version. Print: a JDK label. JSON: raw major/minor numbers
/// (flattened into the parent object).
struct ClassVersion {
    major: u16,
    minor: u16,
}

impl InfoValue for ClassVersion {
    fn render_value(&self, theme: &PeekTheme) -> String {
        theme.paint_value(&version_label(self.major, self.minor))
    }
}

impl Serialize for ClassVersion {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut st = ser.serialize_struct("version", 2)?;
        st.serialize_field("major_version", &self.major)?;
        st.serialize_field("minor_version", &self.minor)?;
        st.end()
    }
}

/// Stable lowercase machine token for the class kind, mirroring the
/// declaration noun `kind_label` chooses.
fn class_kind_token(f: ClassAccessFlags) -> &'static str {
    if f.contains(ClassAccessFlags::ANNOTATION) {
        "annotation"
    } else if f.contains(ClassAccessFlags::INTERFACE) {
        "interface"
    } else if f.contains(ClassAccessFlags::ENUM) {
        "enum"
    } else {
        "class"
    }
}

/// Modifiers + class kind as a Java-declaration-like phrase —
/// `public final class`, `public interface`, `public enum`.
fn kind_label(f: ClassAccessFlags) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if f.contains(ClassAccessFlags::PUBLIC) {
        parts.push("public");
    }
    if f.contains(ClassAccessFlags::FINAL) {
        parts.push("final");
    }
    // `abstract` is implied by `interface`; only show it for classes.
    if f.contains(ClassAccessFlags::ABSTRACT) && !f.contains(ClassAccessFlags::INTERFACE) {
        parts.push("abstract");
    }
    parts.push(if f.contains(ClassAccessFlags::ANNOTATION) {
        "@interface"
    } else if f.contains(ClassAccessFlags::INTERFACE) {
        "interface"
    } else if f.contains(ClassAccessFlags::ENUM) {
        "enum"
    } else {
        "class"
    });
    if f.contains(ClassAccessFlags::SYNTHETIC) {
        parts.push("(synthetic)");
    }
    parts.join(" ")
}

/// `major.minor` → JDK release label. JVM major 49+ maps linearly:
/// 52 = Java 8, 61 = Java 17, 65 = Java 21. 45–48 predate the
/// single-number naming.
fn version_label(major: u16, minor: u16) -> String {
    let jdk = match major {
        45 => "Java 1.1".to_string(),
        46 => "Java 1.2".to_string(),
        47 => "Java 1.3".to_string(),
        48 => "Java 1.4".to_string(),
        m if m >= 49 => format!("Java {}", m - 44),
        _ => "pre-1.1".to_string(),
    };
    format!("{jdk}  (classfile {major}.{minor})")
}
