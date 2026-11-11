//! Embeds the user manual (`manual/src/`) as a generated chapter table in
//! `OUT_DIR/manual.rs`. Chapters are rewritten for the in-app copy by
//! `src/preprocess.rs`. A broken manual (a `SUMMARY.md` entry with no file,
//! a chapter missing from `SUMMARY.md`, a bad peek marker) fails the build
//! so it can't ship.

#[path = "src/preprocess.rs"]
mod preprocess;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

use preprocess::SummaryItem;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src_dir = manifest.join("../../manual/src");
    println!("cargo:rerun-if-changed={}", src_dir.display());
    println!("cargo:rerun-if-changed=src/preprocess.rs");

    if let Err(e) = generate(&src_dir) {
        panic!("embedded manual: {e}");
    }
}

fn generate(src_dir: &Path) -> Result<(), String> {
    let read = |rel: &str| {
        fs::read_to_string(src_dir.join(rel)).map_err(|e| format!("reading {rel}: {e}"))
    };
    let summary = read("SUMMARY.md")?;
    let items = preprocess::parse_summary(&summary).map_err(|e| format!("SUMMARY.md: {e}"))?;

    let mut listed = BTreeSet::new();
    let mut out = String::new();
    writeln!(out, "pub(crate) static SUMMARY: &str = {summary:?};").unwrap();
    writeln!(out, "pub(crate) static ENTRIES: &[TocEntry] = &[").unwrap();
    for item in &items {
        match item {
            SummaryItem::Part(title) => {
                writeln!(out, "    TocEntry::Part({title:?}),").unwrap();
            }
            SummaryItem::Chapter { title, path, depth } => {
                if !listed.insert(path.clone()) {
                    return Err(format!("SUMMARY.md lists {path} twice"));
                }
                let body =
                    preprocess::preprocess(&read(path)?).map_err(|e| format!("{path}: {e}"))?;
                writeln!(
                    out,
                    "    TocEntry::Chapter(Chapter {{ title: {title:?}, path: {path:?}, \
                     depth: {depth}, body: {body:?} }}),"
                )
                .unwrap();
            }
        }
    }
    writeln!(out, "];").unwrap();

    let mut on_disk = BTreeSet::new();
    collect_md(src_dir, src_dir, &mut on_disk)?;
    on_disk.remove("SUMMARY.md");
    let unlisted: Vec<_> = on_disk.difference(&listed).cloned().collect();
    if !unlisted.is_empty() {
        return Err(format!(
            "chapters missing from SUMMARY.md: {}",
            unlisted.join(", ")
        ));
    }

    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join("manual.rs");
    fs::write(&dest, out).map_err(|e| format!("writing {}: {e}", dest.display()))
}

/// Every `.md` under `dir`, as a forward-slash path relative to `root`.
fn collect_md(root: &Path, dir: &Path, out: &mut BTreeSet<String>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            collect_md(root, &path, out)?;
        } else if path.extension().is_some_and(|e| e == "md") {
            let rel = path.strip_prefix(root).unwrap();
            let key: Vec<_> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect();
            out.insert(key.join("/"));
        }
    }
    Ok(())
}
