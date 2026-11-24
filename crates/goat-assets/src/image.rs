//! Images → [`TextureData`] (RGBA8, rows top to bottom): PNG of any color type / bit depth.
//! Import settings (`<texture>.import.json`, like Unity's texture importer):
//! `{ "filter": "nearest"|"linear", "wrap": "repeat"|"clamp", "mipmaps": true, "format": "rgba8"|"rgb565"|"rgba4"|"rgba5551"|"etc1"|"etc1a4", "max_size": 256 }`.
//! `etc1` / `etc1a4` are block-compressed on the 3DS (4 / 8 bits per pixel); desktop keeps RGBA8.

use goat_gfx::{Filter, TextureData, TextureFormat, Wrap};
use serde::Deserialize;

/// Decodes a PNG into RGBA8.
pub fn decode_png(bytes: &[u8]) -> Result<TextureData, String> {
    let mut decoder = png::Decoder::new(bytes);
    // palette → RGB, < 8 bits → 8, tRNS → alpha, 16 bits → 8
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width, info.height);
    let src = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => src.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => src.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => src.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("indexed PNG not expanded".into()),
    };
    Ok(TextureData { width: w, height: h, rgba, filter: Filter::Linear, wrap: Wrap::Repeat, mipmaps: false, format: TextureFormat::Rgba8 })
}

/// `<texture>.import.json`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct TextureImport {
    pub filter: Option<String>,
    pub wrap: Option<String>,
    pub mipmaps: Option<bool>,
    pub format: Option<String>,
    /// Downscale so neither side exceeds this (power of two).
    pub max_size: Option<u32>,
}

impl TextureImport {
    pub fn apply(&self, mut t: TextureData) -> TextureData {
        match self.filter.as_deref() {
            Some("nearest") | Some("point") => t.filter = Filter::Nearest,
            Some("linear") | Some("bilinear") => t.filter = Filter::Linear,
            _ => {}
        }
        match self.wrap.as_deref() {
            Some("clamp") => t.wrap = Wrap::Clamp,
            Some("repeat") => t.wrap = Wrap::Repeat,
            _ => {}
        }
        if let Some(m) = self.mipmaps {
            t.mipmaps = m;
        }
        t.format = match self.format.as_deref() {
            Some("rgb565") => TextureFormat::Rgb565,
            Some("rgba4") => TextureFormat::Rgba4,
            Some("rgba5551") => TextureFormat::Rgba5551,
            Some("etc1") => TextureFormat::Etc1,
            Some("etc1a4") => TextureFormat::Etc1A4,
            _ => t.format,
        };
        if let Some(max) = self.max_size {
            let max = max.max(8);
            if t.width > max || t.height > max {
                let s = (max as f32 / t.width.max(t.height) as f32).min(1.0);
                let (w, h) = (((t.width as f32 * s) as u32).max(1), ((t.height as f32 * s) as u32).max(1));
                t = resize(&t, w, h);
            }
        }
        t
    }
}

/// Bilinear resize (import-time only).
pub fn resize(t: &TextureData, w: u32, h: u32) -> TextureData {
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let (sw, sh) = (t.width as f32, t.height as f32);
    for y in 0..h {
        for x in 0..w {
            let fx = ((x as f32 + 0.5) * sw / w as f32 - 0.5).clamp(0.0, sw - 1.0);
            let fy = ((y as f32 + 0.5) * sh / h as f32 - 0.5).clamp(0.0, sh - 1.0);
            let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
            let (x1, y1) = ((x0 + 1).min(t.width - 1), (y0 + 1).min(t.height - 1));
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            for c in 0..4 {
                let p = |px: u32, py: u32| t.rgba[((py * t.width + px) * 4 + c) as usize] as f32;
                let top = p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx;
                let bottom = p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx;
                rgba[((y * w + x) * 4 + c) as usize] = (top * (1.0 - ty) + bottom * ty).round() as u8;
            }
        }
    }
    TextureData { width: w, height: h, rgba, ..t.clone_meta() }
}

/// The nearest power-of-two size (8..1024) — GPUs like the 3DS's need them.
pub fn to_power_of_two(t: &TextureData) -> TextureData {
    let pot = |v: u32| {
        let up = v.next_power_of_two();
        let down = up / 2;
        (if down >= 8 && v - down < up - v { down } else { up }).clamp(8, 1024)
    };
    let (w, h) = (pot(t.width), pot(t.height));
    if (w, h) == (t.width, t.height) {
        t.clone()
    } else {
        resize(t, w, h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(w: u32, h: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, w, h);
            e.set_color(color);
            e.set_depth(png::BitDepth::Eight);
            e.write_header().unwrap().write_image_data(data).unwrap();
        }
        out
    }

    #[test]
    fn png_rgb_and_gray_to_rgba() {
        let t = decode_png(&encode(2, 1, png::ColorType::Rgb, &[255, 0, 0, 0, 0, 255])).unwrap();
        assert_eq!((t.width, t.height), (2, 1));
        assert_eq!(t.rgba, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        let g = decode_png(&encode(1, 1, png::ColorType::GrayscaleAlpha, &[100, 50])).unwrap();
        assert_eq!(g.rgba, vec![100, 100, 100, 50]);
    }

    #[test]
    fn power_of_two_resize() {
        let t = TextureData { width: 100, height: 30, rgba: vec![200; 100 * 30 * 4], ..TextureData::solid(goat_core::Color::WHITE) };
        let p = to_power_of_two(&t);
        assert_eq!((p.width, p.height), (128, 32));
        assert!(p.rgba.iter().all(|&v| v == 200));
    }
}
