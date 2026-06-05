//! Binary fallback: friendly format label from a magic-byte MIME plus a
//! trivial Format info section. Used both as the explicit handler for
//! `FileType::Binary` and as the fallback by other gather paths
//! (image / text / svg) when their primary detection fails.

pub mod info;
