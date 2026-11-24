//! What a NavMeshSurface bakes: world triangles of MeshRenderers (builtin meshes and meshes of
//! models, through `AssetServer::mesh_geometry`) or of goat-physics colliders (feature
//! `physics`: Box / Sphere / Capsule / Mesh), with areas from NavMeshModifier. Objects with a
//! NavMeshAgent / NavMeshObstacle are left out (Unity).

use goat_assets::AssetServer;
use goat_core::math::{Mat4, Vec3};
use goat_runtime::{Entity, GlobalTransform, MeshRenderer, Parent, World};

use crate::core::BakeInput;
use crate::{CollectObjects, NavMeshAgent, NavMeshModifier, NavMeshObstacle, NavMeshSurface, UseGeometry};

struct Tris<'a> {
    out: &'a mut BakeInput,
}

impl Tris<'_> {
    /// A triangle; closed shapes pass their center to orient it outward (negative scales flip windings).
    fn add(&mut self, a: Vec3, b: Vec3, c: Vec3, area: u8, center: Option<Vec3>) {
        let (mut b, mut c) = (b, c);
        if let Some(o) = center {
            let n = (b - a).cross(c - a);
            if n.dot((a + b + c) / 3.0 - o) < 0.0 {
                std::mem::swap(&mut b, &mut c);
            }
        }
        self.out.push(a.into(), b.into(), c.into(), area);
    }

    fn mesh(&mut self, m: &Mat4, points: &[Vec3], tris: &[[u32; 3]], area: u8) {
        // a negative determinant mirrors: keep normals pointing the same way
        let flip = m.determinant() < 0.0;
        for t in tris {
            let (a, b, c) = (m.transform_point3(points[t[0] as usize]), m.transform_point3(points[t[1] as usize]), m.transform_point3(points[t[2] as usize]));
            if flip {
                self.add(a, c, b, area, None);
            } else {
                self.add(a, b, c, area, None);
            }
        }
    }

    #[cfg_attr(not(feature = "physics"), allow(dead_code))]
    fn cuboid(&mut self, m: &Mat4, c: Vec3, h: Vec3, area: u8) {
        let p = |sx: f32, sy: f32, sz: f32| m.transform_point3(c + Vec3::new(sx * h.x, sy * h.y, sz * h.z));
        let v = [p(-1., -1., -1.), p(1., -1., -1.), p(1., 1., -1.), p(-1., 1., -1.), p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.)];
        let center = m.transform_point3(c);
        for f in [[0, 1, 2, 3], [5, 4, 7, 6], [4, 0, 3, 7], [1, 5, 6, 2], [3, 2, 6, 7], [4, 5, 1, 0]] {
            self.add(v[f[0]], v[f[1]], v[f[2]], area, Some(center));
            self.add(v[f[0]], v[f[2]], v[f[3]], area, Some(center));
        }
    }

    /// Sphere (half = 0) / capsule along local Y as rings of quads.
    #[cfg_attr(not(feature = "physics"), allow(dead_code))]
    fn round(&mut self, m: &Mat4, c: Vec3, radius: f32, half: f32, area: u8) {
        let (seg, rings) = (12, 6);
        let mut rows: Vec<Vec<Vec3>> = Vec::new();
        for r in 0..=rings {
            let phi = r as f32 / rings as f32 * std::f32::consts::PI;
            let y = -phi.cos() * radius + if r <= rings / 2 { -half } else { half };
            let rr = phi.sin() * radius;
            let ring = |y: f32| (0..seg).map(|i| m.transform_point3(c + Vec3::new((i as f32 / seg as f32 * std::f32::consts::TAU).cos() * rr, y, (i as f32 / seg as f32 * std::f32::consts::TAU).sin() * rr))).collect::<Vec<_>>();
            rows.push(ring(y));
            if r == rings / 2 && half > 0.0 {
                rows.push(ring(y + 2.0 * half));
            }
        }
        let center = m.transform_point3(c);
        for r in 0..rows.len() - 1 {
            for i in 0..seg {
                let j = (i + 1) % seg;
                self.add(rows[r][i], rows[r][j], rows[r + 1][j], area, Some(center));
                self.add(rows[r][i], rows[r + 1][j], rows[r + 1][i], area, Some(center));
            }
        }
    }
}

/// The modifier applying to `e` (its own, or an ancestor's with `apply_to_children`).
fn modifier(world: &World, e: Entity) -> Option<NavMeshModifier> {
    let mut cur = Some(e);
    let mut first = true;
    for _ in 0..256 {
        let x = cur?;
        if let Ok(m) = world.get::<&NavMeshModifier>(x) {
            if first || m.apply_to_children {
                return Some((*m).clone());
            }
        }
        first = false;
        cur = world.get::<&Parent>(x).ok().map(|p| p.0);
    }
    None
}

fn under(world: &World, e: Entity, root: Entity) -> bool {
    let mut cur = Some(e);
    for _ in 0..256 {
        match cur {
            Some(x) if x == root => return true,
            Some(x) => cur = world.get::<&Parent>(x).ok().map(|p| p.0),
            None => return false,
        }
    }
    false
}

/// Collects a surface's bake input.
pub fn collect_sources(world: &World, assets: &mut AssetServer, s: &NavMeshSurface, surface: Entity, bounds: Option<([f32; 3], [f32; 3])>) -> BakeInput {
    let mut input = BakeInput::default();
    let mut t = Tris { out: &mut input };
    let mut warned: Vec<String> = Vec::new();
    let candidates: Vec<(Entity, Mat4)> = world.query::<&GlobalTransform>().iter().map(|(e, g)| (e, g.0)).collect();
    for (e, m) in candidates {
        if world.get::<&NavMeshAgent>(e).is_ok() || world.get::<&NavMeshObstacle>(e).is_ok() || world.get::<&goat_runtime::DontSave>(e).is_ok() {
            continue;
        }
        if s.collect_objects == CollectObjects::Children && !under(world, e, surface) {
            continue;
        }
        let modif = modifier(world, e);
        if modif.as_ref().is_some_and(|m| m.ignore_from_build) {
            continue;
        }
        let area = modif.filter(|m| m.override_area).map_or(s.default_area, |m| m.area);
        match s.use_geometry {
            UseGeometry::RenderMeshes => {
                let Ok(r) = world.get::<&MeshRenderer>(e) else { continue };
                if !r.visible || (s.layer_mask >> (r.layer & 31)) & 1 == 0 {
                    continue;
                }
                let Some(key) = assets.key_of_mesh(r.mesh).map(str::to_string) else { continue };
                match assets.mesh_geometry(&key) {
                    Ok((points, tris)) => t.mesh(&m, &points, &tris, area),
                    Err(err) => {
                        if !warned.contains(&key) {
                            goat_core::warn!("NavMeshSurface: {key}: {err}");
                            warned.push(key);
                        }
                    }
                }
            }
            UseGeometry::PhysicsColliders => {
                #[cfg(feature = "physics")]
                {
                    use goat_physics::{Collider, Shape};
                    let Ok(c) = world.get::<&Collider>(e) else { continue };
                    if c.is_trigger || (s.layer_mask >> (c.layer & 31)) & 1 == 0 {
                        continue;
                    }
                    match &c.shape {
                        Shape::Box(h) => t.cuboid(&m, c.center, *h, area),
                        Shape::Sphere(r) => t.round(&m, c.center, *r, 0.0, area),
                        Shape::Capsule { half_height, radius } => t.round(&m, c.center, *radius, *half_height, area),
                        Shape::Mesh { source, .. } => {
                            let key = if source.is_empty() { world.get::<&MeshRenderer>(e).ok().and_then(|r| assets.key_of_mesh(r.mesh).map(str::to_string)) } else { Some(source.clone()) };
                            if let Some((points, tris)) = key.and_then(|k| assets.mesh_geometry(&k).ok()) {
                                let mc = m * Mat4::from_translation(c.center);
                                t.mesh(&mc, &points, &tris, area);
                            }
                        }
                    }
                }
            }
        }
    }
    // volume: drop triangles entirely outside
    if let Some((min, max)) = bounds {
        let mut kept = BakeInput::default();
        for (i, &area) in input.areas.iter().enumerate() {
            let p = &input.positions[i * 9..i * 9 + 9];
            let out = (0..3).any(|k| (p[k] < min[k] && p[k + 3] < min[k] && p[k + 6] < min[k]) || (p[k] > max[k] && p[k + 3] > max[k] && p[k + 6] > max[k]));
            if !out {
                kept.positions.extend_from_slice(p);
                kept.areas.push(area);
            }
        }
        return kept;
    }
    input
}
