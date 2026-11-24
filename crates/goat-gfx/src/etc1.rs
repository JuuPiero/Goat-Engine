//! ETC1 block compression (4 bits per pixel), shared by the importers (KTX, precompressed builds)
//! and the 3DS backend (which stores it natively; desktop GPUs get the decoded pixels).
//!
//! Blocks are standard ETC1 64-bit values (the KTX / OpenGL layout read as a big-endian word).
//! An image is a list of blocks in raster order (rows of 4×4 blocks, top to bottom). The ETC1A4
//! variant interleaves a 64-bit block of 4-bit alphas before each color block; alpha nibble
//! `x * 4 + y` = pixel (x, y), like the PICA200.
//!
//! The encoder tries both sub-block layouts (2×4 / 4×2), differential and individual base
//! colors, and per sub-block the intensity table + pixel indices with the least squared error.

/// ETC1 intensity modifier tables (index order: +small, +large, -small, -large).
const TABLES: [[i32; 4]; 8] = [[2, 8, -2, -8], [5, 17, -5, -17], [9, 29, -9, -29], [13, 42, -13, -42], [18, 60, -18, -60], [24, 80, -24, -80], [33, 106, -33, -106], [47, 183, -47, -183]];

/// Precompressed ETC1 mip levels of a [`crate::TextureData`] (level 0 first, raster block order).
#[derive(Clone, Debug, PartialEq)]
pub struct Etc1Levels {
    /// ETC1A4: an alpha block before each color block.
    pub alpha: bool,
    pub levels: Vec<Vec<u64>>,
}

fn clamp8(v: i32) -> i32 {
    v.clamp(0, 255)
}

/// Pixels of a sub-block: (x, y) pairs.
fn sub_pixels(flip: bool, second: bool) -> [(usize, usize); 8] {
    let mut out = [(0, 0); 8];
    let mut k = 0;
    for y in 0..4 {
        for x in 0..4 {
            let in_second = if flip { y >= 2 } else { x >= 2 };
            if in_second == second {
                out[k] = (x, y);
                k += 1;
            }
        }
    }
    out
}

/// Best table + indices for pixels around a base color: (error, table, indices per pixel).
fn fit(block: &[[i32; 3]; 16], pixels: &[(usize, usize); 8], base: [i32; 3]) -> (i64, u32, [u32; 8]) {
    let mut best = (i64::MAX, 0, [0; 8]);
    for (t, tab) in TABLES.iter().enumerate() {
        let mut err = 0i64;
        let mut idx = [0u32; 8];
        for (k, &(x, y)) in pixels.iter().enumerate() {
            let p = block[y * 4 + x];
            let mut be = i64::MAX;
            for (m, &d) in tab.iter().enumerate() {
                let e: i64 = (0..3).map(|c| ((clamp8(base[c] + d) - p[c]) as i64).pow(2)).sum();
                if e < be {
                    be = e;
                    idx[k] = m as u32;
                }
            }
            err += be;
            if err >= best.0 {
                break;
            }
        }
        if err < best.0 {
            best = (err, t as u32, idx);
        }
    }
    best
}

fn avg(block: &[[i32; 3]; 16], pixels: &[(usize, usize); 8]) -> [f32; 3] {
    let mut s = [0f32; 3];
    for &(x, y) in pixels {
        for c in 0..3 {
            s[c] += block[y * 4 + x][c] as f32;
        }
    }
    s.map(|v| v / 8.0)
}

/// Encodes a 4×4 RGB block (row-major, y down) to an ETC1 block.
pub fn encode_block(block: &[[i32; 3]; 16]) -> u64 {
    let mut best: (i64, u64) = (i64::MAX, 0);
    for flip in [false, true] {
        let (p1, p2) = (sub_pixels(flip, false), sub_pixels(flip, true));
        let (a1, a2) = (avg(block, &p1), avg(block, &p2));
        let mut candidates: [Option<([i32; 3], [i32; 3], u64)>; 2] = [None, None];
        // differential: 5-bit base + 3-bit signed delta
        let q5 = |v: f32| ((v * 31.0 / 255.0).round() as i32).clamp(0, 31);
        let e5 = |q: i32| (q << 3) | (q >> 2);
        let (c1, c2) = (a1.map(q5), a2.map(q5));
        let delta = [c2[0] - c1[0], c2[1] - c1[1], c2[2] - c1[2]];
        if delta.iter().all(|d| (-4..=3).contains(d)) {
            let bits = ((c1[0] as u64) << 59) | (((delta[0] & 7) as u64) << 56) | ((c1[1] as u64) << 51) | (((delta[1] & 7) as u64) << 48) | ((c1[2] as u64) << 43) | (((delta[2] & 7) as u64) << 40) | (1 << 33);
            candidates[0] = Some((c1.map(e5), c2.map(e5), bits));
        }
        // individual: two 4-bit colors
        let q4 = |v: f32| ((v * 15.0 / 255.0).round() as i32).clamp(0, 15);
        let (d1, d2) = (a1.map(q4), a2.map(q4));
        let bits = ((d1[0] as u64) << 60) | ((d2[0] as u64) << 56) | ((d1[1] as u64) << 52) | ((d2[1] as u64) << 48) | ((d1[2] as u64) << 44) | ((d2[2] as u64) << 40);
        candidates[1] = Some((d1.map(|q| q * 17), d2.map(|q| q * 17), bits));
        for (b1, b2, head) in candidates.into_iter().flatten() {
            let (e1, t1, i1) = fit(block, &p1, b1);
            let (e2, t2, i2) = fit(block, &p2, b2);
            let err = e1.saturating_add(e2);
            if err < best.0 {
                let mut v = head | ((t1 as u64) << 37) | ((t2 as u64) << 34) | ((flip as u64) << 32);
                for (pix, idx) in [(&p1, &i1), (&p2, &i2)] {
                    for (k, &(x, y)) in pix.iter().enumerate() {
                        let i = x * 4 + y;
                        v |= (((idx[k] >> 1) & 1) as u64) << (16 + i);
                        v |= ((idx[k] & 1) as u64) << i;
                    }
                }
                best = (err, v);
            }
        }
    }
    best.1
}

/// Decodes an ETC1 block: 16 RGB pixels, row-major (y down).
pub fn decode_block(v: u64) -> [[i32; 3]; 16] {
    let diff = (v >> 33) & 1 == 1;
    let flip = (v >> 32) & 1 == 1;
    let (b1, b2) = if diff {
        let base = |s: u32| ((v >> s) & 31) as i32;
        let d = |s: u32| {
            let x = ((v >> s) & 7) as i32;
            if x >= 4 {
                x - 8
            } else {
                x
            }
        };
        let c1 = [base(59), base(51), base(43)];
        let c2 = [c1[0] + d(56), c1[1] + d(48), c1[2] + d(40)];
        let e = |q: i32| (q << 3) | (q >> 2);
        (c1.map(e), c2.map(e))
    } else {
        let n = |s: u32| (((v >> s) & 15) as i32) * 17;
        ([n(60), n(52), n(44)], [n(56), n(48), n(40)])
    };
    let (t1, t2) = (((v >> 37) & 7) as usize, ((v >> 34) & 7) as usize);
    let mut out = [[0; 3]; 16];
    for y in 0..4 {
        for x in 0..4 {
            let second = if flip { y >= 2 } else { x >= 2 };
            let (base, t) = if second { (b2, t2) } else { (b1, t1) };
            let i = x * 4 + y;
            let m = ((((v >> (16 + i)) & 1) << 1) | ((v >> i) & 1)) as usize;
            out[y * 4 + x] = base.map(|c| clamp8(c + TABLES[t][m]));
        }
    }
    out
}

/// The same block upside down, or `None` when it can't be expressed exactly (a differential
/// block split top / bottom whose delta is -4: swapping the halves would need +4).
pub fn flip_block_vertical(v: u64) -> Option<u64> {
    let mut out = v & !0xFFFF_FFFF;
    // pixel (x, y) → (x, 3 - y): index bits
    for x in 0..4 {
        for y in 0..4 {
            let (from, to) = (x * 4 + y, x * 4 + (3 - y));
            out |= ((v >> from) & 1) << to;
            out |= ((v >> (16 + from)) & 1) << (16 + to);
        }
    }
    if (v >> 32) & 1 == 1 {
        // top / bottom halves: swap their colors and tables
        let (t1, t2) = ((v >> 37) & 7, (v >> 34) & 7);
        out = (out & !(0x3F << 34)) | (t2 << 37) | (t1 << 34);
        if (v >> 33) & 1 == 1 {
            let mut colors = 0u64;
            for s in [59u32, 51, 43] {
                let base = ((v >> s) & 31) as i32;
                let raw = ((v >> (s - 3)) & 7) as i32;
                let d = if raw >= 4 { raw - 8 } else { raw };
                if d == -4 {
                    return None;
                }
                colors |= (((base + d) as u64) << s) | ((((-d) & 7) as u64) << (s - 3));
            }
            out = (out & !(0xFFFFFF << 40)) | colors;
        } else {
            let mut colors = 0u64;
            for s in [60u32, 52, 44] {
                colors |= ((v >> (s - 4)) & 15) << s | ((v >> s) & 15) << (s - 4);
            }
            out = (out & !(0xFFFFFF << 40)) | colors;
        }
    }
    Some(out)
}

/// A 4-bit alpha block upside down.
pub fn flip_alpha_vertical(a: u64) -> u64 {
    let mut out = 0;
    for x in 0..4 {
        for y in 0..4 {
            out |= ((a >> (4 * (x * 4 + y))) & 15) << (4 * (x * 4 + 3 - y));
        }
    }
    out
}

fn blocks_of(n: u32) -> usize {
    (n as usize).div_ceil(4)
}

/// RGBA8 pixels (rows top to bottom) → blocks in raster order (edges padded by repetition).
pub fn encode_image(width: u32, height: u32, rgba: &[u8], alpha: bool) -> Vec<u64> {
    let (w, h) = (width as usize, height as usize);
    let (bw, bh) = (blocks_of(width), blocks_of(height));
    let mut out = Vec::with_capacity(bw * bh * if alpha { 2 } else { 1 });
    for by in 0..bh {
        for bx in 0..bw {
            let mut block = [[0i32; 3]; 16];
            let mut a = 0u64;
            for y in 0..4 {
                for x in 0..4 {
                    let (px, py) = ((bx * 4 + x).min(w - 1), (by * 4 + y).min(h - 1));
                    let o = (py * w + px) * 4;
                    block[y * 4 + x] = [rgba[o] as i32, rgba[o + 1] as i32, rgba[o + 2] as i32];
                    a |= (((rgba[o + 3] as u32 * 15 + 127) / 255) as u64) << (4 * (x * 4 + y));
                }
            }
            if alpha {
                out.push(a);
            }
            out.push(encode_block(&block));
        }
    }
    out
}

/// Blocks in raster order → RGBA8 pixels (rows top to bottom).
pub fn decode_image(width: u32, height: u32, blocks: &[u64], alpha: bool) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let bw = blocks_of(width);
    let step = if alpha { 2 } else { 1 };
    let mut out = vec![255u8; w * h * 4];
    for (n, chunk) in blocks.chunks_exact(step).enumerate() {
        let (bx, by) = (n % bw, n / bw);
        let px = decode_block(chunk[step - 1]);
        for y in 0..4 {
            for x in 0..4 {
                let (ix, iy) = (bx * 4 + x, by * 4 + y);
                if ix >= w || iy >= h {
                    continue;
                }
                let o = (iy * w + ix) * 4;
                let p = px[y * 4 + x];
                out[o..o + 3].copy_from_slice(&[p[0] as u8, p[1] as u8, p[2] as u8]);
                if alpha {
                    out[o + 3] = (((chunk[0] >> (4 * (x * 4 + y))) & 15) * 17) as u8;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn psnr(a: &[[i32; 3]; 16], b: &[[i32; 3]; 16]) -> f64 {
        let mse: f64 = a.iter().zip(b).map(|(p, q)| (0..3).map(|c| ((p[c] - q[c]) as f64).powi(2)).sum::<f64>()).sum::<f64>() / 48.0;
        if mse == 0.0 {
            99.0
        } else {
            10.0 * (255.0f64 * 255.0 / mse).log10()
        }
    }

    fn samples() -> Vec<[[i32; 3]; 16]> {
        let flat = [[200, 40, 90]; 16];
        // ETC1 shifts all channels together: luminance gradients are its good case
        let grad: [[i32; 3]; 16] = std::array::from_fn(|i| {
            let v = 40 + (i % 4 + i / 4) as i32 * 15;
            [v + 30, v, v / 2]
        });
        let split: [[i32; 3]; 16] = std::array::from_fn(|i| if i % 4 < 2 { [250, 10, 10] } else { [10, 10, 250] });
        let top_bottom: [[i32; 3]; 16] = std::array::from_fn(|i| if i < 8 { [30, 200, 60] } else { [60, 190, 90] });
        vec![flat, grad, split, top_bottom]
    }

    #[test]
    fn blocks_round_trip_with_good_quality() {
        for b in samples() {
            let d = decode_block(encode_block(&b));
            assert!(psnr(&b, &d) > 28.0, "{b:?} -> {d:?}: {}", psnr(&b, &d));
        }
        let split = samples()[2];
        let d = decode_block(encode_block(&split));
        assert!(d[0][0] > 200 && d[3][2] > 200);
    }

    #[test]
    fn vertical_flip_is_exact() {
        for b in samples() {
            let v = encode_block(&b);
            let d = decode_block(v);
            let f = decode_block(flip_block_vertical(v).unwrap());
            for y in 0..4 {
                for x in 0..4 {
                    assert_eq!(f[y * 4 + x], d[(3 - y) * 4 + x]);
                }
            }
        }
        let a = 0x0123_4567_89AB_CDEFu64;
        assert_eq!(flip_alpha_vertical(flip_alpha_vertical(a)), a);
    }

    #[test]
    fn images_round_trip() {
        let (w, h) = (8u32, 6u32);
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i * 5) as u8, 100, 50, if i % 2 == 0 { 255 } else { 0 }]).collect();
        let blocks = encode_image(w, h, &rgba, true);
        assert_eq!(blocks.len(), 2 * 2 * 2);
        let back = decode_image(w, h, &blocks, true);
        assert_eq!(back.len(), rgba.len());
        assert_eq!((back[3], back[7]), (255, 0));
        assert!((back[4 * 9] as i32 - rgba[4 * 9] as i32).abs() < 24);
    }
}
