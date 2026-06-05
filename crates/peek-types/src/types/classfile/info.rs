//! Java classfile info shape: parsed header metadata, or a parse-error
//! surface. Mirrors `ObjectInfo` — `meta` is `Some` exactly when
//! `error` is `None`. Metadata keeps `cafebabe`'s semantic access-flag
//! type; `info_render` maps it to display labels.

use cafebabe::ClassAccessFlags;

/// Classfile metadata, or the reason parsing failed.
pub struct ClassfileInfo {
    /// Header metadata. `None` when parsing failed.
    pub meta: Option<ClassfileMeta>,
    /// User-facing parse-failure reason. `None` on success.
    pub error: Option<String>,
}

impl ClassfileInfo {
    pub fn ok(meta: ClassfileMeta) -> Self {
        Self {
            meta: Some(meta),
            error: None,
        }
    }

    pub fn err(msg: String) -> Self {
        Self {
            meta: None,
            error: Some(msg),
        }
    }
}

/// Header-level metadata for one parsed classfile.
pub struct ClassfileMeta {
    /// Fully-qualified class name, dotted (`com.example.Foo`).
    pub class_name: String,
    /// Superclass, dotted. `None` only for `java.lang.Object` itself.
    pub super_class: Option<String>,
    /// Implemented interfaces, dotted.
    pub interfaces: Vec<String>,
    pub major_version: u16,
    pub minor_version: u16,
    pub access_flags: ClassAccessFlags,
    /// The `SourceFile` attribute, when present.
    pub source_file: Option<String>,
    pub field_count: usize,
    pub method_count: usize,
}
