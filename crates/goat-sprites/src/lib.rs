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

pub mod anim;
#[cfg(feature = "scene")]
pub mod scene;
#[cfg(feature = "script")]
pub mod script;

pub use anim::{SpriteAnimator, SpriteClip};

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
    /// Shown only inside / outside the SpriteMasks (Unity's Mask Interaction).
    pub mask_interaction: MaskInteraction,
}

/// How a SpriteRenderer reacts to [`SpriteMask`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "scene", derive(serde::Serialize, serde::Deserialize))]
pub enum MaskInteraction {
    #[default]
    None,
    VisibleInsideMask,
    VisibleOutsideMask,
}

/// A mask for sprites (Unity's SpriteMask): its rectangle (`size` world units around the
/// object, scaled by it; zero = its own SpriteRenderer's sprite) shows / hides the sprites whose
/// `mask_interaction` asks for it. Rectangular masks, sprites clipped exactly (uv included);
/// rotated sprites are shown / hidden whole by their center.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "scene", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "scene", serde(default))]
pub struct SpriteMask {
    pub size: Vec2,
    pub center: Vec2,
    /// Masks only sprites with a sorting order in this range (Unity's custom range); None = all.
    pub sorting_range: Option<(i32, i32)>,
}

impl SpriteRenderer {
    pub fn new(sprite: Sprite) -> Self {
        SpriteRenderer { sprite, color: Color::WHITE, flip_x: false, flip_y: false, visible: true, layer: 0, sorting_order: 0, blend: BlendMode::Alpha, mask_interaction: MaskInteraction::None }
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

/// Clips an axis-aligned quad (corners 0 = bottom-left, 2 = top-right) to a rectangle in its XY
/// plane, uv interpolated; false when nothing is left. (UI masks use it too.)
pub fn clip_quad(q: &mut Quad, min: Vec2, max: Vec2) -> bool {
    let (x0, y0) = (q.corners[0].x, q.corners[0].y);
    let (x1, y1) = (q.corners[2].x, q.corners[2].y);
    if x1 <= min.x || x0 >= max.x || y1 <= min.y || y0 >= max.y || x1 <= x0 || y1 <= y0 {
        return false;
    }
    let (nx0, nx1, ny0, ny1) = (x0.max(min.x), x1.min(max.x), y0.max(min.y), y1.min(max.y));
    if (nx0, nx1, ny0, ny1) == (x0, x1, y0, y1) {
        return true;
    }
    let fx = |x: f32| (x - x0) / (x1 - x0);
    let fy = |y: f32| (y - y0) / (y1 - y0);
    let uv = q.uv;
    let at = |u: f32, v: f32| -> [f32; 2] {
        let bottom = [uv[0][0] + (uv[1][0] - uv[0][0]) * u, uv[0][1] + (uv[1][1] - uv[0][1]) * u];
        let top = [uv[3][0] + (uv[2][0] - uv[3][0]) * u, uv[3][1] + (uv[2][1] - uv[3][1]) * u];
        [bottom[0] + (top[0] - bottom[0]) * v, bottom[1] + (top[1] - bottom[1]) * v]
    };
    let z = q.corners[0].z;
    q.corners = [Vec3::new(nx0, ny0, z), Vec3::new(nx1, ny0, z), Vec3::new(nx1, ny1, z), Vec3::new(nx0, ny1, z)];
    q.uv = [at(fx(nx0), fy(ny0)), at(fx(nx1), fy(ny0)), at(fx(nx1), fy(ny1)), at(fx(nx0), fy(ny1))];
    true
}

/// Is a quad axis-aligned in XY (no rotation)?
fn axis_aligned(q: &Quad) -> bool {
    let c = &q.corners;
    (c[0].y - c[1].y).abs() < 1e-5 && (c[0].x - c[3].x).abs() < 1e-5 && c[1].x > c[0].x && c[3].y > c[0].y
}

/// Mask interaction: inside = the parts within any mask rect; outside = what no rect covers.
fn apply_masks(q: Quad, rects: &[(Vec2, Vec2)], inside: bool, out: &mut Vec<Quad>) {
    if !axis_aligned(&q) {
        // rotated sprites: whole, by their center
        let c = (q.corners[0] + q.corners[2]) * 0.5;
        let within = rects.iter().any(|(a, b)| c.x >= a.x && c.x <= b.x && c.y >= a.y && c.y <= b.y);
        if within == inside {
            out.push(q);
        }
        return;
    }
    if inside {
        for &(a, b) in rects {
            let mut piece = q;
            if clip_quad(&mut piece, a, b) {
                out.push(piece);
            }
        }
        return;
    }
    // outside: subtract every rect (up to 4 strips per rect)
    let mut pieces = vec![q];
    let big = 1e9;
    for &(a, b) in rects {
        let mut next = Vec::new();
        for p in pieces {
            for (lo, hi) in [(Vec2::new(-big, -big), Vec2::new(a.x, big)), (Vec2::new(b.x, -big), Vec2::new(big, big)), (Vec2::new(a.x, -big), Vec2::new(b.x, a.y)), (Vec2::new(a.x, b.y), Vec2::new(b.x, big))] {
                let mut s = p;
                if clip_quad(&mut s, lo, hi) {
                    next.push(s);
                }
            }
        }
        pieces = next;
    }
    out.extend(pieces);
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
        {
            let r = goat_scene::registry(_ctx);
            r.component(scene::CODEC).component(goat_scene::serde_codec::<SpriteAnimator>("SpriteAnimator")).component(goat_scene::serde_codec::<SpriteMask>("SpriteMask"));
            r.describe("SpriteMask", goat_scene::ComponentInfo::new("Rendering/Sprite Mask").tip("size", "World units (scaled by the object); zero = its own sprite"));
        }
        #[cfg(feature = "script")]
        script::register(goat_script::extensions(_ctx));
    }

    /// Sprite animations (play mode).
    fn update(&mut self, ctx: &mut Context) {
        anim::update(ctx);
    }

    fn late_update(&mut self, ctx: &mut Context) {
        // world rectangles of the masks
        let masks: Vec<(Vec2, Vec2, Option<(i32, i32)>)> = ctx
            .world
            .query::<(&SpriteMask, &GlobalTransform, Option<&SpriteRenderer>)>()
            .without::<&goat_runtime::InactiveInHierarchy>()
            .iter()
            .map(|(_, (m, g, r))| {
                let size = if m.size == Vec2::ZERO { r.map(|r| r.sprite.size()).unwrap_or(Vec2::ONE) } else { m.size };
                let (s, _, t) = g.0.to_scale_rotation_translation();
                let c = Vec2::new(t.x, t.y) + m.center * s.truncate();
                let h = size * s.truncate().abs() * 0.5;
                (c - h, c + h, m.sorting_range)
            })
            .collect();
        let mut quads: Vec<Quad> = Vec::new();
        for (_, (r, g)) in ctx.world.query::<(&SpriteRenderer, &GlobalTransform)>().without::<&goat_runtime::InactiveInHierarchy>().iter() {
            if !r.visible || r.color.a <= 0.0 {
                continue;
            }
            let q = quad_of(r, &g.0);
            if r.mask_interaction == MaskInteraction::None {
                quads.push(q);
                continue;
            }
            let rects: Vec<(Vec2, Vec2)> = masks.iter().filter(|(_, _, range)| range.is_none_or(|(lo, hi)| (lo..=hi).contains(&r.sorting_order))).map(|(a, b, _)| (*a, *b)).collect();
            apply_masks(q, &rects, r.mask_interaction == MaskInteraction::VisibleInsideMask, &mut quads);
        }
        self.batcher.submit(ctx, quads);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goat_core::handle::Pool;

    #[test]
    fn sprite_masks_clip_inside_and_outside() {
        let mut pool: Pool<goat_gfx::TextureData> = Pool::new();
        let tex = pool.insert(goat_gfx::TextureData::solid(Color::WHITE));
        // a 2×2 sprite at the origin (100 px per unit, 200×200 texture), a mask over its right half
        let q = quad_of(&SpriteRenderer::new(Sprite::new(tex, 200, 200)), &Mat4::IDENTITY);
        let rect = [(Vec2::new(0.0, -5.0), Vec2::new(5.0, 5.0))];
        let mut inside = Vec::new();
        apply_masks(q, &rect, true, &mut inside);
        assert_eq!(inside.len(), 1);
        assert_eq!((inside[0].corners[0].x, inside[0].corners[2].x), (0.0, 1.0));
        assert!((inside[0].uv[0][0] - 0.5).abs() < 1e-5, "uv follows the cut");
        let mut outside = Vec::new();
        apply_masks(q, &rect, false, &mut outside);
        assert_eq!(outside.len(), 1);
        assert_eq!((outside[0].corners[0].x, outside[0].corners[2].x), (-1.0, 0.0));
        // a hole in the middle leaves 4 strips around it
        let hole = [(Vec2::new(-0.5, -0.5), Vec2::new(0.5, 0.5))];
        let mut ring = Vec::new();
        apply_masks(q, &hole, false, &mut ring);
        let area: f32 = ring.iter().map(|q| (q.corners[2].x - q.corners[0].x) * (q.corners[2].y - q.corners[0].y)).sum();
        assert_eq!(ring.len(), 4);
        assert!((area - 3.0).abs() < 1e-4);
    }
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
