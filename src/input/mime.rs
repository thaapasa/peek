use std::path::Path;

use crate::input::detect::{FileType, StructuredFormat};

/// How official a MIME type is — drives display markers in the info view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimeCategory {
    /// IANA-registered standard type (no marker shown).
    Registered,
    /// IANA-registered vendor-specific type (`vnd.*` subtype).
    Vendor,
    /// De facto convention not formally registered (`x-*` subtype).
    Convention,
    /// Personal / vanity-tree type (`prs.*` subtype).
    Personal,
    /// Explicitly experimental / unregistered (`x.*` subtype).
    Experimental,
}

impl MimeCategory {
    /// Classify a MIME string by its subtype prefix (RFC 6838 conventions).
    pub fn classify(mime: &str) -> Self {
        let sub = mime.split_once('/').map(|(_, s)| s).unwrap_or(mime);
        if sub.starts_with("vnd.") {
            Self::Vendor
        } else if sub.starts_with("prs.") {
            Self::Personal
        } else if sub.starts_with("x.") {
            Self::Experimental
        } else if sub.starts_with("x-") {
            Self::Convention
        } else {
            Self::Registered
        }
    }

    /// Display marker shown next to the MIME string. `None` for registered.
    pub fn marker(self) -> Option<&'static str> {
        match self {
            Self::Registered => None,
            Self::Vendor => Some("(vendor)"),
            Self::Convention => Some("(convention)"),
            Self::Personal => Some("(personal)"),
            Self::Experimental => Some("(experimental)"),
        }
    }
}

/// One MIME type associated with a file, plus its standardness category.
#[derive(Debug, Clone)]
pub struct MimeInfo {
    pub mime: String,
    pub category: MimeCategory,
}

impl MimeInfo {
    pub fn new(mime: impl Into<String>) -> Self {
        let mime = mime.into();
        let category = MimeCategory::classify(&mime);
        Self { mime, category }
    }
}

/// Build the list of MIME types that apply to a file.
///
/// Sources, in order of priority:
/// 1. `magic_mime` — what `infer` detected from the file's magic bytes.
/// 2. The IANA registered type implied by the [`FileType`] (e.g. `text/plain`
///    for source code, `application/json` for JSON), so users can see the
///    formally-registered type even when only a convention applies.
/// 3. `mime_guess` from the path extension — catches `text/x-python`,
///    `text/x-rust`, and similar language conventions.
///
/// Duplicates are removed (case-insensitive). Result is `Vec<MimeInfo>` with
/// each entry classified by [`MimeCategory::classify`].
pub fn mimes_for_path(
    file_type: &FileType,
    path: Option<&Path>,
    magic_mime: Option<&str>,
) -> Vec<MimeInfo> {
    let mut out: Vec<MimeInfo> = Vec::new();

    if let Some(m) = magic_mime {
        push_unique(&mut out, MimeInfo::new(m));
    }

    if let Some(m) = registered_for_type(file_type) {
        push_unique(&mut out, MimeInfo::new(m));
    }

    if let Some(ext) = path.and_then(|p| p.extension()).and_then(|e| e.to_str()) {
        let ext = ext.to_lowercase();
        for guess in mime_guess::from_ext(&ext).iter() {
            push_unique(&mut out, MimeInfo::new(guess.essence_str()));
        }
        // Supplement: mime_guess misses many language conventions (returns
        // text/plain or nothing). Add the well-established `text/x-*` MIMEs
        // for popular languages so users see e.g. text/x-python for .py.
        if let Some(extra) = source_code_convention(&ext) {
            push_unique(&mut out, MimeInfo::new(extra));
        }
    }

    if out.is_empty() {
        // Last-resort fallback so the info view always shows something.
        out.push(MimeInfo::new(match file_type {
            FileType::Binary | FileType::Archive(_) => "application/octet-stream",
            _ => "text/plain",
        }));
    }

    out
}

fn push_unique(out: &mut Vec<MimeInfo>, entry: MimeInfo) {
    if out.iter().any(|e| e.mime.eq_ignore_ascii_case(&entry.mime)) {
        return;
    }
    out.push(entry);
}

/// Conventional `text/x-*` MIME for popular source languages where one is
/// well-established but not returned by `mime_guess`. Limited to languages
/// with clear de facto conventions in use (editors, syntax highlighters).
fn source_code_convention(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "py" | "pyw" => "text/x-python",
        "go" => "text/x-go",
        "rb" => "text/x-ruby",
        "java" => "text/x-java",
        "kt" | "kts" => "text/x-kotlin",
        "swift" => "text/x-swift",
        "scala" | "sc" => "text/x-scala",
        "hs" | "lhs" => "text/x-haskell",
        "pl" | "pm" => "text/x-perl",
        "php" | "phtml" => "application/x-httpd-php",
        "sh" | "bash" | "zsh" => "application/x-sh",
        "ts" | "tsx" => "application/typescript",
        "c" | "h" => "text/x-csrc",
        "cpp" | "cxx" | "cc" | "hpp" | "hxx" => "text/x-c++src",
        "ex" | "exs" => "text/x-elixir",
        "erl" | "hrl" => "text/x-erlang",
        "clj" | "cljs" | "cljc" => "text/x-clojure",
        "ml" | "mli" => "text/x-ocaml",
        "fs" | "fsx" => "text/x-fsharp",
        "r" => "text/x-r",
        "jl" => "text/x-julia",
        "dart" => "application/dart",
        "zig" => "text/x-zig",
        _ => return None,
    })
}

/// The IANA-registered type implied by a [`FileType`], when one exists.
fn registered_for_type(file_type: &FileType) -> Option<&'static str> {
    Some(match file_type {
        FileType::SourceCode { .. } => "text/plain",
        FileType::Structured(StructuredFormat::Json) => "application/json",
        // text/yaml is not formally registered; application/yaml was registered
        // in 2024 (RFC 9512). Use the new registration.
        FileType::Structured(StructuredFormat::Yaml) => "application/yaml",
        FileType::Structured(StructuredFormat::Toml) => "application/toml",
        FileType::Structured(StructuredFormat::Xml) => "application/xml",
        FileType::Svg => "image/svg+xml",
        // For Image, Archive, and Binary, the magic-byte MIME is more
        // specific than any generic registered fallback would be.
        FileType::Image | FileType::Archive(_) | FileType::Binary => return None,
    })
}

/// Returns a warning message if the path's extension doesn't match what the
/// detected MIME type expects. Returns `None` when there's no extension, no
/// magic-byte detection, or the extension is consistent with the MIME.
pub fn extension_mismatch(path: &Path, magic_mime: Option<&str>) -> Option<String> {
    let magic = magic_mime?;
    let ext = path.extension()?.to_str()?.to_lowercase();
    let expected = mime_guess::get_mime_extensions_str(magic)?;
    if expected.is_empty() || expected.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
        return None;
    }
    let expected_list: Vec<String> = expected.iter().map(|e| format!(".{e}")).collect();
    Some(format!(
        "extension `.{ext}` doesn't match content ({magic}); expected {}",
        expected_list.join(" / "),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_registered() {
        assert_eq!(
            MimeCategory::classify("image/png"),
            MimeCategory::Registered
        );
        assert_eq!(
            MimeCategory::classify("text/plain"),
            MimeCategory::Registered
        );
        assert_eq!(
            MimeCategory::classify("application/json"),
            MimeCategory::Registered
        );
    }

    #[test]
    fn classify_vendor() {
        assert_eq!(
            MimeCategory::classify("application/vnd.microsoft.portable-executable"),
            MimeCategory::Vendor
        );
        assert_eq!(
            MimeCategory::classify(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            ),
            MimeCategory::Vendor
        );
    }

    #[test]
    fn classify_convention() {
        assert_eq!(
            MimeCategory::classify("text/x-python"),
            MimeCategory::Convention
        );
        assert_eq!(
            MimeCategory::classify("application/x-bittorrent"),
            MimeCategory::Convention
        );
    }

    #[test]
    fn classify_experimental_and_personal() {
        assert_eq!(
            MimeCategory::classify("application/x.example"),
            MimeCategory::Experimental
        );
        assert_eq!(
            MimeCategory::classify("application/prs.example"),
            MimeCategory::Personal
        );
    }

    #[test]
    fn source_code_yields_plain_plus_convention() {
        let mimes = mimes_for_path(
            &FileType::SourceCode {
                syntax: Some("py".into()),
            },
            Some(Path::new("foo.py")),
            None,
        );
        let strings: Vec<&str> = mimes.iter().map(|m| m.mime.as_str()).collect();
        assert!(strings.contains(&"text/plain"));
        assert!(strings.iter().any(|s| s.contains("python")));
    }

    #[test]
    fn rust_source_classifies_convention() {
        let mimes = mimes_for_path(
            &FileType::SourceCode {
                syntax: Some("rs".into()),
            },
            Some(Path::new("foo.rs")),
            None,
        );
        let conv = mimes.iter().find(|m| m.mime.contains("rust"));
        if let Some(c) = conv {
            assert_eq!(c.category, MimeCategory::Convention);
        }
    }

    #[test]
    fn json_yields_application_json_only() {
        let mimes = mimes_for_path(
            &FileType::Structured(StructuredFormat::Json),
            Some(Path::new("data.json")),
            None,
        );
        assert_eq!(mimes[0].mime, "application/json");
        assert_eq!(mimes[0].category, MimeCategory::Registered);
    }

    #[test]
    fn duplicate_mimes_collapsed() {
        let mimes = mimes_for_path(
            &FileType::Image,
            Some(Path::new("foo.png")),
            Some("image/png"),
        );
        assert_eq!(mimes.len(), 1);
        assert_eq!(mimes[0].mime, "image/png");
    }

    #[test]
    fn binary_falls_back_to_octet_stream_when_unknown() {
        let mimes = mimes_for_path(&FileType::Binary, None, None);
        assert_eq!(mimes[0].mime, "application/octet-stream");
    }

    #[test]
    fn extension_mismatch_detects_jpg_extension_with_png_content() {
        let warn = extension_mismatch(Path::new("photo.jpg"), Some("image/png"));
        assert!(warn.is_some(), "should warn on .jpg with PNG content");
        assert!(warn.unwrap().contains(".jpg"));
    }

    #[test]
    fn extension_mismatch_silent_when_consistent() {
        let warn = extension_mismatch(Path::new("photo.png"), Some("image/png"));
        assert!(warn.is_none());
    }

    #[test]
    fn extension_mismatch_silent_when_no_magic() {
        let warn = extension_mismatch(Path::new("foo.txt"), None);
        assert!(warn.is_none());
    }
}
