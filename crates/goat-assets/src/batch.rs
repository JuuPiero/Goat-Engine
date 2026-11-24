//! Static batching (Unity's): on a root with [`StaticBatching`], every visible descendant
//! MeshRenderer whose mesh has an asset key (builtins, model meshes) is baked into world space
//! and merged with the others of the same material / layer / order — one mesh (split at 65 535
//! vertices) and one draw call per group. The originals are hidden, not removed (colliders,
//! scripts keep working on them). With ake_lighting the scene's lights are computed into the
//! vertex colors and the batches draw unlit.

use std::collections::HashMap;

use goat_core::math::{Mat4, Vec3};
use goat_gfx::{MeshData, Vertex};
use goat_runtime::{Context, DontSave, Entity, GlobalTransform, MeshRenderer, Name, Parent, Plugin, StaticBatching, Transform};

use crate::AssetServer;

/// Marks a root whose batches exist (built once).
struct Batched(#[allow(dead_code)] Vec<Entity>);

/// Builds the static batches in play mode (add it after AssetsPlugin).
#[derive(Default)]
pub struct StaticBatchPlugin;

impl Plugin for StaticBatchPlugin {
    fn late_update(&mut self, ctx: &mut Context) {
        let roots: Vec<(Entity, StaticBatching)> = ctx.world.query::<&StaticBatching>().without::<&Batched>().iter().map(|(e, s)| (e, *s)).collect();
        for (root, s) in roots {
            let made = build(ctx, root, s.bake_lighting);
            let _ = ctx.world.insert_one(root, Batched(made));
        }
    }
}

fn descendants(ctx: &Context, root: Entity) -> Vec<Entity> {
    let pairs: Vec<(Entity, Entity)> = ctx.world.query::<&Parent>().iter().map(|(c, p)| (p.0, c)).collect();
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        let x = out[i];
        out.extend(pairs.iter().filter(|(p, _)| *p == x).map(|(_, c)| *c));
        i += 1;
    }
    out
}

/// The lights baked into vertices (world space, as the renderer would light them).
struct SceneLights {
    ambient: Vec3,
    ground: Option<Vec3>,
    lights: Vec<goat_gfx::Light>,
}

impl SceneLights {
    fn gather(ctx: &Context) -> Self {
        use goat_runtime::{DirectionalLight, PointLight, RenderSettings, SpotLight};
        let v = |c: goat_core::Color, k: f32| Vec3::new(c.r, c.g, c.b) * k;
        let s = ctx.world.query::<&RenderSettings>().iter().next().map(|(_, s)| *s).unwrap_or_default();
        let fwd = |g: &GlobalTransform| g.0.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Z);
        let mut lights = Vec::new();
        let col = |c: goat_core::Color, i: f32| goat_core::Color::rgba(c.r * i, c.g * i, c.b * i, 1.0);
        for (_, (l, g)) in ctx.world.query::<(&DirectionalLight, &GlobalTransform)>().iter() {
            lights.push(goat_gfx::Light { kind: goat_gfx::LightKind::Directional, position: g.0.w_axis.truncate(), direction: fwd(g), color: col(l.color, l.intensity), range: 0.0 });
        }
        for (_, (l, g)) in ctx.world.query::<(&PointLight, &GlobalTransform)>().iter() {
            lights.push(goat_gfx::Light { kind: goat_gfx::LightKind::Point, position: g.0.w_axis.truncate(), direction: fwd(g), color: col(l.color, l.intensity), range: l.range.max(0.01) });
        }
        for (_, (l, g)) in ctx.world.query::<(&SpotLight, &GlobalTransform)>().iter() {
            let half = |deg: f32| (deg.clamp(0.1, 179.0) * 0.5).to_radians().cos();
            let cos_outer = half(l.spot_angle);
            let cos_inner = half(l.inner_spot_angle.min(l.spot_angle)).max(cos_outer + 1e-3);
            lights.push(goat_gfx::Light { kind: goat_gfx::LightKind::Spot { cos_inner, cos_outer }, position: g.0.w_axis.truncate(), direction: fwd(g), color: col(l.color, l.intensity), range: l.range.max(0.01) });
        }
        SceneLights { ambient: v(s.ambient, 1.0), ground: s.ambient_ground.map(|g| v(g, 1.0)), lights }
    }

    /// Light arriving at a surface point (clamped to 0..1: vertex colors can't go brighter).
    fn at(&self, p: Vec3, n: Vec3) -> Vec3 {
        let mut c = match self.ground {
            Some(g) => g.lerp(self.ambient, 0.5 + 0.5 * n.y),
            None => self.ambient,
        };
        for l in &self.lights {
            let (dir, k) = match l.kind {
                goat_gfx::LightKind::Directional => (-l.direction, 1.0),
                _ => {
                    let to = l.position - p;
                    let d = to.length().max(1e-4);
                    let dir = to / d;
                    let mut k = goat_gfx::Light::attenuation(d, l.range);
                    if let goat_gfx::LightKind::Spot { cos_inner, cos_outer } = l.kind {
                        k *= goat_gfx::Light::spot_factor((-dir).dot(l.direction), cos_inner, cos_outer);
                    }
                    (dir, k)
                }
            };
            c += Vec3::new(l.color.r, l.color.g, l.color.b) * n.dot(dir).max(0.0) * k;
        }
        c.clamp(Vec3::ZERO, Vec3::ONE)
    }
}

/// Merges the root's meshes; returns the batch entities.
pub fn build(ctx: &mut Context, root: Entity, bake: bool) -> Vec<Entity> {
    let lights = if bake { Some(SceneLights::gather(ctx)) } else { None };
    let Some(assets) = ctx.resources.get::<AssetServer>() else { return Vec::new() };
    // group key: (material, layer, sorting order)
    let mut groups: HashMap<(u64, u8, i32), (goat_gfx::MaterialId, Vec<(Entity, MeshData, Mat4)>)> = HashMap::new();
    let mut cache: HashMap<String, MeshData> = HashMap::new();
    for e in descendants(ctx, root) {
        let Ok(r) = ctx.world.get::<&MeshRenderer>(e).map(|r| *r) else { continue };
        if !r.visible {
            continue;
        }
        let Some(key) = assets.key_of_mesh(r.mesh).map(str::to_string) else { continue };
        let data = match cache.get(&key) {
            Some(d) => d.clone(),
            None => match assets.mesh_data(&key) {
                Ok(d) => {
                    cache.insert(key.clone(), d.clone());
                    d
                }
                Err(_) => continue,
            },
        };
        let m = ctx.world.get::<&GlobalTransform>(e).map(|g| g.0).unwrap_or(Mat4::IDENTITY);
        groups.entry((r.material.id(), r.layer, r.sorting_order)).or_insert_with(|| (r.material, Vec::new())).1.push((e, data, m));
    }
    let mut made = Vec::new();
    for ((_, layer, order), (material, parts)) in groups {
        if parts.len() < 2 && lights.is_none() {
            continue; // nothing to gain
        }
        // baked: the same material drawn unlit
        let material = match (&lights, ctx.gfx.material_desc(material)) {
            (Some(_), Some(d)) => ctx.gfx.create_material(&goat_gfx::MaterialDesc { lighting: goat_gfx::LightingModel::Unlit, ..d }).unwrap_or(material),
            _ => material,
        };
        let mut chunk = MeshData::default();
        let mut hidden = Vec::new();
        let flush = |chunk: &mut MeshData, made: &mut Vec<Entity>, ctx: &mut Context| {
            if chunk.indices.is_empty() {
                return;
            }
            if let Ok(mesh) = ctx.gfx.create_mesh(chunk) {
                let e = ctx.world.spawn((Name("Static Batch".into()), DontSave, Transform::default(), GlobalTransform::default(), MeshRenderer::new(mesh, material).with_layer(layer).with_sorting_order(order)));
                made.push(e);
            }
            *chunk = MeshData::default();
        };
        for (e, data, m) in parts {
            if chunk.vertices.len() + data.vertices.len() > 65_535 {
                flush(&mut chunk, &mut made, ctx);
            }
            let base = chunk.vertices.len() as u16;
            let nm = m.inverse().transpose();
            chunk.vertices.extend(data.vertices.iter().map(|v| {
                let mut o: Vertex = *v;
                let (p, n) = (m.transform_point3(Vec3::from(v.position)), nm.transform_vector3(Vec3::from(v.normal)).normalize_or(Vec3::Y));
                o.position = p.into();
                o.normal = n.into();
                if let Some(l) = &lights {
                    let c = l.at(p, n);
                    o.color = [v.color[0] * c.x, v.color[1] * c.y, v.color[2] * c.z, v.color[3]];
                }
                o
            }));
            chunk.indices.extend(data.indices.iter().map(|i| i + base));
            hidden.push(e);
        }
        flush(&mut chunk, &mut made, ctx);
        for e in hidden {
            if let Ok(mut r) = ctx.world.get::<&mut MeshRenderer>(e) {
                r.visible = false;
            }
        }
    }
    made
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AssetIo;
    use goat_gfx::GraphicsBackend;
    use goat_runtime::{Engine, Game};

    struct G;
    impl Game for G {
        fn start(&mut self, ctx: &mut Context) {
            ctx.resources.insert(AssetServer::new(AssetIo::new("/nonexistent")));
            let mesh = crate::mesh(ctx, "builtin:cube").unwrap();
            let sphere = crate::mesh(ctx, "builtin:sphere").unwrap();
            let mat = crate::material(ctx, "builtin:default").unwrap();
            let root = ctx.world.spawn((Name("Level".into()), Transform::default(), StaticBatching::default()));
            for i in 0..5 {
                ctx.world.spawn((Transform::from_position(Vec3::new(i as f32 * 2.0, 0.0, 0.0)), Parent(root), MeshRenderer::new(if i < 4 { mesh } else { sphere }, mat)));
            }
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(StaticBatchPlugin)]
        }
    }

    #[test]
    fn statics_merge_into_one_draw() {
        let mut gfx = goat_gfx::NullBackend::new();
        let mut e = Engine::new(G);
        e.frame(0.0, &mut gfx);
        e.frame(1.0 / 60.0, &mut gfx);
        let batches: Vec<Entity> = e.world.query::<&Name>().iter().filter(|(_, n)| n.0 == "Static Batch").map(|(e, _)| e).collect();
        assert_eq!(batches.len(), 1, "one material: one batch");
        let hidden = e.world.query::<&MeshRenderer>().iter().filter(|(_, r)| !r.visible).count();
        assert_eq!(hidden, 5);
        // the merged mesh spans the 5 objects (x from -0.5 to 8.5)
        let b = gfx.mesh_bounds(e.world.get::<&MeshRenderer>(batches[0]).unwrap().mesh).unwrap();
        assert!((b.center.x - 4.0).abs() < 0.6, "{:?}", b);
    }
}
#[cfg(test)]
mod bake_tests {
    use super::*;
    use crate::AssetIo;
    use goat_runtime::{DirectionalLight, Engine, Game, RenderSettings};

    struct G;
    impl Game for G {
        fn start(&mut self, ctx: &mut Context) {
            ctx.resources.insert(AssetServer::new(AssetIo::new("/nonexistent")));
            let mesh = crate::mesh(ctx, "builtin:cube").unwrap();
            let mat = crate::material(ctx, "builtin:default").unwrap();
            ctx.world.spawn((RenderSettings { ambient: goat_core::Color::rgb(0.2, 0.2, 0.2), ambient_ground: None, fog: None },));
            // sun straight down
            ctx.world.spawn((Transform::default().with_rotation(goat_core::math::Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), DirectionalLight { color: goat_core::Color::WHITE, intensity: 0.5 }));
            let root = ctx.world.spawn((Transform::default(), StaticBatching { bake_lighting: true }));
            ctx.world.spawn((Transform::default(), Parent(root), MeshRenderer::new(mesh, mat)));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(StaticBatchPlugin)]
        }
    }

    #[test]
    fn baked_vertices_carry_the_lighting() {
        let mut gfx = goat_gfx::NullBackend::new();
        let mut e = Engine::new(G);
        e.frame(0.0, &mut gfx);
        e.frame(1.0 / 60.0, &mut gfx);
        let r = e.world.query::<(&Name, &MeshRenderer)>().iter().find(|(_, (n, _))| n.0 == "Static Batch").map(|(_, (_, r))| *r).expect("a batch even for one object");
        assert_eq!(gfx.material(r.material).unwrap().lighting, goat_gfx::LightingModel::Unlit);
    }

    #[test]
    fn light_math() {
        let l = SceneLights { ambient: Vec3::splat(0.2), ground: Some(Vec3::ZERO), lights: vec![goat_gfx::Light { kind: goat_gfx::LightKind::Directional, position: Vec3::ZERO, direction: Vec3::NEG_Y, color: goat_core::Color::rgb(0.5, 0.5, 0.5), range: 0.0 }] };
        assert!((l.at(Vec3::ZERO, Vec3::Y) - Vec3::splat(0.7)).length() < 1e-5, "top: sky ambient + sun");
        assert!(l.at(Vec3::ZERO, Vec3::NEG_Y).length() < 1e-5, "bottom: ground ambient, no sun");
    }
}