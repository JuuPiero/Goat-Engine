//! Blob shadows (feature `render`): a soft dark disc on the ground under an object — the cheap
//! shadow every platform can afford (one batched quad each; the 3DS has no spare fill rate for
//! shadow maps in busy scenes). The ground is found with a physics raycast straight down; the
//! disc shrinks and fades with height.

use goat_core::math::{Mat4, Quat, Vec3};
use goat_gfx::{BlendMode, Filter, TextureData, TextureId, Wrap};
use goat_runtime::{Context, Entity, GlobalTransform, Plugin};
use goat_sprites::{Batcher, Quad};
use serde::{Deserialize, Serialize};

use crate::Physics;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BlobShadow {
    /// Diameter on the ground when the object touches it (world units).
    pub size: f32,
    /// Darkness 0..1 at ground level.
    pub opacity: f32,
    /// No shadow above this height.
    pub max_distance: f32,
    /// Layers the ground ray hits.
    pub layer_mask: u32,
}

impl Default for BlobShadow {
    fn default() -> Self {
        BlobShadow { size: 1.0, opacity: 0.55, max_distance: 6.0, layer_mask: u32::MAX }
    }
}

/// A 64×64 radial falloff (black, alpha from the middle).
fn blob_texture() -> TextureData {
    let n = 64u32;
    let rgba = (0..n * n)
        .flat_map(|i| {
            let (x, y) = ((i % n) as f32 + 0.5 - n as f32 * 0.5, (i / n) as f32 + 0.5 - n as f32 * 0.5);
            let d = (x * x + y * y).sqrt() / (n as f32 * 0.5);
            let a = (1.0 - d).clamp(0.0, 1.0);
            [0u8, 0, 0, (a * a * (3.0 - 2.0 * a) * 255.0) as u8]
        })
        .collect();
    TextureData { width: n, height: n, rgba, filter: Filter::Linear, wrap: Wrap::Clamp, mipmaps: false, format: Default::default() }
}

/// Draws the [`BlobShadow`]s (add it after PhysicsPlugin).
pub struct BlobShadowPlugin {
    batcher: Batcher,
    texture: Option<TextureId>,
}

impl Default for BlobShadowPlugin {
    fn default() -> Self {
        BlobShadowPlugin { batcher: Batcher::new("Blob Shadows"), texture: None }
    }
}

impl Plugin for BlobShadowPlugin {
    fn build(&mut self, _ctx: &mut Context) {
        #[cfg(feature = "scene")]
        goat_scene::registry(_ctx).component(goat_scene::serde_codec::<BlobShadow>("BlobShadow"));
    }

    fn late_update(&mut self, ctx: &mut Context) {
        if self.texture.is_none() {
            self.texture = ctx.gfx.create_texture(&blob_texture()).ok();
        }
        let Some(texture) = self.texture else { return };
        let Some(physics) = ctx.resources.get::<Physics>() else { return };
        let casters: Vec<(Entity, BlobShadow, Vec3)> = ctx.world.query::<(&BlobShadow, &GlobalTransform)>().iter().map(|(e, (b, g))| (e, *b, g.0.w_axis.truncate())).collect();
        let mut quads = Vec::with_capacity(casters.len());
        for (e, b, p) in casters {
            // the ground under it (the caster's own colliders ignored)
            let Some(h) = physics.raycast_excluding(p, Vec3::NEG_Y, b.max_distance, b.layer_mask, e) else { continue };
            let height = (p - h.point).length();
            let t = (height / b.max_distance.max(1e-3)).clamp(0.0, 1.0);
            let alpha = b.opacity * (1.0 - t);
            if alpha <= 0.01 {
                continue;
            }
            let half = b.size * 0.5 * (1.0 - 0.5 * t);
            let rot = Quat::from_rotation_arc(Vec3::Y, h.normal.normalize_or(Vec3::Y));
            let m = Mat4::from_rotation_translation(rot, h.point + h.normal * 0.02);
            let c = |x: f32, z: f32| m.transform_point3(Vec3::new(x * half, 0.0, z * half));
            quads.push(Quad {
                sorting_order: -100,
                layer: 0,
                texture,
                blend: BlendMode::Alpha,
                z: -1e6,
                corners: [c(-1.0, 1.0), c(1.0, 1.0), c(1.0, -1.0), c(-1.0, -1.0)],
                normal: h.normal,
                uv: [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                color: [1.0, 1.0, 1.0, alpha],
            });
        }
        self.batcher.submit(ctx, quads);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Collider, PhysicsPlugin, Shape};
    use goat_runtime::{Engine, Game, MeshRenderer, Name, Transform};

    struct G;
    impl Game for G {
        fn start(&mut self, ctx: &mut Context) {
            ctx.world.spawn((Transform::from_position(Vec3::new(0.0, -0.5, 0.0)).with_scale(Vec3::new(20.0, 1.0, 20.0)), GlobalTransform::default(), Collider::unit_box()));
            ctx.world.spawn((Name("ball".into()), Transform::from_position(Vec3::new(1.0, 2.0, 0.0)), GlobalTransform::default(), BlobShadow::default(), Collider::new(Shape::Sphere(0.5))));
            // and one without a collider
            ctx.world.spawn((Transform::from_position(Vec3::new(-3.0, 1.0, 0.0)), GlobalTransform::default(), BlobShadow::default()));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(PhysicsPlugin), Box::new(BlobShadowPlugin::default())]
        }
    }

    #[test]
    fn a_shadow_lies_on_the_ground_under_the_caster() {
        let mut gfx = goat_gfx::NullBackend::new();
        let mut e = Engine::new(G);
        for _ in 0..5 {
            e.frame(1.0 / 60.0, &mut gfx);
        }
        let batch = e.world.query::<(&Name, &MeshRenderer)>().iter().find(|(_, (n, _))| n.0.contains("Blob")).map(|(_, (_, r))| r.mesh);
        let mesh = batch.expect("a shadow batch was drawn");
        let b = goat_gfx::GraphicsBackend::mesh_bounds(&gfx, mesh).unwrap();
        // two discs: under x = 1 and x = -3, on the ground (y ≈ 0)
        assert!((b.center.x + 1.0).abs() < 0.1 && b.center.y.abs() < 0.05, "both discs on the ground: {:?}", b);
        assert!(b.radius > 2.0);
    }
}