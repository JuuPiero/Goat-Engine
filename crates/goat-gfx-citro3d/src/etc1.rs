//! ETC1 / ETC1A4 in the PICA200's memory layout (codec: [`goat_gfx::etc1`]).
//!
//! The PICA reads the image bottom-up (first row in memory = v 0), 8×8 tiles in row order, each
//! tile = 4 blocks in Z order (bottom-left, bottom-right, top-left, top-right in image terms),
//! each 64-bit block little-endian; ETC1A4 puts the 4-bit alpha block before each color block.
//! Blocks are therefore upside down compared to a top-down image: encoded from flipped pixels,
//! or flipped ([`flip_block_vertical`]) when they come precompressed.

use goat_gfx::etc1::{decode_block, encode_block, flip_alpha_vertical, flip_block_vertical};

/// Calls `f(pica_x, pica_y)` for each 4×4 block in PICA order (block coordinates, y from the bottom).
fn for_each_block(w: usize, h: usize, mut f: impl FnMut(usize, usize)) {
    for ty in (0..h / 4).step_by(2) {
        for tx in (0..w / 4).step_by(2) {
            for (bx, by) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                f(tx + bx, ty + by);
            }
        }
    }
}

/// RGBA8 pixels (rows top to bottom) → PICA ETC1 / ETC1A4 data. Sizes must be multiples of 8.
pub fn encode_pica(width: u32, height: u32, rgba: &[u8], with_alpha: bool) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut out = Vec::with_capacity(w * h / 2 * if with_alpha { 2 } else { 1 });
    for_each_block(w, h, |bx, by| {
        let mut block = [[0i32; 3]; 16];
        let mut alpha = 0u64;
        for y in 0..4 {
            for x in 0..4 {
                // PICA row y counts from the bottom of the image
                let o = ((h - 1 - (by * 4 + y)) * w + bx * 4 + x) * 4;
                block[y * 4 + x] = [rgba[o] as i32, rgba[o + 1] as i32, rgba[o + 2] as i32];
                alpha |= (((rgba[o + 3] as u32 * 15 + 127) / 255) as u64) << (4 * (x * 4 + y));
            }
        }
        if with_alpha {
            out.extend_from_slice(&alpha.to_le_bytes());
        }
        out.extend_from_slice(&encode_block(&block).to_le_bytes());
    });
    out
}

/// Precompressed blocks (raster order, top-down image; alpha blocks interleaved when
/// `with_alpha`) → PICA data. Sizes must be multiples of 8.
pub fn from_blocks(width: u32, height: u32, blocks: &[u64], with_alpha: bool) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let (bw, bh) = (w / 4, h / 4);
    let step = if with_alpha { 2 } else { 1 };
    let mut out = Vec::with_capacity(blocks.len() * 8);
    for_each_block(w, h, |bx, by| {
        let at = ((bh - 1 - by) * bw + bx) * step;
        if with_alpha {
            out.extend_from_slice(&flip_alpha_vertical(blocks[at]).to_le_bytes());
        }
        let color = blocks[at + step - 1];
        let flipped = flip_block_vertical(color).unwrap_or_else(|| {
            // rare: re-encode the decoded block upside down
            let p = decode_block(color);
            encode_block(&std::array::from_fn(|i| p[(3 - i / 4) * 4 + i % 4]))
        });
        out.extend_from_slice(&flipped.to_le_bytes());
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pica_layout_sizes() {
        let rgba = [255u8, 0, 0, 128].repeat(16 * 8);
        assert_eq!(encode_pica(16, 8, &rgba, false).len(), 16 * 8 / 2);
        let a = encode_pica(16, 8, &rgba, true);
        assert_eq!(a.len(), 16 * 8);
        // alpha 128 → 8 in every nibble of the first alpha block
        assert_eq!(u64::from_le_bytes(a[0..8].try_into().unwrap()), 0x8888_8888_8888_8888);
    }

    #[test]
    fn precompressed_blocks_match_direct_encoding_layout() {
        // a top-down image: white top half, dark bottom half, alpha 0 in the top-left pixel
        let (w, h) = (8u32, 8u32);
        let mut rgba: Vec<u8> = (0..64).flat_map(|i| if i < 32 { [250, 250, 250, 255] } else { [20, 20, 20, 255] }).collect();
        rgba[3] = 0;
        let direct = encode_pica(w, h, &rgba, true);
        let pre = from_blocks(w, h, &goat_gfx::etc1::encode_image(w, h, &rgba, true), true);
        assert_eq!(direct.len(), pre.len());
        // decode both: same pixels (first PICA block = bottom-left = dark; its alpha all opaque)
        for buf in [&direct, &pre] {
            let block = |k: usize| u64::from_le_bytes(buf[k * 8..k * 8 + 8].try_into().unwrap());
            assert_eq!(block(0), 0xFFFF_FFFF_FFFF_FFFF);
            assert!(decode_block(block(1))[0][0] < 40);
            // third block = top-left: bright, and its alpha has the transparent pixel at PICA (0, 3)
            assert!(decode_block(block(5))[0][0] > 230);
            assert_eq!((block(4) >> (4 * 3)) & 15, 0);
        }
    }
}
