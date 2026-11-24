//! Nintendo 3DS graphics backend (PICA200 through citro3d).
//!
//! Built to look as good as the hardware allows, cheaply:
//! - **per-pixel lighting in hardware**: the PICA200's fragment lighting (diffuse + Blinn-Phong
//!   specular from a lookup table per shininess), fed by a vertex shader that outputs the normal as
//!   a quaternion (`shaders/standard.v.pica`);
//! - **2×2 supersampling** on the top screen (rendered at 800×480, downscaled by the display
//!   transfer) when the 3D slider is off, **stereoscopic 3D** (one target per eye) when it is on;
//! - vertex / index buffers in linear memory (read by the GPU directly), textures tiled (Morton
//!   8×8) once at load, opaque draws first then blended ones back to front.
//!
//! Everything unsafe (FFI) lives in this crate; its API is the safe [`GraphicsBackend`] trait.
//! On other targets the crate is empty.

#[cfg(target_os = "horizon")]
mod pica;

#[cfg(target_os = "horizon")]
pub use pica::Citro3dBackend;

/// Bytes per texel of a storage format on the PICA200.
pub fn bytes_per_texel(format: goat_gfx::TextureFormat) -> usize {
    use goat_gfx::TextureFormat::*;
    match format {
        Rgba8 => 4,
        Rgb565 | Rgba4 | Rgba5551 => 2,
    }
}

/// One RGBA8 pixel in a PICA200 texel format (little-endian bytes as the GPU reads them).
fn encode_texel(format: goat_gfx::TextureFormat, [r, g, b, a]: [u8; 4], out: &mut [u8]) {
    use goat_gfx::TextureFormat::*;
    // round to n bits
    let q = |v: u8, bits: u32| ((v as u32 * ((1 << bits) - 1) + 127) / 255) as u16;
    match format {
        Rgba8 => out.copy_from_slice(&[a, b, g, r]),
        Rgb565 => out.copy_from_slice(&((q(r, 5) << 11) | (q(g, 6) << 5) | q(b, 5)).to_le_bytes()),
        Rgba4 => out.copy_from_slice(&((q(r, 4) << 12) | (q(g, 4) << 8) | (q(b, 4) << 4) | q(a, 4)).to_le_bytes()),
        Rgba5551 => out.copy_from_slice(&((q(r, 5) << 11) | (q(g, 5) << 6) | (q(b, 5) << 1) | (a >= 128) as u16).to_le_bytes()),
    }
}

/// Converts RGBA8 pixels (rows top to bottom) into the PICA200 layout of `format`: rows flipped
/// (bottom-up), 8×8 tiles in
/// row order, Morton order inside a tile. Width / height must be multiples of 8.
pub fn tile_texture(width: u32, height: u32, rgba: &[u8], format: goat_gfx::TextureFormat) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let bpp = bytes_per_texel(format);
    let mut out = vec![0u8; w * h * bpp];
    for y in 0..h {
        for x in 0..w {
            let tile = (y / 8) * (w / 8) + (x / 8);
            let (tx, ty) = (x & 7, y & 7);
            let morton = (tx & 1) | ((ty & 1) << 1) | ((tx & 2) << 1) | ((ty & 2) << 2) | ((tx & 4) << 2) | ((ty & 4) << 3);
            let dst = (tile * 64 + morton) * bpp;
            // PICA textures are bottom-up: the first row in memory is v = 0 (the bottom)
            let src = ((h - 1 - y) * w + x) * 4;
            encode_texel(format, [rgba[src], rgba[src + 1], rgba[src + 2], rgba[src + 3]], &mut out[dst..dst + bpp]);
        }
    }
    out
}

/// [`tile_texture`] in RGBA8 (bytes A B G R).
pub fn tile_rgba8(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    tile_texture(width, height, rgba, goat_gfx::TextureFormat::Rgba8)
}

#[cfg(test)]
mod tests {
    #[test]
    fn tiling_puts_pixels_in_morton_order() {
        // 8×8 image, pixel (x, y) = [x, y, 0, 255]
        let mut rgba = Vec::new();
        for y in 0..8u8 {
            for x in 0..8u8 {
                rgba.extend_from_slice(&[x, y, 0, 255]);
            }
        }
        let t = super::tile_rgba8(8, 8, &rgba);
        // memory starts at the bottom row (y = 7): index 1 = (1, 7), 2 = (0, 6), 3 = (1, 6); bytes A B G R
        assert_eq!(&t[4..8], &[255, 0, 7, 1]);
        assert_eq!(&t[8..12], &[255, 0, 6, 0]);
        assert_eq!(&t[12..16], &[255, 0, 6, 1]);
    }

    #[test]
    fn sixteen_bit_formats() {
        use goat_gfx::TextureFormat::*;
        let red = [255u8, 0, 0, 255].repeat(64);
        assert_eq!(&super::tile_texture(8, 8, &red, Rgb565)[0..2], &0xF800u16.to_le_bytes());
        assert_eq!(&super::tile_texture(8, 8, &red, Rgba4)[0..2], &0xF00Fu16.to_le_bytes());
        assert_eq!(&super::tile_texture(8, 8, &red, Rgba5551)[0..2], &0xF801u16.to_le_bytes());
    }
}
