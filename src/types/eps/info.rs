//! EPS / PostScript info shape.

use super::dos_eps::PreviewKind;
use super::dsc::DscInfo;
use super::format::PostScriptFormat;

/// Metadata about an embedded DOS-EPS preview.
#[derive(Debug, Clone)]
pub struct PreviewMeta {
    pub kind: PreviewKind,
    /// Byte length of the preview section.
    pub bytes: usize,
    /// Decoded pixel dimensions (best-effort; `None` for WMF or a
    /// preview the image crate can't decode).
    pub dimensions: Option<(u32, u32)>,
}

#[derive(Debug, Clone)]
pub struct EpsInfo {
    pub format: PostScriptFormat,
    pub dsc: DscInfo,
    /// Embedded preview, when the file is a binary DOS-EPS that carries
    /// one.
    pub preview: Option<PreviewMeta>,
    /// Whether a Ghostscript interpreter was found on PATH — drives the
    /// "Render" availability hint.
    pub gs_available: bool,
}
