//! Classfile info gathering: parse the header via `cafebabe` and
//! capture it into [`ClassfileMeta`]. Parse failures land in
//! `ClassfileInfo::err` so the Info view always renders.

use cafebabe::attributes::AttributeData;
use cafebabe::{ParseOptions, parse_class_with_options};

use super::info::{ClassfileInfo, ClassfileMeta};
use crate::info::Extras;
use crate::input::InputSource;

pub fn gather_extras(source: &InputSource) -> Extras {
    Box::new(gather(source))
}

fn gather(source: &InputSource) -> ClassfileInfo {
    let bytes = match source.read_bytes(crate::input::limits::Budget::Sidecar("class file")) {
        Ok(b) => b,
        Err(e) => return ClassfileInfo::err(format!("read failed: {e}")),
    };
    // v1 needs only the header and member tables — skip bytecode
    // parsing (faster, and one less thing that can fail).
    let mut opts = ParseOptions::default();
    opts.parse_bytecode(false);
    let class = match parse_class_with_options(&bytes, &opts) {
        Ok(c) => c,
        Err(e) => return ClassfileInfo::err(format!("not a valid classfile: {e}")),
    };
    let source_file = class.attributes.iter().find_map(|a| match &a.data {
        AttributeData::SourceFile(name) => Some(name.to_string()),
        _ => None,
    });
    ClassfileInfo::ok(ClassfileMeta {
        class_name: dotted(&class.this_class),
        super_class: class.super_class.as_deref().map(dotted),
        interfaces: class.interfaces.iter().map(|i| dotted(i)).collect(),
        major_version: class.major_version,
        minor_version: class.minor_version,
        access_flags: class.access_flags,
        source_file,
        field_count: class.fields.len(),
        method_count: class.methods.len(),
    })
}

/// JVM internal name (`java/lang/String`) → dotted (`java.lang.String`).
fn dotted(internal: &str) -> String {
    internal.replace('/', ".")
}
