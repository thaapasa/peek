//! Property tests for the detection surface — the security floor for
//! pointing peek at hostile files. `peek-detect` is the first code to touch
//! untrusted bytes (magic probes, content sniff, the UTF-8 scan), and the
//! crate was split out precisely so this surface is reader-free and
//! fuzzable. These assert the public entry points never panic / hang on
//! arbitrary input, that the file and in-memory paths agree where they must,
//! and that known magic signatures still round-trip after hardening.
//!
//! Deeper coverage-guided fuzzing (libFuzzer, nightly) lives in `fuzz/` —
//! run via `just fuzz`. This stable suite is the permanent CI gate; a
//! crasher it finds is pinned under `proptest-regressions/`.

use std::io::Write;

use peek_detect::{FileType, detect, detect_ignore_name};
use peek_io::InputSource;
use proptest::prelude::*;

/// Bytes the file path reads before consulting the body in full — mirrors
/// the private `HEAD_BYTES`. At or below this size the file and in-memory
/// paths see identical content, so their content verdict must agree.
const HEAD_BYTES: usize = 8 * 1024;

fn mem(bytes: &[u8], name: &str) -> InputSource {
    InputSource::memory(bytes.to_vec(), name)
}

/// Detect through the real on-disk path by spilling bytes to a temp file.
fn detect_via_file(bytes: &[u8]) -> FileType {
    let mut f = tempfile::NamedTempFile::new().expect("temp file");
    f.write_all(bytes).expect("write");
    f.flush().expect("flush");
    detect_ignore_name(&InputSource::File(f.path().to_path_buf()))
        .expect("file detect")
        .file_type
}

proptest! {
    /// Never-panic / never-hang: arbitrary bytes and an arbitrary source
    /// name through both public entry points. A panic or non-termination
    /// here is the exact bug class the reader-free split was meant to expose
    /// — and what the bounded UTF-8 scan already fixed one instance of.
    #[test]
    fn detect_never_panics(
        bytes in proptest::collection::vec(any::<u8>(), 0..70_000),
        name in ".{0,40}",
    ) {
        let src = mem(&bytes, &name);
        let _ = detect(&src);
        let _ = detect_ignore_name(&src);
    }

    /// Two-path parity: for inputs the file path reads whole (≤ HEAD_BYTES)
    /// both paths see identical content, so the content-only verdict (name
    /// ignored) must match. A divergence is a real finding — the file/memory
    /// unification is still pending — not a flaky test.
    #[test]
    fn file_memory_parity_under_head(
        bytes in proptest::collection::vec(any::<u8>(), 0..=HEAD_BYTES),
    ) {
        let via_mem = detect_ignore_name(&mem(&bytes, "")).expect("mem detect").file_type;
        let via_file = detect_via_file(&bytes);
        prop_assert_eq!(via_mem, via_file);
    }
}

/// Regression: a multi-byte char straddling byte 512 of an XML-ish head
/// once panicked the `<html>` sniff in `sniff_text_content` (raw byte slice,
/// found by `just fuzz`). The verdict doesn't matter here — only that it
/// returns without panicking.
#[test]
fn xml_head_char_boundary_at_512() {
    // 510 ASCII '<' bytes, then a 3-byte char crossing index 512.
    let mut bytes = vec![b'<'; 510];
    bytes.extend_from_slice("櫃".as_bytes());
    bytes.extend_from_slice(b"<html>");
    let _ = detect(&mem(&bytes, "")).expect("detect");
}

/// Magic-byte corpus round-trips: real signatures must still surface their
/// `infer` MIME after never-panic hardening. Guards true positives from
/// silently degrading into the text/binary fallback. Deterministic table —
/// not generated input.
#[test]
fn magic_corpus_round_trips() {
    // (leading bytes, substring expected in the detected magic MIME).
    let cases: &[(&[u8], &str)] = &[
        (b"\x89PNG\r\n\x1a\n\x00\x00", "png"),
        (b"\xff\xd8\xff\xe0\x00\x10JFIF", "jpeg"),
        (b"GIF89a\x01\x00\x01\x00", "gif"),
        (b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n", "pdf"),
        (b"PK\x03\x04\x14\x00\x00\x00", "zip"),
        (b"\x1f\x8b\x08\x00\x00\x00\x00\x00", "gzip"),
    ];
    for (bytes, want_mime) in cases {
        let detected = detect(&mem(bytes, "")).expect("detect");
        let mime = detected.magic_mime.unwrap_or_default();
        assert!(
            mime.contains(want_mime),
            "magic {want_mime:?} not detected for {:02x?}: got mime {mime:?}, type {:?}",
            &bytes[..bytes.len().min(8)],
            detected.file_type,
        );
    }
}
