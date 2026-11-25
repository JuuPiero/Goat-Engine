//! Images → [`TextureData`] (RGBA8, rows top to bottom): PNG of any color type / bit depth.
//! Import settings (`<texture>.import.json`, like Unity's texture importer):
//! `{ "filter": "nearest"|"linear", "wrap": "repeat"|"clamp", "mipmaps": true, "format": "rgba8"|"rgb565"|"rgba4"|"rgba5551"|"etc1"|"etc1a4", "max_size": 256 }`.
//! `etc1` / `etc1a4` are block-compressed on the 3DS (4 / 8 bits per pixel); desktop keeps RGBA8.

use goat_gfx::{Filter, TextureData, TextureFormat, Wrap};
use serde::Deserialize;

/// Decodes a PNG into RGBA8.
/// RGBA8 pixels (rows top-down) as a PNG file (Texture2D.EncodeToPNG).
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    if rgba.len() != (width * height * 4) as usize {
        return Err(format!("encode_png: {} bytes for {width}×{height}", rgba.len()));
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

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
    Ok(TextureData { width: w, height: h, rgba, filter: Filter::Linear, wrap: Wrap::Repeat, mipmaps: false, format: TextureFormat::Rgba8, compressed: None, bc: None })
}

/// Decodes a JPEG (baseline / progressive, any color space) into RGBA8.
pub fn decode_jpeg(bytes: &[u8]) -> Result<TextureData, String> {
    use zune_core::{colorspace::ColorSpace, options::DecoderOptions};
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut d = zune_jpeg::JpegDecoder::new_with_options(bytes, options);
    let rgba = d.decode().map_err(|e| format!("{e:?}"))?;
    let info = d.info().ok_or("no JPEG header")?;
    Ok(TextureData { width: info.width as u32, height: info.height as u32, rgba, filter: Filter::Linear, wrap: Wrap::Repeat, mipmaps: false, format: TextureFormat::Rgba8, compressed: None, bc: None })
}

const KTX_ID: [u8; 12] = [0xAB, b'K', b'T', b'X', b' ', b'1', b'1', 0xBB, b'\r', b'\n', 0x1A, b'\n'];
const GL_ETC1_RGB8_OES: u32 = 0x8D64;
const GL_RGBA8: u32 = 0x8058;
const GL_RGB8: u32 = 0x8051;
const GL_DXT1: u32 = 0x83F1; // GL_COMPRESSED_RGBA_S3TC_DXT1_EXT
const GL_DXT5: u32 = 0x83F3; // GL_COMPRESSED_RGBA_S3TC_DXT5_EXT
/// ETC1A4 (the 3DS's ETC1 + 4-bit alpha blocks): no GL enum exists, GoatEngine's own value.
const GOAT_ETC1A4: u32 = 0x6E1A;

/// Decodes a KTX 1.1 texture: ETC1 (kept precompressed for the 3DS, decoded for other GPUs),
/// RGBA8 or RGB8, with its mip levels. Cube maps / arrays / 3D textures aren't supported.
pub fn decode_ktx(bytes: &[u8]) -> Result<TextureData, String> {
    if bytes.len() < 64 || bytes[..12] != KTX_ID {
        return Err("not a KTX 1.1 file".into());
    }
    let swap = match u32::from_le_bytes(bytes[12..16].try_into().unwrap()) {
        0x0403_0201 => false,
        0x0102_0304 => true,
        _ => return Err("bad KTX endianness".into()),
    };
    let word = |at: usize| -> Result<u32, String> {
        let b: [u8; 4] = bytes.get(at..at + 4).ok_or("KTX truncated")?.try_into().unwrap();
        Ok(if swap { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
    };
    let internal = word(28)?;
    let (width, height, depth) = (word(36)?, word(40)?.max(1), word(44)?);
    let (array, faces, mips, kv) = (word(48)?, word(52)?, word(56)?.max(1), word(60)? as usize);
    if depth > 1 || array > 0 || faces != 1 {
        return Err("KTX: only plain 2D textures are supported".into());
    }
    let mut at = 64 + kv;
    let mut levels: Vec<&[u8]> = Vec::new();
    for _ in 0..mips {
        let size = word(at)? as usize;
        levels.push(bytes.get(at + 4..at + 4 + size).ok_or("KTX truncated")?);
        at += 4 + size.div_ceil(4) * 4;
    }
    let base = TextureData { width, height, rgba: Vec::new(), filter: Filter::Linear, wrap: Wrap::Repeat, mipmaps: mips > 1, format: TextureFormat::Rgba8, compressed: None, bc: None };
    match internal {
        GL_ETC1_RGB8_OES | GOAT_ETC1A4 => {
            let alpha = internal == GOAT_ETC1A4;
            let blocks: Vec<Vec<u64>> = levels.iter().map(|l| l.chunks_exact(8).map(|b| u64::from_be_bytes(b.try_into().unwrap())).collect()).collect();
            let expected = width.div_ceil(4) as usize * height.div_ceil(4) as usize * if alpha { 2 } else { 1 };
            if blocks[0].len() < expected {
                return Err("KTX: ETC1 level 0 too small".into());
            }
            let rgba = goat_gfx::etc1::decode_image(width, height, &blocks[0], alpha);
            let format = if alpha { TextureFormat::Etc1A4 } else { TextureFormat::Etc1 };
            Ok(TextureData { rgba, format, compressed: Some(goat_gfx::etc1::Etc1Levels { alpha, levels: blocks }), ..base })
        }
        GL_DXT1 | GL_DXT5 => {
            let alpha = internal == GL_DXT5;
            let rgba = goat_gfx::bc::decode_image(width, height, levels[0], alpha);
            Ok(TextureData { rgba, bc: Some(goat_gfx::bc::BcLevels { alpha, levels: levels.iter().map(|l| l.to_vec()).collect() }), ..base })
        }
        GL_RGBA8 | GL_RGB8 => {
            let bpp = if internal == GL_RGBA8 { 4 } else { 3 };
            // rows are padded to 4 bytes
            let stride = (width as usize * bpp).div_ceil(4) * 4;
            let src = levels[0];
            if src.len() < stride * height as usize {
                return Err("KTX: level 0 too small".into());
            }
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for y in 0..height as usize {
                for p in src[y * stride..y * stride + width as usize * bpp].chunks_exact(bpp) {
                    rgba.extend_from_slice(&[p[0], p[1], p[2], if bpp == 4 { p[3] } else { 255 }]);
                }
            }
            Ok(TextureData { rgba, ..base })
        }
        other => Err(format!("KTX: unsupported internal format 0x{other:04X} (ETC1, ETC1A4, BC1, BC3, RGBA8, RGB8)")),
    }
}

/// Block compression of a precompressed texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    /// 3DS: 4 bits / pixel, no alpha.
    Etc1,
    /// 3DS: ETC1 + 4-bit alpha, 8 bits / pixel.
    Etc1A4,
    /// Desktop GPUs: 4 bits / pixel (BC1 / DXT1).
    Bc1,
    /// Desktop GPUs: 8 bits / pixel with alpha (BC3 / DXT5).
    Bc3,
}

/// Encodes a texture as an ETC1 KTX 1.1 file (with its mip chain when `t.mipmaps`): what builds
/// and tools write so the console never compresses.
pub fn encode_ktx_etc1(t: &TextureData) -> Vec<u8> {
    encode_ktx(t, Codec::Etc1)
}

/// Encodes a texture as a compressed KTX 1.1 file, with its mip chain when `t.mipmaps`.
pub fn encode_ktx(t: &TextureData, codec: Codec) -> Vec<u8> {
    let chain = t.mip_chain(1);
    let (internal, base_format, levels): (u32, u32, Vec<Vec<u8>>) = match codec {
        Codec::Etc1 | Codec::Etc1A4 => {
            let alpha = codec == Codec::Etc1A4;
            let blocks: Vec<Vec<u64>> = match &t.compressed {
                Some(c) if c.alpha == alpha => c.levels.clone(),
                _ => chain.iter().map(|(w, h, rgba)| goat_gfx::etc1::encode_image(*w, *h, rgba, alpha)).collect(),
            };
            let bytes = blocks.iter().map(|l| l.iter().flat_map(|b| b.to_be_bytes()).collect()).collect();
            (if alpha { GOAT_ETC1A4 } else { GL_ETC1_RGB8_OES }, if alpha { 0x1908 } else { 0x1907 }, bytes)
        }
        Codec::Bc1 | Codec::Bc3 => {
            let alpha = codec == Codec::Bc3;
            let bytes = chain.iter().map(|(w, h, rgba)| goat_gfx::bc::encode_image(*w, *h, rgba, alpha)).collect();
            (if alpha { GL_DXT5 } else { GL_DXT1 }, if alpha { 0x1908 } else { 0x1907 }, bytes)
        }
    };
    let mut out = KTX_ID.to_vec();
    // endianness, glType, glTypeSize, glFormat, internal, base internal (GL_RGB / GL_RGBA), size, depth, array, faces, mips, kv bytes
    for v in [0x0403_0201, 0, 1, 0, internal, base_format, t.width, t.height, 0, 0, 1, levels.len() as u32, 0] {
        out.extend_from_slice(&u32::to_le_bytes(v));
    }
    for l in &levels {
        out.extend_from_slice(&(l.len() as u32).to_le_bytes());
        out.extend_from_slice(l);
        // KTX pads each level to 4 bytes
        out.extend(std::iter::repeat_n(0u8, l.len().div_ceil(4) * 4 - l.len()));
    }
    out
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
    /// Builds: "none" ships the file as it is (no precompression for the target GPU).
    pub compression: Option<String>,
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
    fn jpeg_decodes_to_rgba() {
        let t = decode_jpeg(include_bytes!("../tests/fixtures/red.jpg")).unwrap();
        assert_eq!((t.width, t.height, t.rgba.len()), (16, 8, 16 * 8 * 4));
        let p = &t.rgba[4 * 20..4 * 21];
        assert!(p[0] > 200 && p[1] < 60 && p[2] < 60 && p[3] == 255, "{p:?}");
    }

    #[test]
    fn ktx_etc1_round_trip() {
        let t = TextureData { width: 16, height: 8, rgba: (0..128).flat_map(|i| [(i * 2) as u8, 90, 200, 255]).collect(), ..TextureData::solid(goat_core::Color::WHITE) }.with_mipmaps();
        let ktx = encode_ktx_etc1(&t);
        let back = decode_ktx(&ktx).unwrap();
        assert_eq!((back.width, back.height, back.format, back.mipmaps), (16, 8, TextureFormat::Etc1, true));
        let c = back.compressed.as_ref().unwrap();
        // 16×8, 8×4, 4×2, 2×1 (the chain stops when a side can't halve; blocks padded to 4×4)
        assert_eq!(c.levels.iter().map(|l| l.len()).collect::<Vec<_>>(), vec![8, 2, 1, 1]);
        assert!((back.rgba[4 * 40 + 1] as i32 - 90).abs() < 16 && (back.rgba[4 * 40 + 2] as i32 - 200).abs() < 16);
        // re-encoding keeps the blocks (no second compression)
        assert_eq!(decode_ktx(&encode_ktx_etc1(&back)).unwrap().compressed, back.compressed);
        assert!(decode_ktx(b"not a ktx file at all, definitely not, no no no no no no no no no").is_err());
    }

    #[test]
    fn ktx_rgba8() {
        let mut f = KTX_ID.to_vec();
        for v in [0x0403_0201u32, 0x1401, 1, 0x1908, GL_RGBA8, 0x1908, 2, 1, 0, 0, 1, 1, 0, 8] {
            f.extend_from_slice(&v.to_le_bytes());
        }
        f.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let t = decode_ktx(&f).unwrap();
        assert_eq!((t.width, t.height, t.rgba.clone(), t.compressed.is_none()), (2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8], true));
    }

    #[test]
    fn power_of_two_resize() {
        let t = TextureData { width: 100, height: 30, rgba: vec![200; 100 * 30 * 4], ..TextureData::solid(goat_core::Color::WHITE) };
        let p = to_power_of_two(&t);
        assert_eq!((p.width, p.height), (128, 32));
        assert!(p.rgba.iter().all(|&v| v == 200));
    }
}
