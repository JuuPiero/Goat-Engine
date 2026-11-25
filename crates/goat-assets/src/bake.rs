//! What a build does to the assets it ships (`goat build`): JSON text → [binary JSON](crate::bjson)
//! and textures → precompressed KTX for the target's GPU (3DS: ETC1 / ETC1A4, desktop: BC1 /
//! BC3), kept under their source names (loaders look at the bytes, not the extension).
//!
//! A texture stays as it is with `"compression": "none"` in its `.import.json`, with `"filter":
//! "nearest"` (pixel art: block compression would smear it), with a 16-bit `"format"`, or when it
//! is too small to matter.

use crate::image::{decode_jpeg, decode_png, encode_ktx, to_power_of_two, Codec, TextureImport};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Nintendo 3DS (PICA200: ETC1, power-of-two sizes).
    N3ds,
    /// Desktop GPUs (BC1 / BC3).
    Desktop,
}

/// JSON text as binary JSON (None: not JSON — left as it is).
pub fn bake_json(text: &str) -> Option<Vec<u8>> {
    let v: serde_json::Value = serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(text)).ok()?;
    Some(crate::bjson::encode(&v))
}

/// The JSON files a build converts: every `.json` asset (scenes, prefabs, materials, clips,
/// controllers, ScriptableObjects, input actions, import settings, project settings).
pub fn is_bakeable_json(key: &str) -> bool {
    key.ends_with(".json")
}

/// A texture file precompressed for `target` (None: shipped as it is).
pub fn bake_texture(bytes: &[u8], ext: &str, import_json: Option<&str>, target: Target) -> Result<Option<(Vec<u8>, Codec)>, String> {
    let import: TextureImport = match import_json {
        Some(t) => serde_json::from_str(t.strip_prefix('\u{feff}').unwrap_or(t)).map_err(|e| format!("import settings: {e}"))?,
        None => TextureImport::default(),
    };
    if import.compression.as_deref() == Some("none") || matches!(import.filter.as_deref(), Some("nearest") | Some("point")) {
        return Ok(None);
    }
    // an explicit format: ETC1 / ETC1A4 are block formats (3DS), the 16-bit ones stay as pixels
    let forced = match import.format.as_deref() {
        Some("etc1") => Some(Codec::Etc1),
        Some("etc1a4") => Some(Codec::Etc1A4),
        Some(_) => return Ok(None),
        None => None,
    };
    let t = match ext {
        "png" => decode_png(bytes)?,
        "jpg" | "jpeg" => decode_jpeg(bytes)?,
        _ => return Ok(None),
    };
    if t.width < 16 || t.height < 16 {
        return Ok(None);
    }
    let mut t = import.apply(t);
    t.compressed = None;
    t.bc = None;
    let alpha = t.rgba.chunks_exact(4).any(|p| p[3] < 255);
    let codec = match target {
        Target::N3ds => {
            // the PICA samples power-of-two sizes only: resize here, not on the console
            if !t.width.is_power_of_two() || !t.height.is_power_of_two() {
                t = to_power_of_two(&t);
            }
            forced.unwrap_or(if alpha { Codec::Etc1A4 } else { Codec::Etc1 })
        }
        Target::Desktop => {
            if forced.is_some() || t.width % 4 != 0 || t.height % 4 != 0 {
                return Ok(None);
            }
            if alpha {
                Codec::Bc3
            } else {
                Codec::Bc1
            }
        }
    };
    Ok(Some((encode_ktx(&t, codec), codec)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::decode_ktx;

    fn png(w: u32, h: u32, alpha: bool) -> Vec<u8> {
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, 90, if alpha { (x * 4) as u8 } else { 255 }]);
            }
        }
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, w, h);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            e.write_header().unwrap().write_image_data(&rgba).unwrap();
        }
        out
    }

    #[test]
    fn textures_are_precompressed_per_target() {
        let opaque = png(64, 32, false);
        let alpha = png(48, 48, true);
        // 3DS: ETC1 / ETC1A4, power-of-two sizes
        let (k, c) = bake_texture(&opaque, "png", None, Target::N3ds).unwrap().unwrap();
        assert_eq!(c, Codec::Etc1);
        let t = decode_ktx(&k).unwrap();
        assert!(t.compressed.is_some() && (t.width, t.height) == (64, 32));
        let (k, c) = bake_texture(&alpha, "png", Some(r#"{ "mipmaps": true }"#), Target::N3ds).unwrap().unwrap();
        assert_eq!(c, Codec::Etc1A4);
        let t = decode_ktx(&k).unwrap();
        assert!(t.width.is_power_of_two() && t.compressed.as_ref().unwrap().alpha && t.compressed.as_ref().unwrap().levels.len() > 1);
        // desktop: BC1 / BC3
        let (k, c) = bake_texture(&opaque, "png", None, Target::Desktop).unwrap().unwrap();
        assert_eq!(c, Codec::Bc1);
        let t = decode_ktx(&k).unwrap();
        assert!(t.bc.is_some() && !t.bc.as_ref().unwrap().alpha);
        let (_, c) = bake_texture(&alpha, "png", None, Target::Desktop).unwrap().unwrap();
        assert_eq!(c, Codec::Bc3);
        // kept as they are
        assert!(bake_texture(&opaque, "png", Some(r#"{ "compression": "none" }"#), Target::N3ds).unwrap().is_none());
        assert!(bake_texture(&opaque, "png", Some(r#"{ "filter": "nearest" }"#), Target::Desktop).unwrap().is_none());
        assert!(bake_texture(&opaque, "png", Some(r#"{ "format": "rgb565" }"#), Target::N3ds).unwrap().is_none());
        assert!(bake_texture(&png(8, 8, false), "png", None, Target::N3ds).unwrap().is_none());
    }

    #[test]
    fn json_becomes_binary_and_reads_back() {
        let bin = bake_json(r#"{ "a": [1, 2.5, "x"] }"#).unwrap();
        let io = crate::AssetIo::new("/nonexistent-bake");
        io.put("a.mat.json", bin);
        assert_eq!(io.read_json("a.mat.json").unwrap(), serde_json::json!({ "a": [1, 2.5, "x"] }));
        let v: serde_json::Value = serde_json::from_str(&io.read_string("a.mat.json").unwrap()).unwrap();
        assert_eq!(v["a"][2], "x");
        assert!(bake_json("not json").is_none());
    }
}
