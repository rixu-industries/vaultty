// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

use std::fmt;

/// RGBA color with 8 bits per channel. In JSON it is written as `#rgb`, `#rrggbb` or `#rrggbbaa`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// The same color with its alpha multiplied by `alpha` (0 to 1): the tints of the tokens.
    pub fn with_alpha(self, alpha: f32) -> Self {
        Self {
            a: (self.a as f32 * alpha).round().clamp(0., 255.) as u8,
            ..self
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.strip_prefix('#')?;

        if !hex.is_ascii() {
            return None;
        }

        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        // `#abc` is `#aabbcc`: each digit is repeated.
        let short = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|v| v * 17);

        match hex.len() {
            3 => Some(Self {
                r: short(0)?,
                g: short(1)?,
                b: short(2)?,
                a: 255,
            }),
            6 => Some(Self {
                r: byte(0)?,
                g: byte(2)?,
                b: byte(4)?,
                a: 255,
            }),
            8 => Some(Self {
                r: byte(0)?,
                g: byte(2)?,
                b: byte(4)?,
                a: byte(6)?,
            }),
            _ => None,
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)?;

        if self.a != 255 {
            write!(f, "{:02x}", self.a)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_three_formats() {
        let rgb = |r, g, b, a| Some(Color { r, g, b, a });

        assert_eq!(Color::parse("#1b2c3d"), rgb(0x1b, 0x2c, 0x3d, 255));
        assert_eq!(Color::parse("#1b2c3d80"), rgb(0x1b, 0x2c, 0x3d, 0x80));
        assert_eq!(Color::parse("#fa0"), rgb(0xff, 0xaa, 0x00, 255));
        assert_eq!(Color::parse("1b2c3d"), None);
        assert_eq!(Color::parse("#12345"), None);
        assert_eq!(Color::parse("#gggggg"), None);
        assert_eq!(Color::parse("#café"), None);
    }

    #[test]
    fn writes_what_it_reads() {
        for text in ["#1b2c3d", "#1b2c3d80"] {
            assert_eq!(Color::parse(text).unwrap().to_string(), text);
        }
    }
}
