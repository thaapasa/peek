//! Render `cafebabe` JVM type descriptors as syntax-highlighted spans.
//!
//! cafebabe parses descriptors into structured types, but its `Display`
//! emits the raw JVM form (`I`, `[B`, `(I)V`). These helpers turn that
//! into source-like, colour-tagged text — `int`, `byte[]`,
//! `(int) -> void` — so a signature reads like a highlighted source line.

use cafebabe::descriptors::{FieldDescriptor, FieldType, MethodDescriptor, ReturnDescriptor};

use crate::viewer::table::CellRole;

/// A styled token: display text plus the table colour role it paints
/// with. A type / signature renders as a sequence of these.
pub type Span = (String, CellRole);

// Colour roles picked to mimic a Java / Rust highlighter:
//   primitive types & `void` → `Primary` (accent  — the keyword colour)
//   class types              → `Tag`     (label   — the entity-name colour)
//   array `[]`               → `Numeric` (accent/value blend, distinct)
//   punctuation `( ) , ->`   → `Muted`
const PRIMITIVE: CellRole = CellRole::Primary;
const CLASS: CellRole = CellRole::Tag;
const ARRAY: CellRole = CellRole::Numeric;
const PUNCT: CellRole = CellRole::Muted;

/// A field / parameter / return type — `int`, `String`, `byte[][]`.
pub fn field(d: &FieldDescriptor<'_>) -> Vec<Span> {
    let mut spans = vec![base_type(&d.field_type)];
    if d.dimensions > 0 {
        spans.push(("[]".repeat(d.dimensions as usize), ARRAY));
    }
    spans
}

/// A method signature — `(int, String) -> void`.
pub fn method(d: &MethodDescriptor<'_>) -> Vec<Span> {
    let mut spans: Vec<Span> = vec![("(".to_string(), PUNCT)];
    for (i, p) in d.parameters.iter().enumerate() {
        if i > 0 {
            spans.push((", ".to_string(), PUNCT));
        }
        spans.extend(field(p));
    }
    spans.push((")".to_string(), PUNCT));
    spans.push((" -> ".to_string(), PUNCT));
    spans.extend(return_type(&d.return_type));
    spans
}

fn return_type(r: &ReturnDescriptor<'_>) -> Vec<Span> {
    match r {
        ReturnDescriptor::Void => vec![("void".to_string(), PRIMITIVE)],
        ReturnDescriptor::Return(d) => field(d),
    }
}

/// The base (non-array) type as a single span.
fn base_type(t: &FieldType<'_>) -> Span {
    let primitive = match t {
        FieldType::Byte => "byte",
        FieldType::Char => "char",
        FieldType::Double => "double",
        FieldType::Float => "float",
        FieldType::Integer => "int",
        FieldType::Long => "long",
        FieldType::Short => "short",
        FieldType::Boolean => "boolean",
        // `ClassName` derefs to the fully-qualified internal name
        // (`java/lang/String`); show only the simple last segment.
        FieldType::Object(class_name) => {
            return (
                class_name.rsplit('/').next().unwrap_or("?").to_string(),
                CLASS,
            );
        }
    };
    (primitive.to_string(), PRIMITIVE)
}
