//! Render `cafebabe` JVM type descriptors human-readably.
//!
//! cafebabe parses descriptors into structured types, but its `Display`
//! emits the raw JVM form (`I`, `[B`, `(I)V`). These helpers turn that
//! into source-like text — `int`, `byte[]`, `(int) -> void`.

use cafebabe::descriptors::{FieldDescriptor, FieldType, MethodDescriptor, ReturnDescriptor};

/// A field / parameter / return type — `int`, `String`, `byte[][]`.
pub fn field(d: &FieldDescriptor<'_>) -> String {
    let mut s = base_type(&d.field_type);
    for _ in 0..d.dimensions {
        s.push_str("[]");
    }
    s
}

/// A method signature — `(int, String) -> void`.
pub fn method(d: &MethodDescriptor<'_>) -> String {
    let params: Vec<String> = d.parameters.iter().map(field).collect();
    format!("({}) -> {}", params.join(", "), return_type(&d.return_type))
}

fn return_type(r: &ReturnDescriptor<'_>) -> String {
    match r {
        ReturnDescriptor::Void => "void".to_string(),
        ReturnDescriptor::Return(d) => field(d),
    }
}

fn base_type(t: &FieldType<'_>) -> String {
    match t {
        FieldType::Byte => "byte".to_string(),
        FieldType::Char => "char".to_string(),
        FieldType::Double => "double".to_string(),
        FieldType::Float => "float".to_string(),
        FieldType::Integer => "int".to_string(),
        FieldType::Long => "long".to_string(),
        FieldType::Short => "short".to_string(),
        FieldType::Boolean => "boolean".to_string(),
        // `ClassName` derefs to the fully-qualified internal name
        // (`java/lang/String`); show only the simple last segment.
        FieldType::Object(class_name) => class_name.rsplit('/').next().unwrap_or("?").to_string(),
    }
}
