//! Manual info section: version, chapter count, embedded size.

use peek_foundation::info::{Extras, Value};

#[derive(serde::Serialize, peek_foundation::info::InfoView)]
#[info(title = "Manual")]
pub struct ManualInfo {
    /// The manual is built from the same commit as the binary.
    #[info(label = "Version")]
    pub version: Value,
    #[info(label = "Chapters")]
    pub chapters: Value,
    /// Total size of the preprocessed chapters.
    #[info(label = "Text size")]
    pub text_bytes: Value,
}

peek_foundation::impl_info_extras!(ManualInfo, json = "manual");

pub fn gather_extras() -> Extras {
    let (count, bytes) =
        super::chapters().fold((0u64, 0u64), |(n, b), c| (n + 1, b + c.body.len() as u64));
    Box::new(ManualInfo {
        version: Value::text(env!("CARGO_PKG_VERSION")),
        chapters: Value::int(count as i64),
        text_bytes: Value::size(bytes),
    })
}
