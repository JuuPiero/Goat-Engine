//! BC1 / BC3 (DXT1 / DXT5) block compression for desktop GPUs: what desktop builds store so
//! textures upload compressed (4 / 8 bits per pixel instead of 32), and a decoder for GPUs or
//! tools that can't sample them.
//!
//! The encoder fits each 4×4 block's colors with the box around them shrunk a little toward its
//! center (stb_dxt's quick mode), then picks the nearest of the 4 palette colors per pixel.
//! BC3 adds an 8-step alpha ramp per block.

/// Precompressed BC levels of a texture (level 0 first).
#[derive(Clone, Debug, PartialEq)]
pub struct BcLevels {
    /// BC3 (with alpha) instead of BC1.
    pub alpha: bool,
    pub levels: Vec<Vec<u8>>,
}

impl BcLevels {
    pub fn block_bytes(&self) -> usize {
        if self.alpha {
            16
        } else {
            8
        }
    }
}

fn to565(c: [f32; 3]) -> u16 {
    let r = (c[0].clamp(0.0, 255.0) * 31.0 / 255.0 + 0.5) as u16;
    let g = (c[1].clamp(0.0, 255.0) * 63.0 / 255.0 + 0.5) as u16;
    let b = (c[2].clamp(0.0, 255.0) * 31.0 / 255.0 + 0.5) as u16;
    (r.min(31) << 11) | (g.min(63) << 5) | b.min(31)
}

fn from565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [((r << 3) | (r >> 2)) as u8, ((g << 2) | (g >> 4)) as u8, ((b << 3) | (b >> 2)) as u8]
}

/// The 4 colors of a BC1 block in 4-color mode (c0 > c1, and always in BC3) or 3-color + black.
fn palette(c0: u16, c1: u16) -> [[u8; 4]; 4] {
    palette_mode(c0, c1, c0 > c1)
}

fn palette_mode(c0: u16, c1: u16, four: bool) -> [[u8; 4]; 4] {
    let (a, b) = (from565(c0), from565(c1));
    let mix = |wa: u32, wb: u32, d: u32| -> [u8; 4] { [((a[0] as u32 * wa + b[0] as u32 * wb) / d) as u8, ((a[1] as u32 * wa + b[1] as u32 * wb) / d) as u8, ((a[2] as u32 * wa + b[2] as u32 * wb) / d) as u8, 255] };
    if four {
        [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], mix(2, 1, 3), mix(1, 2, 3)]
    } else {
        [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], mix(1, 1, 2), [0, 0, 0, 0]]
    }
}

/// One 4×4 block (16 RGBA pixels, row by row) as BC1 (8 bytes, 4-color mode).
pub fn encode_bc1_block(px: &[[u8; 4]; 16]) -> [u8; 8] {
    // endpoints along the colors' principal axis (power iteration on the covariance)
    let mut mean = [0.0f32; 3];
    for p in px {
        for k in 0..3 {
            mean[k] += p[k] as f32 / 16.0;
        }
    }
    let mut cov = [[0.0f32; 3]; 3];
    for p in px {
        let d = [p[0] as f32 - mean[0], p[1] as f32 - mean[1], p[2] as f32 - mean[2]];
        for a in 0..3 {
            for b in 0..3 {
                cov[a][b] += d[a] * d[b];
            }
        }
    }
    let mut axis = [1.0f32, 1.0, 1.0];
    for _ in 0..8 {
        let n = [0, 1, 2].map(|a| cov[a][0] * axis[0] + cov[a][1] * axis[1] + cov[a][2] * axis[2]);
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len < 1e-6 {
            break;
        }
        axis = n.map(|x| x / len);
    }
    let (mut tmin, mut tmax) = (f32::MAX, f32::MIN);
    for p in px {
        let t = (0..3).map(|k| (p[k] as f32 - mean[k]) * axis[k]).sum::<f32>();
        tmin = tmin.min(t);
        tmax = tmax.max(t);
    }
    // shrink the range by 1/16 (less error from the endpoints' rounding)
    let inset = (tmax - tmin) / 16.0;
    let (tmin, tmax) = (tmin + inset, tmax - inset);
    let lo = [0, 1, 2].map(|k| mean[k] + axis[k] * tmin);
    let hi = [0, 1, 2].map(|k| mean[k] + axis[k] * tmax);    let (mut c0, mut c1) = (to565(hi), to565(lo));
    if c0 < c1 {
        std::mem::swap(&mut c0, &mut c1);
    }
    let mut indices = 0u32;
    if c0 != c1 {
        let pal = palette(c0, c1);
        for (i, p) in px.iter().enumerate() {
            let mut best = (u32::MAX, 0u32);
            for (j, q) in pal.iter().enumerate() {
                let d: u32 = (0..3).map(|k| (p[k] as i32 - q[k] as i32).unsigned_abs().pow(2)).sum();
                if d < best.0 {
                    best = (d, j as u32);
                }
            }
            indices |= best.1 << (i * 2);
        }
    }
    let mut out = [0u8; 8];
    out[0..2].copy_from_slice(&c0.to_le_bytes());
    out[2..4].copy_from_slice(&c1.to_le_bytes());
    out[4..8].copy_from_slice(&indices.to_le_bytes());
    out
}

/// One block as BC3 (16 bytes): an 8-step alpha ramp + a BC1 color block.
pub fn encode_bc3_block(px: &[[u8; 4]; 16]) -> [u8; 16] {
    let (mut a0, mut a1) = (0u8, 255u8);
    for p in px {
        a0 = a0.max(p[3]);
        a1 = a1.min(p[3]);
    }
    let mut out = [0u8; 16];
    out[0] = a0;
    out[1] = a1;
    let ramp: [u8; 8] = if a0 > a1 {
        let (x, y) = (a0 as u32, a1 as u32);
        [a0, a1, ((6 * x + y) / 7) as u8, ((5 * x + 2 * y) / 7) as u8, ((4 * x + 3 * y) / 7) as u8, ((3 * x + 4 * y) / 7) as u8, ((2 * x + 5 * y) / 7) as u8, ((x + 6 * y) / 7) as u8]
    } else {
        [a0; 8]
    };
    let mut bits = 0u64;
    for (i, p) in px.iter().enumerate() {
        let j = (0..8).min_by_key(|&j| (p[3] as i32 - ramp[j] as i32).abs()).unwrap_or(0) as u64;
        bits |= j << (i * 3);
    }
    out[2..8].copy_from_slice(&bits.to_le_bytes()[..6]);
    // color in 4-color mode regardless of alpha
    let opaque: [[u8; 4]; 16] = std::array::from_fn(|i| [px[i][0], px[i][1], px[i][2], 255]);
    out[8..16].copy_from_slice(&encode_bc1_block(&opaque));
    out
}

/// The 4×4 block at block (bx, by) of an image (edges repeat the last row / column).
fn block(w: u32, h: u32, rgba: &[u8], bx: u32, by: u32) -> [[u8; 4]; 16] {
    std::array::from_fn(|i| {
        let x = (bx * 4 + (i as u32 % 4)).min(w - 1);
        let y = (by * 4 + (i as u32 / 4)).min(h - 1);
        let o = ((y * w + x) * 4) as usize;
        [rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3]]
    })
}

/// A whole image (row-major blocks).
pub fn encode_image(w: u32, h: u32, rgba: &[u8], alpha: bool) -> Vec<u8> {
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let mut out = Vec::with_capacity((bw * bh) as usize * if alpha { 16 } else { 8 });
    for by in 0..bh {
        for bx in 0..bw {
            let b = block(w, h, rgba, bx, by);
            if alpha {
                out.extend_from_slice(&encode_bc3_block(&b));
            } else {
                out.extend_from_slice(&encode_bc1_block(&b));
            }
        }
    }
    out
}

/// Back to RGBA8 pixels.
pub fn decode_image(w: u32, h: u32, data: &[u8], alpha: bool) -> Vec<u8> {
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let step = if alpha { 16 } else { 8 };
    let mut out = vec![0u8; (w * h * 4) as usize];
    for by in 0..bh {
        for bx in 0..bw {
            let at = ((by * bw + bx) as usize) * step;
            let Some(b) = data.get(at..at + step) else { return out };
            let (ab, cb) = if alpha { (Some(&b[0..8]), &b[8..16]) } else { (None, &b[0..8]) };
            let c0 = u16::from_le_bytes([cb[0], cb[1]]);
            let c1 = u16::from_le_bytes([cb[2], cb[3]]);
            let pal = palette_mode(c0, c1, alpha || c0 > c1);
            let idx = u32::from_le_bytes([cb[4], cb[5], cb[6], cb[7]]);
            let alphas: Option<[u8; 16]> = ab.map(|a| {
                let (a0, a1) = (a[0] as u32, a[1] as u32);
                let ramp: [u8; 8] = if a0 > a1 {
                    std::array::from_fn(|j| match j {
                        0 => a0 as u8,
                        1 => a1 as u8,
                        _ => (((8 - j as u32) * a0 + (j as u32 - 1) * a1) / 7) as u8,
                    })
                } else {
                    std::array::from_fn(|j| match j {
                        0 => a0 as u8,
                        1 => a1 as u8,
                        6 => 0,
                        7 => 255,
                        _ => (((6 - j as u32) * a0 + (j as u32 - 1) * a1) / 5) as u8,
                    })
                };
                let mut bits = [0u8; 8];
                bits[..6].copy_from_slice(&a[2..8]);
                let bits = u64::from_le_bytes(bits);
                std::array::from_fn(|i| ramp[((bits >> (i * 3)) & 7) as usize])
            });
            for i in 0..16u32 {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                if x >= w || y >= h {
                    continue;
                }
                let mut c = pal[((idx >> (i * 2)) & 3) as usize];
                if let Some(a) = alphas {
                    c[3] = a[i as usize];
                }
                let o = ((y * w + x) * 4) as usize;
                out[o..o + 4].copy_from_slice(&c);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc1_and_bc3_round_trip_closely() {
        // a gradient with a soft alpha ramp
        let (w, h) = (16u32, 12u32);
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[(x * 16) as u8, (y * 20) as u8, 128, (x * 16 + y) as u8]);
            }
        }
        for alpha in [false, true] {
            let enc = encode_image(w, h, &rgba, alpha);
            assert_eq!(enc.len(), (w / 4 * h / 4) as usize * if alpha { 16 } else { 8 });
            let dec = decode_image(w, h, &enc, alpha);
            let (mut sum, mut n) = (0u64, 0u64);
            for (i, (a, b)) in rgba.iter().zip(&dec).enumerate() {
                if i % 4 == 3 && !alpha {
                    assert_eq!(*b, 255);
                    continue;
                }
                sum += (*a as i32 - *b as i32).unsigned_abs() as u64;
                n += 1;
            }
            // colors varying in two directions inside a block don't fit BC1's line: mean error
            assert!(sum / n <= 8, "alpha {alpha}: mean error {}", sum / n);
        }
        // a 1-D gradient fits the line: small max error
        let grad: Vec<u8> = (0..64u32).flat_map(|i| [(i * 4) as u8, (255 - i * 4) as u8, (i * 2) as u8, 255]).collect();
        let dec = decode_image(8, 8, &encode_image(8, 8, &grad, false), false);
        let max = grad.iter().zip(&dec).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
        assert!(max <= 12, "gradient max error {max}");
    }

    #[test]
    fn flat_blocks_are_exact() {
        let px = [[200u8, 100, 50, 255]; 16];
        let dec = decode_image(4, 4, &encode_bc1_block(&px), false);
        let want = from565(to565([200.0, 100.0, 50.0]));
        assert_eq!(&dec[0..3], &want);
    }
}
