//! ETC1 encoding for the PICA200 (4 bits per pixel: a quarter of RGB565's memory).
//!
//! A straightforward quality encoder: for each 4×4 block, both sub-block layouts (2×4 / 4×2),
//! differential or individual base colors, and per sub-block the intensity table and pixel
//! indices with the least squared error. Standard ETC1 bit layout; the PICA stores each 64-bit
//! block little-endian, blocks in 8×8 tiles (4 blocks in Z order), image bottom-up. ETC1A4 adds
//! a 64-bit block of 4-bit alphas before each color block.

/// ETC1 intensity modifier tables (index order: +small, +large, -small, -large).
const TABLES: [[i32; 4]; 8] = [[2, 8, -2, -8], [5, 17, -5, -17], [9, 29, -9, -29], [13, 42, -13, -42], [18, 60, -18, -60], [24, 80, -24, -80], [33, 106, -33, -106], [47, 183, -47, -183]];

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

/// Encodes a 4×4 RGB block (row-major, y down) to a standard ETC1 64-bit value.
pub fn encode_block(block: &[[i32; 3]; 16]) -> u64 {
    let mut best: (i64, u64) = (i64::MAX, 0);
    for flip in [false, true] {
        let (p1, p2) = (sub_pixels(flip, false), sub_pixels(flip, true));
        let (a1, a2) = (avg(block, &p1), avg(block, &p2));
        // differential: 5-bit base + 3-bit signed delta
        let q5 = |v: f32| ((v * 31.0 / 255.0).round() as i32).clamp(0, 31);
        let e5 = |q: i32| (q << 3) | (q >> 2);
        let c1 = a1.map(q5);
        let c2 = a2.map(q5);
        let delta: [i32; 3] = [c2[0] - c1[0], c2[1] - c1[1], c2[2] - c1[2]];
        let mut candidates: Vec<(bool, [i32; 3], [i32; 3], u64)> = Vec::new();
        if delta.iter().all(|d| (-4..=3).contains(d)) {
            let bits = ((c1[0] as u64) << 59) | (((delta[0] & 7) as u64) << 56) | ((c1[1] as u64) << 51) | (((delta[1] & 7) as u64) << 48) | ((c1[2] as u64) << 43) | (((delta[2] & 7) as u64) << 40) | (1 << 33);
            candidates.push((true, c1.map(e5), c2.map(e5), bits));
        }
        // individual: two 4-bit colors
        let q4 = |v: f32| ((v * 15.0 / 255.0).round() as i32).clamp(0, 15);
        let (d1, d2) = (a1.map(q4), a2.map(q4));
        let bits = ((d1[0] as u64) << 60) | ((d2[0] as u64) << 56) | ((d1[1] as u64) << 52) | ((d2[1] as u64) << 48) | ((d1[2] as u64) << 44) | ((d2[2] as u64) << 40);
        candidates.push((false, d1.map(|q| q * 17), d2.map(|q| q * 17), bits));
        for (_, b1, b2, head) in candidates {
            let (e1, t1, i1) = fit(block, &p1, b1);
            let (e2, t2, i2) = fit(block, &p2, b2);
            let err = e1 + e2;
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

/// Decodes a standard ETC1 block (tests, tools): 16 RGB pixels, row-major.
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
            let d = TABLES[t][m];
            out[y * 4 + x] = base.map(|c| clamp8(c + d));
        }
    }
    out
}

/// RGBA8 pixels (rows top to bottom) → PICA ETC1 / ETC1A4 data. Sizes must be multiples of 8.
pub fn encode_pica(width: u32, height: u32, rgba: &[u8], with_alpha: bool) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    // the PICA's texture rows run bottom-up
    let px = |x: usize, y: usize| -> [u8; 4] {
        let o = ((h - 1 - y) * w + x) * 4;
        [rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3]]
    };
    let mut out = Vec::with_capacity(w * h / 2 * if with_alpha { 2 } else { 1 });
    for ty in (0..h).step_by(8) {
        for tx in (0..w).step_by(8) {
            for (bx, by) in [(0, 0), (4, 0), (0, 4), (4, 4)] {
                let mut block = [[0i32; 3]; 16];
                let mut alpha = 0u64;
                for y in 0..4 {
                    for x in 0..4 {
                        let p = px(tx + bx + x, ty + by + y);
                        block[y * 4 + x] = [p[0] as i32, p[1] as i32, p[2] as i32];
                        let a4 = ((p[3] as u32 * 15 + 127) / 255) as u64;
                        alpha |= a4 << (4 * (x * 4 + y));
                    }
                }
                if with_alpha {
                    out.extend_from_slice(&alpha.to_le_bytes());
                }
                out.extend_from_slice(&encode_block(&block).to_le_bytes());
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

    #[test]
    fn blocks_round_trip_with_good_quality() {
        // a flat color, a gradient and a two-color split
        let flat = [[200, 40, 90]; 16];
        // ETC1 shifts all channels together: luminance gradients are its good case
        let grad: [[i32; 3]; 16] = std::array::from_fn(|i| {
            let v = 40 + (i % 4 + i / 4) as i32 * 15;
            [v + 30, v, v / 2]
        });
        let split: [[i32; 3]; 16] = std::array::from_fn(|i| if i % 4 < 2 { [250, 10, 10] } else { [10, 10, 250] });
        for b in [flat, grad, split] {
            let d = decode_block(encode_block(&b));
            assert!(psnr(&b, &d) > 28.0, "{b:?} -> {d:?}: {}", psnr(&b, &d));
        }
        // the split's halves are found (individual or differential, left / right)
        let d = decode_block(encode_block(&split));
        assert!(d[0][0] > 200 && d[3][2] > 200);
    }

    #[test]
    fn pica_layout_sizes() {
        let rgba = [255u8, 0, 0, 128].repeat(16 * 8);
        assert_eq!(encode_pica(16, 8, &rgba, false).len(), 16 * 8 / 2);
        let a = encode_pica(16, 8, &rgba, true);
        assert_eq!(a.len(), 16 * 8);
        // alpha 128 → 8 in every nibble of the first alpha block
        assert_eq!(u64::from_le_bytes(a[0..8].try_into().unwrap()), 0x8888_8888_8888_8888);
    }
}
