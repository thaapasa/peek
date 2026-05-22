//! Classfile info section rendering. Maps `cafebabe`'s class version
//! and access flags to display labels. On a parse error only the error
//! row is shown.

use cafebabe::ClassAccessFlags;

use super::info::ClassfileInfo;
use crate::info::{push_field, push_section_header, thousands_sep};
use crate::theme::PeekTheme;

pub fn render_section(lines: &mut Vec<String>, info: &ClassfileInfo, theme: &PeekTheme) {
    lines.push(String::new());
    push_section_header(lines, "Class File", theme);

    let Some(meta) = &info.meta else {
        let msg = info.error.as_deref().unwrap_or("could not parse classfile");
        push_field(lines, "Status", &theme.paint_warning(msg), theme);
        return;
    };

    push_field(lines, "Class", &theme.paint_value(&meta.class_name), theme);
    if let Some(super_class) = &meta.super_class {
        push_field(lines, "Extends", &theme.paint_value(super_class), theme);
    }
    if !meta.interfaces.is_empty() {
        push_field(
            lines,
            "Implements",
            &theme.paint_value(&meta.interfaces.join(", ")),
            theme,
        );
    }
    push_field(
        lines,
        "Kind",
        &theme.paint_value(&kind_label(meta.access_flags)),
        theme,
    );
    push_field(
        lines,
        "Version",
        &theme.paint_value(&version_label(meta.major_version, meta.minor_version)),
        theme,
    );
    if let Some(source_file) = &meta.source_file {
        push_field(lines, "Source", &theme.paint_value(source_file), theme);
    }
    push_field(
        lines,
        "Fields",
        &theme.paint_value(&thousands_sep(meta.field_count as u64)),
        theme,
    );
    push_field(
        lines,
        "Methods",
        &theme.paint_value(&thousands_sep(meta.method_count as u64)),
        theme,
    );
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
