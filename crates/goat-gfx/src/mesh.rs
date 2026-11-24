//! CPU-side meshes: one interleaved vertex layout every backend understands, 16-bit indices
//! (the PICA200 and mobile GPUs prefer them; split big meshes).

use goat_core::math::Vec3;

/// The engine's vertex: position, normal, uv, color. 48 bytes, `repr(C)` (uploaded as is).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

impl Vertex {
    pub const SIZE: usize = std::mem::size_of::<Vertex>();
    pub fn new(p: [f32; 3], n: [f32; 3], uv: [f32; 2]) -> Self {
        Vertex { position: p, normal: n, uv, color: [1.0; 4] }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    /// Triangle list.
    pub indices: Vec<u16>,
}

impl MeshData {
    /// Bounding sphere around the AABB of the vertices.
    pub fn bounds(&self) -> crate::Bounds {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for v in &self.vertices {
            let p = Vec3::from(v.position);
            min = min.min(p);
            max = max.max(p);
        }
        if self.vertices.is_empty() {
            return crate::Bounds { center: Vec3::ZERO, radius: 0.0, half_extents: Vec3::ZERO };
        }
        let center = (min + max) * 0.5;
        let radius = self.vertices.iter().map(|v| (Vec3::from(v.position) - center).length()).fold(0.0, f32::max);
        crate::Bounds { center, radius, half_extents: (max - min) * 0.5 }
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Raw bytes of the vertices (for uploads).
    pub fn vertex_bytes(&self) -> &[u8] {
        // SAFETY: Vertex is repr(C) plain old data (f32s only, no padding).
        unsafe { std::slice::from_raw_parts(self.vertices.as_ptr() as *const u8, self.vertices.len() * Vertex::SIZE) }
    }

    /// A cube of edge `size`, one face per direction, uvs 0..1 per face.
    pub fn cube(size: f32) -> Self {
        let h = size * 0.5;
        let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
            // normal, right, up
            ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
            ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ];
        let mut m = MeshData::default();
        for (n, r, u) in faces {
            let (n, r, u) = (Vec3::from(n), Vec3::from(r), Vec3::from(u));
            let base = m.vertices.len() as u16;
            for (sx, sy, uv) in [(-1.0, -1.0, [0.0, 1.0]), (1.0, -1.0, [1.0, 1.0]), (1.0, 1.0, [1.0, 0.0]), (-1.0, 1.0, [0.0, 0.0])] {
                let p = (n + r * sx + u * sy) * h;
                m.vertices.push(Vertex::new(p.into(), n.into(), uv));
            }
            m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        m
    }

    /// A flat square on XZ (facing +Y), `size` wide, uvs tiled `tiles` times.
    pub fn plane(size: f32, tiles: f32) -> Self {
        let h = size * 0.5;
        let n = [0.0, 1.0, 0.0];
        MeshData {
            vertices: vec![
                Vertex::new([-h, 0.0, h], n, [0.0, tiles]),
                Vertex::new([h, 0.0, h], n, [tiles, tiles]),
                Vertex::new([h, 0.0, -h], n, [tiles, 0.0]),
                Vertex::new([-h, 0.0, -h], n, [0.0, 0.0]),
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
        }
    }

    /// A UV sphere.
    pub fn sphere(radius: f32, rings: u32, segments: u32) -> Self {
        let mut m = MeshData::default();
        for r in 0..=rings {
            let v = r as f32 / rings as f32;
            let phi = v * std::f32::consts::PI;
            for s in 0..=segments {
                let u = s as f32 / segments as f32;
                let th = u * std::f32::consts::TAU;
                let n = Vec3::new(phi.sin() * th.cos(), phi.cos(), phi.sin() * th.sin());
                m.vertices.push(Vertex::new((n * radius).into(), n.into(), [u, v]));
            }
        }
        let w = segments + 1;
        for r in 0..rings {
            for s in 0..segments {
                let a = (r * w + s) as u16;
                let b = a + w as u16;
                m.indices.extend_from_slice(&[a, a + 1, b, b, a + 1, b + 1]);
            }
        }
        m
    }

    /// Unity's Capsule: `radius`, total `height` along Y (hemispheres included).
    pub fn capsule(radius: f32, height: f32, rings: u32, segments: u32) -> Self {
        let mut m = MeshData::default();
        let half = (height / 2.0 - radius).max(0.0);
        // a sphere split at the equator, the halves pushed apart; the equator row is doubled
        let rings = rings.max(2) & !1;
        let w = segments + 1;
        let mut rows = 0;
        for r in 0..=rings {
            let phi = r as f32 / rings as f32 * std::f32::consts::PI;
            let copies: &[f32] = if r == rings / 2 { &[half, -half] } else if r < rings / 2 { &[half] } else { &[-half] };
            for &off in copies {
                for s in 0..=segments {
                    let u = s as f32 / segments as f32;
                    let th = u * std::f32::consts::TAU;
                    let n = Vec3::new(phi.sin() * th.cos(), phi.cos(), phi.sin() * th.sin());
                    let p = n * radius + Vec3::new(0.0, off, 0.0);
                    let v = 0.5 - p.y / height.max(1e-6);
                    m.vertices.push(Vertex::new(p.into(), n.into(), [u, v]));
                }
                rows += 1;
            }
        }
        for r in 0..rows - 1 {
            for s in 0..segments {
                let a = (r * w + s) as u16;
                let b = a + w as u16;
                m.indices.extend_from_slice(&[a, a + 1, b, b, a + 1, b + 1]);
            }
        }
        m
    }

    /// Unity's Cylinder: `radius`, `height` along Y, capped.
    pub fn cylinder(radius: f32, height: f32, segments: u32) -> Self {
        let mut m = MeshData::default();
        let h = height / 2.0;
        // side
        for s in 0..=segments {
            let u = s as f32 / segments as f32;
            let th = u * std::f32::consts::TAU;
            let n = Vec3::new(th.cos(), 0.0, th.sin());
            for (y, v) in [(h, 0.0), (-h, 1.0)] {
                m.vertices.push(Vertex::new([n.x * radius, y, n.z * radius], n.into(), [u, v]));
            }
        }
        for s in 0..segments {
            let a = (s * 2) as u16;
            m.indices.extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
        }
        // caps
        for (y, ny) in [(h, 1.0f32), (-h, -1.0)] {
            let center = m.vertices.len() as u16;
            m.vertices.push(Vertex::new([0.0, y, 0.0], [0.0, ny, 0.0], [0.5, 0.5]));
            for s in 0..=segments {
                let th = s as f32 / segments as f32 * std::f32::consts::TAU;
                m.vertices.push(Vertex::new([th.cos() * radius, y, th.sin() * radius], [0.0, ny, 0.0], [0.5 + th.cos() * 0.5, 0.5 + th.sin() * 0.5]));
            }
            for s in 0..segments as u16 {
                let (a, b) = (center + 1 + s, center + 2 + s);
                if ny > 0.0 {
                    m.indices.extend_from_slice(&[center, b, a]);
                } else {
                    m.indices.extend_from_slice(&[center, a, b]);
                }
            }
        }
        m
    }

    /// Unity's Quad: `size` × `size` in the XY plane, normal +Z.
    pub fn quad(size: f32) -> Self {
        let h = size / 2.0;
        let mut m = MeshData::default();
        for (x, y, u, v) in [(-h, -h, 0.0, 1.0), (h, -h, 1.0, 1.0), (h, h, 1.0, 0.0), (-h, h, 0.0, 0.0)] {
            m.vertices.push(Vertex::new([x, y, 0.0], [0.0, 0.0, 1.0], [u, v]));
        }
        m.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        m
    }
}