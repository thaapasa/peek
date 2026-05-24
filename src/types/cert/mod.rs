//! PEM-encoded certificate / key info: per-block decode of X.509
//! certificates, CSRs, CRLs, private/public keys, and OpenSSH public
//! keys, paired with the standard text-stats sidecar. The source view
//! still renders as plain UTF-8 text — the value-add is the parsed
//! Info section.

pub mod compose;
pub mod detect;
pub mod format;
pub mod info;
pub mod info_gather;
pub mod info_render;
