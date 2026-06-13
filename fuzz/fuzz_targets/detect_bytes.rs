//! Coverage-guided fuzz target for the detection surface. Drives both
//! public entry points over arbitrary bytes; libFuzzer mutates toward new
//! code paths in the magic probes / content sniff / UTF-8 scan. A crash,
//! hang, or OOM is a finding. The stable property-test floor lives in
//! `crates/peek-detect/tests/fuzz_detect.rs`; this is the deep nightly pass.

#![no_main]

use libfuzzer_sys::fuzz_target;
use peek_io::InputSource;

fuzz_target!(|data: &[u8]| {
    let src = InputSource::memory(data.to_vec(), "fuzz.bin");
    let _ = peek_detect::detect(&src);
    let _ = peek_detect::detect_ignore_name(&src);
});
