//! Process-level check that print mode ends quietly when the reader closes
//! stdout early (`peek … | head`). Unix only: Windows has no `head`-style
//! early close in the test harness worth simulating.

#![cfg(unix)]

use std::io::Read;
use std::process::{Command, Stdio};

#[test]
fn closed_stdout_exits_zero_without_error() {
    // Cargo.lock printed with colour is far larger than a pipe buffer, so
    // peek is still writing when the read end drops.
    let mut child = Command::new(env!("CARGO_BIN_EXE_peek"))
        .args(["Cargo.lock", "--print", "--color", "truecolor"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn peek");

    let mut stdout = child.stdout.take().unwrap();
    let mut head = [0u8; 64];
    stdout.read_exact(&mut head).expect("read first bytes");
    drop(stdout);

    let output = child.wait_with_output().expect("wait for peek");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "exit {:?}, stderr: {stderr}",
        output.status
    );
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
}
