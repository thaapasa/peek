# Detection fuzzing

Coverage-guided fuzz targets for `peek-detect` — the reader-free surface
that classifies untrusted bytes before any viewer runs. The crate split
exists to keep this surface small and fuzzable; these targets are the deep
pass.

The permanent, stable regression gate is the property-test suite in
`crates/peek-detect/tests/fuzz_detect.rs` (runs in `cargo test --workspace`).
This crate is **excluded from the workspace** (`libfuzzer-sys` needs nightly)
and is run on demand.

## Setup

```sh
rustup toolchain install nightly      # one-time
cargo install cargo-fuzz              # one-time
```

## Run

```sh
just fuzz                  # detect_bytes target, 60s
just fuzz detect_bytes 300 # explicit target + seconds
```

Or directly:

```sh
cargo +nightly fuzz run detect_bytes -- -max_total_time=60
```

A crash writes a reproducer under `fuzz/artifacts/<target>/`. Re-run a
single case with `cargo +nightly fuzz run detect_bytes <artifact-path>`.
When a crasher is confirmed, add a minimized reproducer as a regression
case to `tests/fuzz_detect.rs` so the stable suite guards it thereafter.

## Targets

- `detect_bytes` — drives `detect` + `detect_ignore_name` over an in-memory
  source built from the fuzz input.
