//! Image rendering mode selection. Drives which glyph palette
//! [`super::render`] uses when matching cells to characters.

/// Image rendering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMode {
    /// All glyphs: ASCII + block elements
    Full,
    /// Unicode block/quadrant elements + curated ASCII subset
    Block,
    /// Unicode block/quadrant elements + line segments (/\|-_) only
    Geo,
    /// Legacy density-ramp renderer (brightness-based, foreground only)
    Ascii,
    /// Sobel edge detection: render image as line-art contours
    Contour,
}

impl ImageMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "block" => Self::Block,
            "geo" => Self::Geo,
            "ascii" => Self::Ascii,
            "contour" => Self::Contour,
            _ => Self::Full,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Full => Self::Block,
            Self::Block => Self::Geo,
            Self::Geo => Self::Ascii,
            Self::Ascii => Self::Contour,
            Self::Contour => Self::Full,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Block => "block",
            Self::Geo => "geo",
            Self::Ascii => "ascii",
            Self::Contour => "contour",
        }
    }
}
