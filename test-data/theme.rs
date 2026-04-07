use std::fmt;

/// A color in the sRGB color space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse a hex color string like "#FF8040" or "FF8040".
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        if hex.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Self { r, g, b })
    }

    /// Linearly interpolate between two colors.
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: (self.r as f32 + (other.r as f32 - self.r as f32) * t) as u8,
            g: (self.g as f32 + (other.g as f32 - self.g as f32) * t) as u8,
            b: (self.b as f32 + (other.b as f32 - self.b as f32) * t) as u8,
        }
    }

    /// Perceived brightness (0.0–1.0) using the sRGB luminance formula.
    pub fn luminance(self) -> f32 {
        0.2126 * (self.r as f32 / 255.0)
            + 0.7152 * (self.g as f32 / 255.0)
            + 0.0722 * (self.b as f32 / 255.0)
    }

    /// ANSI 24-bit foreground escape sequence.
    pub fn fg_ansi(self) -> String {
        format!("\x1b[38;2;{};{};{}m", self.r, self.g, self.b)
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

/// Named color palette for a theme.
pub struct Palette {
    pub foreground: Color,
    pub background: Color,
    pub accent: Color,
    pub muted: Color,
    pub warning: Color,
}

impl Palette {
    /// Generate a gradient of `n` colors between two endpoints.
    pub fn gradient(from: Color, to: Color, n: usize) -> Vec<Color> {
        (0..n)
            .map(|i| {
                let t = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.0 };
                from.lerp(to, t)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_color() {
        assert_eq!(Color::from_hex("#FF0000"), Some(Color::new(255, 0, 0)));
        assert_eq!(Color::from_hex("00FF00"), Some(Color::new(0, 255, 0)));
        assert_eq!(Color::from_hex("#abc"), None); // too short
    }

    #[test]
    fn lerp_midpoint() {
        let black = Color::new(0, 0, 0);
        let white = Color::new(255, 255, 255);
        let mid = black.lerp(white, 0.5);
        assert!((mid.r as i16 - 127).abs() <= 1);
    }

    #[test]
    fn luminance_range() {
        let black = Color::new(0, 0, 0);
        let white = Color::new(255, 255, 255);
        assert!(black.luminance() < 0.01);
        assert!(white.luminance() > 0.99);
    }
}
