//! GoatEngine core: the vocabulary every other crate shares.
//!
//! Nothing here knows about a platform, a GPU or a window. It builds for every target
//! (desktop, Nintendo 3DS, ...) with `std` only.

pub mod handle;
pub mod log;
pub mod time;

/// Math types (glam): `Vec3`, `Quat`, `Mat4`... Re-exported so every crate uses the same version.
pub mod math {
    pub use glam::{vec2, vec3, vec4, Affine3A, EulerRot, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};

    /// An RGBA color in linear 0..1 floats.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Color {
        pub r: f32,
        pub g: f32,
        pub b: f32,
        pub a: f32,
    }

    impl Color {
        pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
        pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);

        pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
            Color { r, g, b, a: 1.0 }
        }
        pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
            Color { r, g, b, a }
        }
        /// 0xRRGGBB
        pub fn hex(rgb: u32) -> Self {
            Color::rgb(((rgb >> 16) & 255) as f32 / 255.0, ((rgb >> 8) & 255) as f32 / 255.0, (rgb & 255) as f32 / 255.0)
        }
        pub fn to_array(self) -> [f32; 4] {
            [self.r, self.g, self.b, self.a]
        }
        /// Colors are authored in sRGB (what you pick is what a gamma-space target such as the
        /// 3DS shows). Linear-space renderers convert with this (alpha unchanged), like Unity's
        /// Linear color space.
        pub fn to_linear(self) -> Self {
            let f = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
            Color { r: f(self.r), g: f(self.g), b: f(self.b), a: self.a }
        }
        /// The inverse of [`to_linear`](Self::to_linear) (file formats with linear colors, e.g. glTF).
        pub fn from_linear(self) -> Self {
            let f = |c: f32| if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.max(0.0).powf(1.0 / 2.4) - 0.055 };
            Color { r: f(self.r), g: f(self.g), b: f(self.b), a: self.a }
        }
        /// Packed 0xRRGGBBAA, components clamped to 0..1.
        pub fn to_rgba8(self) -> [u8; 4] {
            let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            [c(self.r), c(self.g), c(self.b), c(self.a)]
        }
    }

    impl Default for Color {
        fn default() -> Self {
            Color::WHITE
        }
    }
}

pub use math::Color;

/// Colors in files: `"#rrggbb"` / `"#rrggbbaa"` (sRGB) or `[r, g, b(, a)]` floats. Written as hex
/// when exact in 8 bits, else as floats.
mod color_serde {
    use super::Color;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn parse_hex(h: &str) -> Option<Color> {
        let h = h.strip_prefix('#')?;
        let v = u32::from_str_radix(h, 16).ok()?;
        let c = |shift: u32| ((v >> shift) & 255) as f32 / 255.0;
        match h.len() {
            6 => Some(Color::rgb(c(16), c(8), c(0))),
            8 => Some(Color::rgba(c(24), c(16), c(8), c(0))),
            _ => None,
        }
    }

    impl Serialize for Color {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let [r, g, b, a] = self.to_rgba8();
            if Color::rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0) == *self {
                if a == 255 {
                    s.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}"))
                } else {
                    s.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}{a:02x}"))
                }
            } else {
                self.to_array().serialize(s)
            }
        }
    }

    impl<'de> Deserialize<'de> for Color {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            #[derive(Deserialize)]
            #[serde(untagged)]
            enum Repr {
                Hex(String),
                Floats(Vec<f32>),
            }
            match Repr::deserialize(d)? {
                Repr::Hex(h) => parse_hex(&h).ok_or_else(|| serde::de::Error::custom(format!("bad color {h}"))),
                Repr::Floats(v) if v.len() == 3 || v.len() == 4 => Ok(Color::rgba(v[0], v[1], v[2], v.get(3).copied().unwrap_or(1.0))),
                Repr::Floats(_) => Err(serde::de::Error::custom("a color needs 3 or 4 numbers")),
            }
        }
    }
}
pub use color_serde::parse_hex;

#[cfg(test)]
mod tests {
    use super::Color;

    #[test]
    fn srgb_to_linear() {
        let c = Color::rgba(0.5, 0.0, 1.0, 0.25).to_linear();
        assert!((c.r - 0.214).abs() < 0.001);
        assert_eq!((c.g, c.b, c.a), (0.0, 1.0, 0.25));
    }
}
