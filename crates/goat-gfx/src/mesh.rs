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
            return crate::Bounds { center: Vec3::ZERO, radius: 0.0 };
        }
        let center = (min + max) * 0.5;
        let radius = self.vertices.iter().map(|v| (Vec3::from(v.position) - center).length()).fold(0.0, f32::max);
        crate::Bounds { center, radius }
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
}
