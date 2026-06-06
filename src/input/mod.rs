//! CLI-level source dispatch. The input foundation itself lives in the
//! extracted `peek-io` (`InputSource` + streaming byte/line sources +
//! single-stream decompression codecs) and `peek-detect` (`FileType` +
//! format enums + magic / extension / content classification + transparent
//! decompression) crates; reference those directly. The only logic that
//! lives here is [`stdin`], which depends on the binary's `Args`.

pub mod stdin;
