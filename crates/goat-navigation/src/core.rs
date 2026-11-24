//! The navigation mesh, engine-free (the same algorithm as PiaEngine's com.pia.ai-navigation
//! `navmeshCore.ts`): a 2.5D heightfield like a reduced Recast.
//!
//! - **Bake** ([`bake`]): world triangles are rasterized into columns (cell = `cell_size` on X / Z);
//!   an up-facing surface (slope ≤ `agent_slope`) with `agent_height` of free space above it is a
//!   **node**; nodes of neighbouring columns are **linked** when their heights differ by ≤
//!   `agent_climb` (8 directions, diagonals only without cutting corners). Nodes closer than
//!   `agent_radius` to an edge are removed (erosion), then islands under `min_region_area`.
//! - **Queries**: nearest node, A* (area costs / mask, carved nodes skipped), string pulling into
//!   straight corners, walks along the surface (raycasts, sliding), closest edge.

use std::collections::BinaryHeap;

/// 8 neighbour directions (dx, dz). Even = straight, odd = diagonal.
pub const DIRS: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

/// Area 1 = Not Walkable (built-in areas: 0 Walkable, 1 Not Walkable, 2 Jump).
pub const NOT_WALKABLE: u8 = 1;

#[derive(Clone, Copy, Debug)]
pub struct BakeSettings {
    pub cell_size: f32,
    pub agent_radius: f32,
    pub agent_height: f32,
    /// Degrees.
    pub agent_slope: f32,
    pub agent_climb: f32,
    /// m²; smaller islands are dropped.
    pub min_region_area: f32,
    /// Optional bake volume (world AABB min, max).
    pub bounds: Option<([f32; 3], [f32; 3])>,
}

/// Triangles (9 floats each, world space) and an area per triangle.
#[derive(Clone, Debug, Default)]
pub struct BakeInput {
    pub positions: Vec<f32>,
    pub areas: Vec<u8>,
}

impl BakeInput {
    pub fn push(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3], area: u8) {
        self.positions.extend_from_slice(&[a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]]);
        self.areas.push(area);
    }
    pub fn triangle_count(&self) -> usize {
        self.areas.len()
    }
}

/// Baked navigation data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NavMeshData {
    pub origin_x: f32,
    pub origin_z: f32,
    pub cell_size: f32,
    pub width: usize,
    pub depth: usize,
    pub agent_radius: f32,
    pub agent_height: f32,
    pub agent_climb: f32,
    pub agent_slope: f32,
    /// Node range of column c = `col_start[c]..col_start[c + 1]`, c = x + z * width.
    pub col_start: Vec<u32>,
    pub y: Vec<f32>,
    pub area: Vec<u8>,
    /// 8 neighbour nodes per node (-1 = none).
    pub links: Vec<i32>,
    pub col: Vec<u32>,
    /// Carving: > 0 = blocked by that many obstacles (runtime only).
    pub blocked: Vec<u16>,
    /// Bumped whenever carving changes (agents re-check their paths).
    pub version: u32,
}

impl NavMeshData {
    pub fn node_count(&self) -> usize {
        self.y.len()
    }
    pub fn node_x(&self, n: usize) -> f32 {
        self.origin_x + ((self.col[n] as usize % self.width) as f32 + 0.5) * self.cell_size
    }
    pub fn node_z(&self, n: usize) -> f32 {
        self.origin_z + ((self.col[n] as usize / self.width) as f32 + 0.5) * self.cell_size
    }
    pub fn link(&self, n: usize, k: usize) -> Option<usize> {
        let l = self.links[n * 8 + k];
        (l >= 0).then_some(l as usize)
    }

    /// Column under a world X / Z.
    pub fn column_at(&self, x: f32, z: f32) -> Option<usize> {
        let cx = ((x - self.origin_x) / self.cell_size).floor();
        let cz = ((z - self.origin_z) / self.cell_size).floor();
        if cx < 0.0 || cz < 0.0 || cx as usize >= self.width || cz as usize >= self.depth {
            return None;
        }
        Some(cx as usize + cz as usize * self.width)
    }

    /// Node of column c best matching height `y` (at most `above` higher / `below` lower).
    pub fn node_in_column(&self, c: usize, y: f32, above: f32, below: f32) -> Option<usize> {
        let mut best = None;
        let mut best_d = f32::INFINITY;
        for n in self.col_start[c] as usize..self.col_start[c + 1] as usize {
            let d = self.y[n] - y;
            if d > above || d < -below {
                continue;
            }
            if d.abs() < best_d {
                best_d = d.abs();
                best = Some(n);
            }
        }
        best
    }

    /// Surface height at (x, z) on node n, interpolated with the straight neighbours (slopes).
    pub fn height_at(&self, n: usize, x: f32, z: f32) -> f32 {
        let fx = (x - self.node_x(n)) / self.cell_size;
        let fz = (z - self.node_z(n)) / self.cell_size;
        let mut h = self.y[n];
        if let Some(nx) = self.link(n, if fx >= 0.0 { 0 } else { 4 }) {
            h += (self.y[nx] - self.y[n]) * fx.abs();
        }
        if let Some(nz) = self.link(n, if fz >= 0.0 { 2 } else { 6 }) {
            h += (self.y[nz] - self.y[n]) * fz.abs();
        }
        h
    }

    /// Columns → nodes, links, carving arrays (from `col_start` / `y`).
    pub fn finish(&mut self) {
        let n = self.y.len();
        self.col = vec![0; n];
        for c in 0..self.width * self.depth {
            for i in self.col_start[c]..self.col_start[c + 1] {
                self.col[i as usize] = c as u32;
            }
        }
        self.blocked = vec![0; n];
        self.links = vec![-1; n * 8];
        let climb = self.agent_climb + 1e-4;
        for i in 0..n {
            let c = self.col[i] as usize;
            let (cx, cz) = ((c % self.width) as i32, (c / self.width) as i32);
            for d in (0..8).step_by(2) {
                let (dx, dz) = DIRS[d];
                let (x, z) = (cx + dx, cz + dz);
                if x < 0 || z < 0 || x as usize >= self.width || z as usize >= self.depth {
                    continue;
                }
                self.links[i * 8 + d] = self.node_in_column(x as usize + z as usize * self.width, self.y[i], climb, climb).map_or(-1, |v| v as i32);
            }
        }
        for i in 0..n {
            for d in (1..8).step_by(2) {
                let (Some(a), Some(b)) = (self.link(i, d - 1), self.link(i, (d + 1) & 7)) else { continue };
                let (va, vb) = (self.links[a * 8 + ((d + 1) & 7)], self.links[b * 8 + d - 1]);
                if va >= 0 && va == vb {
                    self.links[i * 8 + d] = va;
                }
            }
        }
    }

    /// Compact serialized form: header + nodes per column (u8) + heights (f32) + areas (u8), little endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"GNM1");
        for v in [self.origin_x, self.origin_z, self.cell_size, self.agent_radius, self.agent_height, self.agent_climb, self.agent_slope] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&(self.width as u32).to_le_bytes());
        out.extend_from_slice(&(self.depth as u32).to_le_bytes());
        for c in 0..self.width * self.depth {
            out.push((self.col_start[c + 1] - self.col_start[c]).min(255) as u8);
        }
        for y in &self.y {
            out.extend_from_slice(&y.to_le_bytes());
        }
        out.extend_from_slice(&self.area);
        out
    }

    pub fn from_bytes(b: &[u8]) -> Option<NavMeshData> {
        if b.len() < 40 || &b[0..4] != b"GNM1" {
            return None;
        }
        let f = |i: usize| f32::from_le_bytes(b[4 + i * 4..8 + i * 4].try_into().unwrap());
        let u = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
        let mut d = NavMeshData { origin_x: f(0), origin_z: f(1), cell_size: f(2), agent_radius: f(3), agent_height: f(4), agent_climb: f(5), agent_slope: f(6), width: u(32), depth: u(36), ..Default::default() };
        let cols = d.width * d.depth;
        let counts = b.get(40..40 + cols)?;
        d.col_start = vec![0; cols + 1];
        for c in 0..cols {
            d.col_start[c + 1] = d.col_start[c] + counts[c] as u32;
        }
        let n = d.col_start[cols] as usize;
        let ys = b.get(40 + cols..40 + cols + n * 4)?;
        d.y = ys.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        d.area = b.get(40 + cols + n * 4..40 + cols + n * 5)?.to_vec();
        d.finish();
        Some(d)
    }
}

/// Max grid columns per side.
const MAX_CELLS: f32 = 2048.0;

/// Bakes triangles into navigation data.
pub fn bake(input: &BakeInput, s: &BakeSettings) -> NavMeshData {
    let pos = &input.positions;
    let tri_count = input.areas.len().min(pos.len() / 9);
    let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    match s.bounds {
        Some((a, b)) => {
            min = a;
            max = b;
        }
        None => {
            for v in pos[..tri_count * 9].chunks_exact(3) {
                for k in 0..3 {
                    min[k] = min[k].min(v[k]);
                    max[k] = max[k].max(v[k]);
                }
            }
        }
    }
    let mut d = NavMeshData { agent_radius: s.agent_radius, agent_height: s.agent_height, agent_climb: s.agent_climb, agent_slope: s.agent_slope, cell_size: s.cell_size.max(0.01), ..Default::default() };
    if !min[0].is_finite() {
        d.width = 1;
        d.depth = 1;
        d.col_start = vec![0, 0];
        d.finish();
        return d;
    }
    let mut cell = d.cell_size;
    while (max[0] - min[0]) / cell > MAX_CELLS || (max[2] - min[2]) / cell > MAX_CELLS {
        cell *= 2.0;
    }
    d.cell_size = cell;
    d.origin_x = min[0];
    d.origin_z = min[2];
    d.width = (((max[0] - min[0]) / cell).ceil() as usize).max(1);
    d.depth = (((max[2] - min[2]) / cell).ceil() as usize).max(1);
    let (w, dp) = (d.width, d.depth);

    // 1. rasterize: (height, tag) per column; tag = area, 255 = solid top / steep, 254 = underside
    let mut spans: Vec<Vec<(f32, u8)>> = vec![Vec::new(); w * dp];
    let min_up = (s.agent_slope.clamp(0.0, 89.9)).to_radians().cos();
    for t in 0..tri_count {
        let o = t * 9;
        let (ax, ay, az, bx, by, bz, cx, cy, cz) = (pos[o], pos[o + 1], pos[o + 2], pos[o + 3], pos[o + 4], pos[o + 5], pos[o + 6], pos[o + 7], pos[o + 8]);
        let (ux, uy, uz, vx, vy, vz) = (bx - ax, by - ay, bz - az, cx - ax, cy - ay, cz - az);
        let (nx, ny, nz) = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx);
        let nl = (nx * nx + ny * ny + nz * nz).sqrt();
        if nl < 1e-12 {
            continue;
        }
        let ny = ny / nl;
        let area = input.areas[t];
        let walkable = area != NOT_WALKABLE && ny >= min_up && ny > 0.0;
        let tag = if walkable { area } else if ny < 0.0 { 254 } else { 255 };
        let den = (bz - cz) * (ax - cx) + (cx - bx) * (az - cz);
        if den.abs() < 1e-12 {
            continue;
        }
        let x0 = (((ax.min(bx).min(cx) - d.origin_x) / cell - 0.5).floor().max(0.0)) as usize;
        let x1 = (((ax.max(bx).max(cx) - d.origin_x) / cell - 0.5).ceil().min(w as f32 - 1.0)).max(-1.0) as i64;
        let z0 = (((az.min(bz).min(cz) - d.origin_z) / cell - 0.5).floor().max(0.0)) as usize;
        let z1 = (((az.max(bz).max(cz) - d.origin_z) / cell - 0.5).ceil().min(dp as f32 - 1.0)).max(-1.0) as i64;
        for gz in z0 as i64..=z1 {
            let pz = d.origin_z + (gz as f32 + 0.5) * cell;
            for gx in x0 as i64..=x1 {
                let px = d.origin_x + (gx as f32 + 0.5) * cell;
                let l1 = ((bz - cz) * (px - cx) + (cx - bx) * (pz - cz)) / den;
                let l2 = ((cz - az) * (px - cx) + (ax - cx) * (pz - cz)) / den;
                let l3 = 1.0 - l1 - l2;
                let e = -1e-6;
                if l1 < e || l2 < e || l3 < e {
                    continue;
                }
                spans[gx as usize + gz as usize * w].push((l1 * ay + l2 * by + l3 * cy, tag));
            }
        }
    }

    // 2. nodes: walkable surfaces with headroom; a solid starting right at h blocks (box on a floor)
    let eps = (s.agent_climb * 0.5 + 1e-3).min(0.05);
    let mut cols: Vec<Vec<(f32, u8)>> = Vec::with_capacity(w * dp);
    for list in &mut spans {
        list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut out: Vec<(f32, u8)> = Vec::new();
        for k in 0..list.len() {
            let (h, tag) = list[k];
            if tag >= 254 {
                continue;
            }
            let free = list.iter().enumerate().all(|(m, &(hm, tm))| m == k || hm >= h + s.agent_height || if tm == 254 { hm < h - 0.01 } else { hm <= h + eps });
            if !free {
                continue;
            }
            if let Some(last) = out.last_mut() {
                if (last.0 - h).abs() <= eps {
                    last.0 = last.0.max(h);
                    continue;
                }
            }
            out.push((h, tag));
        }
        cols.push(out);
    }
    fill(&mut d, &cols);

    // 3. erosion by the agent radius
    let r = s.agent_radius / cell;
    if r > 0.0 {
        let n = d.node_count();
        let mut dist = vec![f32::INFINITY; n];
        let mut queue = Vec::new();
        for i in 0..n {
            if (0..8).step_by(2).filter(|&k| d.links[i * 8 + k] >= 0).count() < 4 {
                dist[i] = 0.0;
                queue.push(i);
            }
        }
        let mut head = 0;
        while head < queue.len() {
            let i = queue[head];
            head += 1;
            for k in 0..8 {
                let Some(j) = d.link(i, k) else { continue };
                let nd = dist[i] + if k & 1 == 1 { std::f32::consts::SQRT_2 } else { 1.0 };
                if nd < dist[j] - 1e-6 {
                    dist[j] = nd;
                    queue.push(j);
                }
            }
        }
        let keep: Vec<bool> = dist.iter().map(|&v| v + 0.5 >= r - 1e-4).collect();
        rebuild(&mut d, &keep);
    }

    // 4. islands
    if s.min_region_area > 0.0 {
        let n = d.node_count();
        let mut region = vec![usize::MAX; n];
        let mut keep = vec![false; n];
        let min_cells = s.min_region_area / (cell * cell);
        for i in 0..n {
            if region[i] != usize::MAX {
                continue;
            }
            region[i] = i;
            let mut stack = vec![i];
            let mut members = Vec::new();
            while let Some(a) = stack.pop() {
                members.push(a);
                for k in (0..8).step_by(2) {
                    if let Some(b) = d.link(a, k) {
                        if region[b] == usize::MAX {
                            region[b] = i;
                            stack.push(b);
                        }
                    }
                }
            }
            if members.len() as f32 >= min_cells {
                for m in members {
                    keep[m] = true;
                }
            }
        }
        rebuild(&mut d, &keep);
    }
    d
}

fn fill(d: &mut NavMeshData, cols: &[Vec<(f32, u8)>]) {
    d.col_start = Vec::with_capacity(cols.len() + 1);
    d.y.clear();
    d.area.clear();
    for c in cols {
        d.col_start.push(d.y.len() as u32);
        for &(y, a) in c {
            d.y.push(y);
            d.area.push(a);
        }
    }
    d.col_start.push(d.y.len() as u32);
    d.finish();
}

fn rebuild(d: &mut NavMeshData, keep: &[bool]) {
    let cols: Vec<Vec<(f32, u8)>> = (0..d.width * d.depth).map(|c| (d.col_start[c] as usize..d.col_start[c + 1] as usize).filter(|&i| keep[i]).map(|i| (d.y[i], d.area[i])).collect()).collect();
    fill(d, &cols);
}

// ------------------------------------------------------------------ queries

/// Which areas may be crossed, at what cost.
#[derive(Clone, Debug)]
pub struct QueryFilter {
    pub area_mask: u32,
    pub costs: [f32; 32],
}

impl Default for QueryFilter {
    fn default() -> Self {
        let mut costs = [1.0; 32];
        costs[2] = 2.0;
        QueryFilter { area_mask: u32::MAX, costs }
    }
}

fn passable(d: &NavMeshData, n: usize, f: &QueryFilter) -> bool {
    d.blocked[n] == 0 && (f.area_mask >> d.area[n]) & 1 == 1
}

/// Nearest node within `max_distance` (3D), searching rings of columns.
pub fn nearest_node(d: &NavMeshData, x: f32, y: f32, z: f32, max_distance: f32, f: &QueryFilter) -> Option<usize> {
    if d.node_count() == 0 {
        return None;
    }
    let cx = ((x - d.origin_x) / d.cell_size).floor() as i64;
    let cz = ((z - d.origin_z) / d.cell_size).floor() as i64;
    let rings = (max_distance / d.cell_size).ceil() as i64 + 1;
    let mut best = None;
    let mut best_d = max_distance * max_distance;
    for r in 0..=rings {
        let ring_min = ((r - 1).max(0)) as f32 * d.cell_size;
        if best.is_some() && ring_min * ring_min > best_d {
            break;
        }
        for gz in cz - r..=cz + r {
            for gx in cx - r..=cx + r {
                if (gx - cx).abs().max((gz - cz).abs()) != r || gx < 0 || gz < 0 || gx as usize >= d.width || gz as usize >= d.depth {
                    continue;
                }
                let c = gx as usize + gz as usize * d.width;
                for n in d.col_start[c] as usize..d.col_start[c + 1] as usize {
                    if !passable(d, n, f) {
                        continue;
                    }
                    let px = x.clamp(d.origin_x + gx as f32 * d.cell_size, d.origin_x + (gx + 1) as f32 * d.cell_size);
                    let pz = z.clamp(d.origin_z + gz as f32 * d.cell_size, d.origin_z + (gz + 1) as f32 * d.cell_size);
                    let dd = (px - x).powi(2) + (d.y[n] - y).powi(2) + (pz - z).powi(2);
                    if dd <= best_d {
                        best_d = dd;
                        best = Some(n);
                    }
                }
            }
        }
    }
    best
}

/// The point of node n closest to (x, z) (inside its cell), with the interpolated height.
pub fn point_on_node(d: &NavMeshData, n: usize, x: f32, z: f32) -> [f32; 3] {
    let h = d.cell_size * 0.5 - 1e-4;
    let (nx, nz) = (d.node_x(n), d.node_z(n));
    let (px, pz) = (x.clamp(nx - h, nx + h), z.clamp(nz - h, nz + h));
    [px, d.height_at(n, px, pz), pz]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathStatus {
    Complete,
    Partial,
    Invalid,
}

#[derive(PartialEq)]
struct Open(f32, usize);
impl Eq for Open {}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        // min-heap on the key
        o.0.partial_cmp(&self.0).unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// A* from node a to node b: the node chain (to b, or to the closest node found).
pub fn find_node_path(d: &NavMeshData, a: usize, b: usize, f: &QueryFilter) -> (Vec<usize>, PathStatus) {
    let n = d.node_count();
    let mut g = vec![f32::INFINITY; n];
    let mut from = vec![usize::MAX; n];
    let mut closed = vec![false; n];
    let (bx, bz, by) = (d.node_x(b), d.node_z(b), d.y[b]);
    let hcost = |i: usize| {
        let (dx, dz) = ((d.node_x(i) - bx).abs() / d.cell_size, (d.node_z(i) - bz).abs() / d.cell_size);
        dx.max(dz) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dz) + (d.y[i] - by).abs() / d.cell_size
    };
    let mut heap = BinaryHeap::new();
    g[a] = 0.0;
    heap.push(Open(hcost(a), a));
    let (mut closest, mut closest_h) = (a, hcost(a));
    let mut iter = 0;
    while let Some(Open(_, i)) = heap.pop() {
        iter += 1;
        if iter > 400_000 {
            break;
        }
        if closed[i] {
            continue;
        }
        closed[i] = true;
        if i == b {
            break;
        }
        let hi = hcost(i);
        if hi < closest_h {
            closest_h = hi;
            closest = i;
        }
        for k in 0..8 {
            let Some(j) = d.link(i, k) else { continue };
            if closed[j] || !passable(d, j, f) {
                continue;
            }
            if k & 1 == 1 {
                let ok = matches!((d.link(i, k - 1), d.link(i, (k + 1) & 7)), (Some(s1), Some(s2)) if passable(d, s1, f) && passable(d, s2, f));
                if !ok {
                    continue;
                }
            }
            let step = if k & 1 == 1 { std::f32::consts::SQRT_2 } else { 1.0 } + (d.y[j] - d.y[i]).abs() / d.cell_size;
            let ng = g[i] + f.costs[d.area[j] as usize % 32] * step;
            if ng < g[j] {
                g[j] = ng;
                from[j] = i;
                heap.push(Open(ng + hcost(j), j));
            }
        }
    }
    let end = if closed[b] { b } else { closest };
    let mut nodes = Vec::new();
    let mut i = end;
    while i != usize::MAX {
        nodes.push(i);
        i = from[i];
    }
    nodes.reverse();
    (nodes, if end == b { PathStatus::Complete } else { PathStatus::Partial })
}

/// Walks the surface from node a at (ax, az) toward (bx, bz): (node reached, fraction 0..1 done).
pub fn walk(d: &NavMeshData, a: usize, ax: f32, az: f32, bx: f32, bz: f32, f: &QueryFilter) -> (usize, f32) {
    let len = ((bx - ax).powi(2) + (bz - az).powi(2)).sqrt();
    if len < 1e-9 {
        return (a, 1.0);
    }
    let steps = ((len / (d.cell_size * 0.25)).ceil() as usize).max(1);
    let (mut node, mut col) = (a, d.col[a] as usize);
    for s in 1..=steps {
        let t = s as f32 / steps as f32;
        let Some(c) = d.column_at(ax + (bx - ax) * t, az + (bz - az) * t) else { return (node, (s - 1) as f32 / steps as f32) };
        if c == col {
            continue;
        }
        let (dx, dz) = ((c % d.width) as i32 - (col % d.width) as i32, (c / d.width) as i32 - (col / d.width) as i32);
        let mut next = None;
        if dx.abs() <= 1 && dz.abs() <= 1 {
            if let Some(k) = DIRS.iter().position(|&v| v == (dx, dz)) {
                next = d.link(node, k);
                if next.is_some() && k & 1 == 1 {
                    let ok = matches!((d.link(node, k - 1), d.link(node, (k + 1) & 7)), (Some(s1), Some(s2)) if passable(d, s1, f) && passable(d, s2, f));
                    if !ok {
                        next = None;
                    }
                }
            }
        }
        match next {
            Some(nx) if passable(d, nx, f) => {
                node = nx;
                col = c;
            }
            _ => return (node, (s - 1) as f32 / steps as f32),
        }
    }
    (node, 1.0)
}

/// Straight corners through a node chain (string pulling with surface line-of-sight).
pub fn string_pull(d: &NavMeshData, nodes: &[usize], start: [f32; 3], end: [f32; 3], f: &QueryFilter) -> Vec<[f32; 3]> {
    let mut corners = vec![start];
    if nodes.is_empty() {
        return corners;
    }
    let last = nodes.len() - 1;
    let target = |i: usize| if i == last { (end[0], end[2]) } else { (d.node_x(nodes[i]), d.node_z(nodes[i])) };
    let (mut cur, mut cx, mut cz) = (0usize, start[0], start[2]);
    while cur < last {
        let mut far = cur + 1;
        for j in (cur + 2..=last).rev() {
            let (tx, tz) = target(j);
            let (node, t) = walk(d, nodes[cur], cx, cz, tx, tz, f);
            if t >= 1.0 && node == nodes[j] {
                far = j;
                break;
            }
        }
        let (tx, tz) = target(far);
        cur = far;
        cx = tx;
        cz = tz;
        if far == last {
            break;
        }
        corners.push([tx, d.height_at(nodes[far], tx, tz), tz]);
    }
    corners.push(end);
    if corners.len() == 2 && ((end[0] - start[0]).powi(2) + (end[2] - start[2]).powi(2)).sqrt() < 1e-6 {
        corners.pop();
    }
    corners
}

/// The closest edge (a side of a node without a passable straight link) near node a:
/// (node, point, outward normal, distance).
pub fn closest_edge(d: &NavMeshData, a: usize, x: f32, z: f32, f: &QueryFilter) -> Option<(usize, [f32; 3], [f32; 3], f32)> {
    let mut seen = std::collections::HashSet::from([a]);
    let mut frontier = vec![a];
    let mut best: Option<(usize, [f32; 3], [f32; 3], f32)> = None;
    for ring in 0..256 {
        if frontier.is_empty() {
            break;
        }
        let mut next = Vec::new();
        for &n in &frontier {
            let (nx, nz, h) = (d.node_x(n), d.node_z(n), d.cell_size / 2.0);
            for k in (0..8).step_by(2) {
                if let Some(j) = d.link(n, k).filter(|&j| passable(d, j, f)) {
                    if seen.insert(j) {
                        next.push(j);
                    }
                    continue;
                }
                let (dx, dz) = DIRS[k];
                let (px, pz) = if dx != 0 { (nx + dx as f32 * h, z.clamp(nz - h, nz + h)) } else { (x.clamp(nx - h, nx + h), nz + dz as f32 * h) };
                let dist = ((px - x).powi(2) + (pz - z).powi(2)).sqrt();
                if best.is_none_or(|b| dist < b.3) {
                    best = Some((n, [px, d.height_at(n, px, pz), pz], [-dx as f32, 0.0, -dz as f32], dist));
                }
            }
        }
        if best.is_some_and(|b| b.3 < (ring as f32 + 0.5) * d.cell_size) {
            break;
        }
        frontier = next;
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(inp: &mut BakeInput, y: f32, x0: f32, z0: f32, x1: f32, z1: f32, up: bool) {
        let (a, b, c, d) = ([x0, y, z0], [x0, y, z1], [x1, y, z1], [x1, y, z0]);
        if up {
            inp.push(a, b, c, 0);
            inp.push(a, c, d, 0);
        } else {
            inp.push(a, c, b, 0);
            inp.push(a, d, c, 0);
        }
    }

    fn settings() -> BakeSettings {
        BakeSettings { cell_size: 0.25, agent_radius: 0.5, agent_height: 2.0, agent_slope: 45.0, agent_climb: 0.4, min_region_area: 0.0, bounds: None }
    }

    #[test]
    fn bake_erode_wall_path_and_round_trip() {
        let mut inp = BakeInput::default();
        quad(&mut inp, 0.0, -5.0, -5.0, 5.0, 5.0, true);
        // a 2 m wall across x = 0 from z = -5 to 3: top + underside (sides are vertical)
        quad(&mut inp, 2.0, -0.25, -5.0, 0.25, 3.0, true);
        quad(&mut inp, 0.0, -0.25, -5.0, 0.25, 3.0, false);
        let d = bake(&inp, &settings());
        assert!(d.node_count() > 200);
        assert!(d.node_in_column(d.column_at(0.1, 0.0).unwrap(), 0.0, 0.1, 0.1).is_none(), "no floor under the wall (equal to the agent height)");
        assert!(d.node_in_column(d.column_at(-4.9, 0.0).unwrap(), 0.0, 0.1, 0.1).is_none(), "eroded edge");
        let f = QueryFilter::default();
        let a = nearest_node(&d, -3.0, 0.0, -3.0, 1.0, &f).unwrap();
        let b = nearest_node(&d, 3.0, 0.0, -3.0, 1.0, &f).unwrap();
        let (nodes, status) = find_node_path(&d, a, b, &f);
        assert_eq!(status, PathStatus::Complete);
        let corners = string_pull(&d, &nodes, point_on_node(&d, a, -3.0, -3.0), point_on_node(&d, b, 3.0, -3.0), &f);
        assert!(corners.iter().any(|c| c[2] > 3.0), "around the wall end: {corners:?}");
        assert!(corners.len() <= 5);
        // the straight walk is blocked by the wall
        assert!(walk(&d, a, -3.0, -3.0, 3.0, -3.0, &f).1 < 1.0);
        let e = closest_edge(&d, nearest_node(&d, -1.5, 0.0, -2.0, 1.0, &f).unwrap(), -1.5, -2.0, &f).unwrap();
        assert!((e.1[0] + 0.75).abs() < 0.2, "{e:?}");
        let back = NavMeshData::from_bytes(&d.to_bytes()).unwrap();
        assert_eq!(back.links, d.links);
        assert_eq!(back.y, d.y);
    }
}
