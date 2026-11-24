//! Static batching (Unity's): on a root with [`StaticBatching`], every visible descendant
//! MeshRenderer whose mesh has an asset key (builtins, model meshes) is baked into world space
//! and merged with the others of the same material / layer / order — one mesh (split at 65 535
//! vertices) and one draw call per group. The originals are hidden, not removed (colliders,
//! scripts keep working on them).

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
        let roots: Vec<Entity> = ctx.world.query::<&StaticBatching>().without::<&Batched>().iter().map(|(e, _)| e).collect();
        for root in roots {
            let made = build(ctx, root);
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

/// Merges the root's meshes; returns the batch entities.
pub fn build(ctx: &mut Context, root: Entity) -> Vec<Entity> {
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
        if parts.len() < 2 {
            continue; // nothing to gain
        }
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
                o.position = m.transform_point3(Vec3::from(v.position)).into();
                o.normal = nm.transform_vector3(Vec3::from(v.normal)).normalize_or(Vec3::Y).into();
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
            let root = ctx.world.spawn((Name("Level".into()), Transform::default(), StaticBatching));
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