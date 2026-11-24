//! Material files (`*.mat.json`) — Unity's .mat:
//!
//! ```json
//! { "base_color": "#3d8bff", "texture": "../textures/crate.png", "lighting": "lit",
//!   "specular": "#999999", "shininess": 64, "emissive": "#000000",
//!   "blend": "opaque", "cull": "back", "depth_write": true, "fog": true }
//! ```
//! Every field is optional. `texture` is relative to the material file (or a root key with a
//! leading `/`). Colors are `#rrggbb` / `#rrggbbaa` (sRGB) or `[r, g, b, a]` floats.
//! Also: `"emissive_texture"` (× emissive), `"env_map"` (a `.cubemap.json` or `builtin:sky`) +
//! `"reflectivity"` 0..1, `"alpha_cutoff"` 0..1 (alpha test) + `"alpha_dither"`.

use goat_core::Color;
use goat_gfx::{BlendMode, CullMode, LightingModel, MaterialDesc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A color in files: "#rrggbb(aa)" or [r, g, b(, a)].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileColor(pub Color);

impl Serialize for FileColor {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let [r, g, b, a] = self.0.to_rgba8();
        // exact 8-bit colors as hex (readable), anything else (HDR / precise) as floats
        if Color::rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0) == self.0 {
            if a == 255 {
                s.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}"))
            } else {
                s.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}{a:02x}"))
            }
        } else {
            self.0.to_array().serialize(s)
        }
    }
}

impl<'de> Deserialize<'de> for FileColor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Hex(String),
            Floats(Vec<f32>),
        }
        match Repr::deserialize(d)? {
            Repr::Hex(h) => parse_hex(&h).map(FileColor).ok_or_else(|| serde::de::Error::custom(format!("bad color {h}"))),
            Repr::Floats(v) if v.len() == 3 || v.len() == 4 => Ok(FileColor(Color::rgba(v[0], v[1], v[2], v.get(3).copied().unwrap_or(1.0)))),
            Repr::Floats(_) => Err(serde::de::Error::custom("a color needs 3 or 4 numbers")),
        }
    }
}

pub use goat_core::parse_hex;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MaterialFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_color: Option<FileColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lighting: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub specular: Option<FileColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shininess: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emissive: Option<FileColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blend: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cull: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_write: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fog: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emissive_texture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_map: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reflectivity: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_cutoff: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_dither: Option<bool>,
}

impl MaterialFile {
    /// The description, without the texture (resolved by the asset server).
    pub fn desc(&self) -> MaterialDesc {
        let d = MaterialDesc::default();
        let blend = match self.blend.as_deref() {
            Some("alpha") | Some("transparent") => BlendMode::Alpha,
            Some("additive") => BlendMode::Additive,
            _ => BlendMode::Opaque,
        };
        MaterialDesc {
            base_color: self.base_color.map(|c| c.0).unwrap_or(d.base_color),
            texture: None,
            lighting: if self.lighting.as_deref() == Some("unlit") { LightingModel::Unlit } else { LightingModel::Lit },
            specular: self.specular.map(|c| c.0).unwrap_or(d.specular),
            shininess: self.shininess.unwrap_or(d.shininess),
            emissive: self.emissive.map(|c| c.0).unwrap_or(d.emissive),
            emissive_texture: None,
            env_map: None,
            reflectivity: self.reflectivity.unwrap_or(0.0).clamp(0.0, 1.0),
            alpha_cutoff: self.alpha_cutoff.unwrap_or(0.0).clamp(0.0, 1.0),
            alpha_dither: self.alpha_dither.unwrap_or(false),
            blend,
            cull: match self.cull.as_deref() {
                Some("none") | Some("off") => CullMode::None,
                Some("front") => CullMode::Front,
                _ => CullMode::Back,
            },
            // transparent materials don't write depth unless asked
            depth_write: self.depth_write.unwrap_or(blend == BlendMode::Opaque),
            fog: self.fog.unwrap_or(true),
            depth_test: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_material_file() {
        let m: MaterialFile = serde_json::from_str(r##"{ "base_color": "#3d8bff80", "texture": "a.png", "blend": "alpha", "specular": [0.5, 0.5, 0.5] }"##).unwrap();
        let d = m.desc();
        assert_eq!(d.base_color.to_rgba8(), [0x3d, 0x8b, 0xff, 0x80]);
        assert_eq!(d.blend, BlendMode::Alpha);
        assert!(!d.depth_write);
        assert_eq!(d.specular, Color::rgb(0.5, 0.5, 0.5));
        assert_eq!(serde_json::to_string(&FileColor(Color::hex(0x102030))).unwrap(), "\"#102030\"");
    }
}
