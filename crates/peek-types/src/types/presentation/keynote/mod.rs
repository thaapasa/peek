//! Apple Keynote (`.key` iWork package).
//!
//! Modern Keynote stores slide text as undocumented snappy-protobuf
//! (`Index/*.iwa`), which there's no usable Rust library for — so slide
//! text isn't extracted. What the package *does* carry cheaply is the
//! QuickLook deck thumbnail (`preview.jpg` and friends) and an XML build
//! history; [`package`] reads those and [`preview`] renders the
//! thumbnail through the image pipeline as the deck's primary view.

pub mod package;
pub mod preview;
