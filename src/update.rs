//! Self-update: query GitHub Releases for the newest version and, if
//! it's newer than `CARGO_PKG_VERSION`, run the official `install.sh`
//! to replace the running binary in-place. Reuses install.sh so the
//! upgrade path matches a fresh install — checksum verification, PATH
//! hint, etc. all come from one place.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};

const REPO: &str = "thaapasa/peek";
const RELEASES_API: &str = "https://api.github.com/repos/thaapasa/peek/releases/latest";
const INSTALL_SCRIPT_URL: &str = "https://raw.githubusercontent.com/thaapasa/peek/main/install.sh";

pub fn run() -> Result<()> {
    if cfg!(windows) {
        bail!(
            "--update is not supported on Windows; download a release from https://github.com/{REPO}/releases"
        );
    }

    let current_exe =
        std::env::current_exe().context("failed to locate the current peek executable")?;
    if current_exe.to_string_lossy().contains("/target/") {
        bail!(
            "refusing to update a development build at {}",
            current_exe.display()
        );
    }
    let install_dir = current_exe
        .parent()
        .ok_or_else(|| anyhow!("current executable has no parent directory"))?;

    let current = env!("CARGO_PKG_VERSION");
    let latest_tag = fetch_latest_tag()?;
    let latest = latest_tag.strip_prefix('v').unwrap_or(&latest_tag);

    println!("peek {current} (current) → {latest} (latest on github.com/{REPO})");

    if !is_newer(latest, current) {
        println!("already on latest");
        return Ok(());
    }

    println!(
        "upgrading to {latest} via install.sh → {}",
        install_dir.display()
    );

    let script = fetch(INSTALL_SCRIPT_URL).context("failed to fetch install.sh")?;
    let mut sh = Command::new("sh")
        .env("PEEK_INSTALL_DIR", install_dir)
        .env("PEEK_VERSION", latest)
        .stdin(Stdio::piped())
        .spawn()
        .context("failed to spawn sh")?;
    sh.stdin
        .as_mut()
        .ok_or_else(|| anyhow!("failed to open sh stdin"))?
        .write_all(script.as_bytes())?;
    drop(sh.stdin.take());
    let status = sh.wait().context("install.sh did not finish")?;
    if !status.success() {
        bail!("install.sh exited with status {status}");
    }
    Ok(())
}

fn fetch_latest_tag() -> Result<String> {
    let body = fetch(RELEASES_API)?;
    let v: serde_json::Value =
        serde_json::from_str(&body).context("failed to parse GitHub releases JSON")?;
    let tag = v
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("releases response missing tag_name"))?;
    Ok(tag.to_string())
}

fn fetch(url: &str) -> Result<String> {
    let ua = concat!("peek/", env!("CARGO_PKG_VERSION"));
    let out = Command::new("curl")
        .args(["-fsSL", "-A", ua, url])
        .output()
        .context("failed to execute curl (is it installed?)")?;
    if !out.status.success() {
        bail!(
            "curl failed for {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    String::from_utf8(out.stdout).context("curl output was not UTF-8")
}

fn is_newer(latest: &str, current: &str) -> bool {
    fn parse(s: &str) -> Option<(u32, u32, u32)> {
        let mut parts = s.split('.');
        let a = parts.next()?.parse().ok()?;
        let b = parts.next()?.parse().ok()?;
        let c = parts.next()?.parse().ok()?;
        Some((a, b, c))
    }
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => latest != current,
    }
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn newer_version_detected() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(is_newer("0.1.10", "0.1.9"));
    }

    #[test]
    fn same_or_older_not_newer() {
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.1.10"));
    }
}
