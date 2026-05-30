use serde::{Deserialize, Serialize};

/// An RGBA color value.
///
/// Each channel is stored as `u8` (0–255). This type provides safe parsing
/// from hex color strings in the format `#RRGGBB` or `#RRGGBBAA`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RgbaColor {
    /// Pure black with full opacity.
    pub const BLACK: RgbaColor = RgbaColor {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    /// Pure white with full opacity.
    pub const WHITE: RgbaColor = RgbaColor {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };

    /// Create a new color from RGBA components.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Create a new color from RGB components with full opacity.
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Parse a hex color string.
    ///
    /// Supported formats:
    /// - `#RRGGBB` — 6 hex digits, alpha defaults to 255
    /// - `#RRGGBBAA` — 8 hex digits
    ///
    /// Returns an error if:
    /// - The string does not start with `#`
    /// - The hex digits are invalid
    /// - The length after `#` is not 6 or 8
    pub fn parse_hex(s: &str) -> Result<Self, String> {
        if !s.starts_with('#') {
            return Err(format!("color string must start with '#': got {:?}", s));
        }

        let hex = &s[1..];

        match hex.len() {
            6 => {
                let r = parse_hex_byte(&hex[0..2])?;
                let g = parse_hex_byte(&hex[2..4])?;
                let b = parse_hex_byte(&hex[4..6])?;
                Ok(Self::from_rgb(r, g, b))
            }
            8 => {
                let r = parse_hex_byte(&hex[0..2])?;
                let g = parse_hex_byte(&hex[2..4])?;
                let b = parse_hex_byte(&hex[4..6])?;
                let a = parse_hex_byte(&hex[6..8])?;
                Ok(Self::new(r, g, b, a))
            }
            other => Err(format!(
                "color string must have 6 or 8 hex digits after '#', got {} digits: {:?}",
                other, s
            )),
        }
    }

    /// Convert to a packed RGBA `u32` (0xRRGGBBAA).
    pub fn to_u32(self) -> u32 {
        ((self.r as u32) << 24) | ((self.g as u32) << 16) | ((self.b as u32) << 8) | (self.a as u32)
    }
}

impl std::fmt::Display for RgbaColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.a == 255 {
            write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            write!(
                f,
                "#{:02x}{:02x}{:02x}{:02x}",
                self.r, self.g, self.b, self.a
            )
        }
    }
}

/// Parse a two-character hex string as a `u8`.
fn parse_hex_byte(s: &str) -> Result<u8, String> {
    let byte =
        u8::from_str_radix(s, 16).map_err(|e| format!("invalid hex digits {:?}: {}", s, e))?;
    Ok(byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_black() {
        let c = RgbaColor::parse_hex("#000000").expect("parse");
        assert_eq!(c, RgbaColor::BLACK);
        assert_eq!(c.r, 0);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 255);
    }

    #[test]
    fn valid_white() {
        let c = RgbaColor::parse_hex("#ffffff").expect("parse");
        assert_eq!(c, RgbaColor::WHITE);
    }

    #[test]
    fn valid_with_alpha() {
        let c = RgbaColor::parse_hex("#ff000080").expect("parse");
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 128);
    }

    #[test]
    fn valid_mixed_case() {
        let c = RgbaColor::parse_hex("#AbCdEf").expect("parse");
        assert_eq!(c.r, 0xab);
        assert_eq!(c.g, 0xcd);
        assert_eq!(c.b, 0xef);
        assert_eq!(c.a, 255);
    }

    #[test]
    fn missing_hash() {
        let result = RgbaColor::parse_hex("000000");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must start with '#'"));
    }

    #[test]
    fn wrong_length_short() {
        let result = RgbaColor::parse_hex("#0000");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("6 or 8 hex digits"));
    }

    #[test]
    fn wrong_length_long() {
        let result = RgbaColor::parse_hex("#000000000");
        assert!(result.is_err());
    }

    #[test]
    fn invalid_hex_chars() {
        let result = RgbaColor::parse_hex("#gggggg");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid hex"));
    }

    #[test]
    fn display_rgb() {
        let c = RgbaColor::from_rgb(0x1a, 0x2b, 0x3c);
        assert_eq!(c.to_string(), "#1a2b3c");
    }

    #[test]
    fn display_rgba() {
        let c = RgbaColor::new(0x1a, 0x2b, 0x3c, 0x80);
        assert_eq!(c.to_string(), "#1a2b3c80");
    }

    #[test]
    fn to_u32() {
        let c = RgbaColor::from_rgb(0xff, 0x00, 0x80);
        assert_eq!(c.to_u32(), 0xff0080ff);
    }
}
