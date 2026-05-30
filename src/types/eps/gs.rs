//! Optional Ghostscript bridge.
//!
//! True PostScript rendering needs an interpreter; no pure-Rust one
//! exists at usable quality, and Ghostscript is AGPL/GPL + a large C
//! dependency, so peek never bundles it. Instead it's detected on PATH
//! at runtime — when present, EPS/PS gain a high-fidelity "Render" view;
//! when absent, the viewer falls back to the embedded preview / source.
//!
//! The interpreter runs sandboxed (`-dSAFER`), one page, PostScript piped
//! in on stdin and a PNG streamed back on stdout — no temp files.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use image::DynamicImage;

/// Render DPI. 150 keeps a typical Letter / BoundingBox page in the
/// low-thousands of pixels — sharp enough for the ASCII pipeline and
/// the usual zoom range without rendering a wastefully huge bitmap.
const RENDER_DPI: u32 = 150;

/// Candidate executable names, in preference order. Unix ships `gs`;
/// Windows ships the console builds `gswin64c` / `gswin32c`.
const CANDIDATES: &[&str] = &["gs", "gswin64c", "gswin32c"];

/// Locate a Ghostscript executable on PATH, returning the name that
/// responds to `--version`. `None` means no usable interpreter — the
/// caller omits the Render view.
pub fn find() -> Option<&'static str> {
    CANDIDATES.iter().copied().find(|name| {
        Command::new(name)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

/// Render the first page of a PostScript / EPS program to a bitmap via
/// Ghostscript. `crop_to_bbox` enables `-dEPSCrop` (clip to the EPS
/// `%%BoundingBox`); plain `.ps` renders the full default page.
pub fn render(exe: &str, postscript: &[u8], crop_to_bbox: bool) -> Result<DynamicImage> {
    let mut cmd = Command::new(exe);
    cmd.args([
        "-q",
        "-dQUIET",
        "-dBATCH",
        "-dNOPAUSE",
        "-dSAFER",
        "-dFirstPage=1",
        "-dLastPage=1",
        "-sDEVICE=png16m",
    ]);
    if crop_to_bbox {
        cmd.arg("-dEPSCrop");
    }
    cmd.arg(format!("-r{RENDER_DPI}"));
    cmd.args(["-sOutputFile=-", "-"]);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().context("failed to spawn Ghostscript")?;
    child
        .stdin
        .take()
        .context("Ghostscript stdin unavailable")?
        .write_all(postscript)
        .context("failed to pipe PostScript to Ghostscript")?;
    let output = child
        .wait_with_output()
        .context("Ghostscript did not complete")?;
    if !output.status.success() {
        bail!("Ghostscript exited with {}", output.status);
    }
    if output.stdout.is_empty() {
        bail!("Ghostscript produced no output");
    }
    image::load_from_memory(&output.stdout).context("failed to decode Ghostscript PNG output")
}
