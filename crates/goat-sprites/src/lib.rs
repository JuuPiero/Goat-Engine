//! 2D sprites (Unity's Sprite / SpriteRenderer).
//!
//! - [`Sprite`]: a rectangle of a texture, a pivot and a pixels-per-unit scale (sprite sheets:
//!   [`Sprite::grid`]).
//! - [`SpriteRenderer`]: draws a sprite at the entity's transform (color tint, flips, layer,
//!   sorting order).
//! - [`SpritePlugin`]: every frame, after transforms, sorts the visible sprites (sorting order,
//!   then back to front along Z) and merges consecutive sprites that share a texture into one
//!   mesh: one draw call per run instead of one per sprite (the 3DS's CPU is the bottleneck).
//!
//! Add it with `fn plugins(&mut self) -> Vec<Box<dyn Plugin>> { vec![Box::new(SpritePlugin::default())] }`.

#[cfg(feature = "scene")]
pub mod scene;

use std::collections::HashMap;

use goat_core::math::{Mat4, Vec2, Vec3};
use goat_core::Color;
use goat_gfx::{BlendMode, CullMode, LightingModel, MaterialDesc, MaterialId, MeshData, MeshId, TextureId, Vertex};
use goat_runtime::{Context, DontSave, GlobalTransform, MeshRenderer, Name, Plugin, Transform};

/// A pixel rectangle of a texture (origin top-left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A rectangle of a texture drawn as a quad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    pub texture: TextureId,
    /// Size of the whole texture in pixels.
    pub texture_size: (u32, u32),
    pub rect: PixelRect,
    /// Pivot in the rect, 0..1 from the bottom-left (Unity's convention); (0.5, 0.5) = center.
    pub pivot: Vec2,
    /// Pixels per world unit (100 = Unity's default; 1 = one unit per pixel for pixel-exact HUDs).
    pub pixels_per_unit: f32,
}

impl Sprite {
    /// The whole texture.
    pub fn new(texture: TextureId, width: u32, height: u32) -> Self {
        Sprite {
            texture,
            texture_size: (width, height),
            rect: PixelRect { x: 0.0, y: 0.0, width: width as f32, height: height as f32 },
            pivot: Vec2::splat(0.5),
            pixels_per_unit: 100.0,
        }
    }

    /// Cell `index` of a sheet cut into `cell_w × cell_h` cells (row by row, from the top-left).
    pub fn grid(texture: TextureId, width: u32, height: u32, cell_w: u32, cell_h: u32, index: u32) -> Self {
        let columns = (width / cell_w.max(1)).max(1);
        let (cx, cy) = (index % columns, index / columns);
        Sprite { rect: PixelRect { x: (cx * cell_w) as f32, y: (cy * cell_h) as f32, width: cell_w as f32, height: cell_h as f32 }, ..Sprite::new(texture, width, height) }
    }

    pub fn with_pixels_per_unit(mut self, ppu: f32) -> Self {
        self.pixels_per_unit = ppu.max(1e-4);
        self
    }
    pub fn with_pivot(mut self, pivot: Vec2) -> Self {
        self.pivot = pivot;
        self
    }

    /// Size in world units.
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.rect.width, self.rect.height) / self.pixels_per_unit
    }
}

/// Draws a [`Sprite`] at the entity's transform (in its local XY plane, facing +Z).
#[derive(Clone, Copy, Debug)]
pub struct SpriteRenderer {
    pub sprite: Sprite,
    /// Tint (multiplies the texture), alpha included.
    pub color: Color,
    pub flip_x: bool,
    pub flip_y: bool,
    pub visible: bool,
    /// Layer 0..31 (cameras' culling masks).
    pub layer: u8,
    /// Lower draws first (Unity's Order in Layer).
    pub sorting_order: i32,
    /// Alpha (default) or Additive (glows, particles).
    pub blend: BlendMode,
}

impl SpriteRenderer {
    pub fn new(sprite: Sprite) -> Self {
        SpriteRenderer { sprite, color: Color::WHITE, flip_x: false, flip_y: false, visible: true, layer: 0, sorting_order: 0, blend: BlendMode::Alpha }
    }
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
    pub fn with_layer(mut self, layer: u8) -> Self {
        self.layer = layer.min(31);
        self
    }
    pub fn with_sorting_order(mut self, order: i32) -> Self {
        self.sorting_order = order;
        self
    }
}

/// Marks the helper entities that hold quad batches. Their own archetype keeps them in spawn
/// order, which is the draw order of batches with equal sorting orders.
pub struct SpriteBatch;

struct Batch {
    entity: hecs::Entity,
    mesh: MeshId,
}

/// One textured quad in world space, before batching (sprites, text glyphs, particles, UI).
#[derive(Clone, Copy, Debug)]
pub struct Quad {
    pub sorting_order: i32,
    pub layer: u8,
    pub texture: TextureId,
    pub blend: BlendMode,
    /// Sort depth (lower draws first).
    pub z: f32,
    /// Bottom-left, bottom-right, top-right, top-left.
    pub corners: [Vec3; 4],
    pub normal: Vec3,
    pub uv: [[f32; 2]; 4],
    pub color: [f32; 4],
}

impl Quad {
    fn key(&self) -> (i32, u8, TextureId, u8) {
        (self.sorting_order, self.layer, self.texture, self.blend as u8)
    }
}

/// Max quads per mesh (u16 indices: 65 536 vertices).
const MAX_QUADS: usize = 65536 / 4;

/// Turns a frame's quads into as few meshes as possible: sorted by (sorting order, z, texture),
/// consecutive quads sharing (order, layer, texture, blend) become one mesh, drawn with an unlit
/// blended material (no depth write, no fog). Each producer (sprites, text...) owns one.
pub struct Batcher {
    name: &'static str,
    materials: HashMap<(TextureId, u8), MaterialId>,
    batches: Vec<Batch>,
    /// Quads / batches of the last submit.
    pub stats: (usize, usize),
}

impl Batcher {
    pub fn new(name: &'static str) -> Self {
        Batcher { name, materials: HashMap::new(), batches: Vec::new(), stats: (0, 0) }
    }

    fn material(&mut self, ctx: &mut Context, texture: TextureId, blend: u8) -> Option<MaterialId> {
        if let Some(m) = self.materials.get(&(texture, blend)) {
            return Some(*m);
        }
        let blend_mode = if blend == BlendMode::Additive as u8 { BlendMode::Additive } else { BlendMode::Alpha };
        let desc = MaterialDesc { texture: Some(texture), lighting: LightingModel::Unlit, blend: blend_mode, cull: CullMode::None, depth_write: false, fog: false, ..Default::default() };
        let m = ctx.gfx.create_material(&desc).ok()?;
        self.materials.insert((texture, blend), m);
        Some(m)
    }

    /// The `i`-th batch (entity + mesh), created on demand and reused every frame.
    fn batch(&mut self, ctx: &mut Context, i: usize, material: MaterialId) -> Option<(hecs::Entity, MeshId)> {
        while self.batches.len() <= i {
            let mesh = ctx.gfx.create_mesh(&MeshData::default()).ok()?;
            let name = Name(format!("{} Batch {}", self.name, self.batches.len()));
            let entity = ctx.world.spawn((name, SpriteBatch, DontSave, Transform::default(), GlobalTransform::default(), MeshRenderer::new(mesh, material)));
            self.batches.push(Batch { entity, mesh });
        }
        let b = &self.batches[i];
        Some((b.entity, b.mesh))
    }

    /// Draws `quads` this frame (call once per frame from a plugin's `late_update`).
    pub fn submit(&mut self, ctx: &mut Context, mut quads: Vec<Quad>) {
        // draw order: sorting order, then back to front (lower z first), then texture (longer runs)
        quads.sort_by(|a, b| {
            a.sorting_order.cmp(&b.sorting_order).then(a.z.partial_cmp(&b.z).unwrap_or(std::cmp::Ordering::Equal)).then(a.texture.index().cmp(&b.texture.index()))
        });
        let mut used = 0;
        let mut start = 0;
        while start < quads.len() {
            let key = quads[start].key();
            let mut end = start + 1;
            while end < quads.len() && quads[end].key() == key && end - start < MAX_QUADS {
                end += 1;
            }
            let Some(material) = self.material(ctx, key.2, key.3) else { break };
            let mut mesh = MeshData { vertices: Vec::with_capacity((end - start) * 4), indices: Vec::with_capacity((end - start) * 6) };
            for q in &quads[start..end] {
                let base = mesh.vertices.len() as u16;
                for k in 0..4 {
                    mesh.vertices.push(Vertex { position: q.corners[k].into(), normal: q.normal.into(), uv: q.uv[k], color: q.color });
                }
                mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
            let Some((entity, mesh_id)) = self.batch(ctx, used, material) else { break };
            if ctx.gfx.update_mesh(mesh_id, &mesh).is_ok() {
                if let Ok(mut r) = ctx.world.get::<&mut MeshRenderer>(entity) {
                    *r = MeshRenderer::new(mesh_id, material).with_layer(key.1).with_sorting_order(key.0);
                }
                used += 1;
            }
            start = end;
        }
        for b in &self.batches[used..] {
            if let Ok(mut r) = ctx.world.get::<&mut MeshRenderer>(b.entity) {
                r.visible = false;
            }
        }
        self.stats = (quads.len(), used);
    }
}

fn quad_of(r: &SpriteRenderer, m: &Mat4) -> Quad {
    let s = &r.sprite;
    let size = s.size();
    let (x0, y0) = (-s.pivot.x * size.x, -s.pivot.y * size.y);
    let (x1, y1) = (x0 + size.x, y0 + size.y);
    // corners: bottom-left, bottom-right, top-right, top-left
    let local = [Vec3::new(x0, y0, 0.0), Vec3::new(x1, y0, 0.0), Vec3::new(x1, y1, 0.0), Vec3::new(x0, y1, 0.0)];
    let corners = local.map(|p| m.transform_point3(p));
    let (tw, th) = (s.texture_size.0.max(1) as f32, s.texture_size.1.max(1) as f32);
    let (mut u0, mut u1) = (s.rect.x / tw, (s.rect.x + s.rect.width) / tw);
    // texture v grows downwards: the sprite's top edge = rect.y
    let (mut v_top, mut v_bottom) = (s.rect.y / th, (s.rect.y + s.rect.height) / th);
    if r.flip_x {
        std::mem::swap(&mut u0, &mut u1);
    }
    if r.flip_y {
        std::mem::swap(&mut v_top, &mut v_bottom);
    }
    let c = r.color;
    Quad {
        sorting_order: r.sorting_order,
        layer: r.layer,
        texture: s.texture,
        blend: r.blend,
        z: m.w_axis.z,
        corners,
        normal: m.transform_vector3(Vec3::Z).normalize_or(Vec3::Z),
        uv: [[u0, v_bottom], [u1, v_bottom], [u1, v_top], [u0, v_top]],
        color: [c.r, c.g, c.b, c.a],
    }
}

/// Batches sprites into meshes every frame. See the crate docs.
pub struct SpritePlugin {
    batcher: Batcher,
}

impl Default for SpritePlugin {
    fn default() -> Self {
        SpritePlugin { batcher: Batcher::new("Sprite") }
    }
}

impl SpritePlugin {
    /// Sprites drawn / batches used last frame.
    pub fn stats(&self) -> (usize, usize) {
        self.batcher.stats
    }
}

impl Plugin for SpritePlugin {
    fn in_edit_mode(&self) -> bool {
        true
    }
    fn build(&mut self, _ctx: &mut Context) {
        #[cfg(feature = "scene")]
        goat_scene::registry(_ctx).component(scene::CODEC);
    }

    fn late_update(&mut self, ctx: &mut Context) {
        let quads: Vec<Quad> = ctx
            .world
            .query::<(&SpriteRenderer, &GlobalTransform)>()
            .iter()
            .filter(|(_, (r, _))| r.visible && r.color.a > 0.0)
            .map(|(_, (r, g))| quad_of(r, &g.0))
            .collect();
        self.batcher.submit(ctx, quads);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goat_core::handle::Pool;

    #[test]
    fn grid_cells_and_quad_uvs() {
        let mut pool: Pool<goat_gfx::TextureData> = Pool::new();
        let tex = pool.insert(goat_gfx::TextureData::solid(Color::WHITE));
        let s = Sprite::grid(tex, 64, 32, 16, 16, 5).with_pixels_per_unit(16.0);
        assert_eq!(s.rect, PixelRect { x: 16.0, y: 16.0, width: 16.0, height: 16.0 });
        assert_eq!(s.size(), Vec2::ONE);
        let q = quad_of(&SpriteRenderer::new(s), &Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)));
        assert_eq!(q.corners[0], Vec3::new(9.5, -0.5, 0.0));
        assert_eq!(q.corners[2], Vec3::new(10.5, 0.5, 0.0));
        // bottom-left corner samples the bottom of the cell (v = 1.0), top-right its top (v = 0.5)
        assert_eq!(q.uv[0], [0.25, 1.0]);
        assert_eq!(q.uv[2], [0.5, 0.5]);
    }
}
