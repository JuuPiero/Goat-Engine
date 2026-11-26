//! Low-level graphics like Unity's / PiaEngine's: `Graphics.DrawMesh(Instanced)`, `GL`,
//! `Debug.DrawLine`, `Graphics.Blit`, render textures as camera targets, the Post Effect component
//! and per-renderer colors (MaterialPropertyBlock `_Color`).
//!
//! Everything submitted here lives for **one frame** (submit again every update), except
//! `Debug.DrawLine` with a duration. The queue is a [`GraphicsQueue`] resource; [`apply`] turns it
//! into draw items / extra views right after the cameras' views are extracted, so every backend
//! draws it with its normal pipeline (no backend code beyond render targets).

use std::collections::HashMap;

use goat_core::math::{Mat4, Vec3};
use goat_core::Color;
use goat_gfx::{BlendMode, CullMode, Display, DrawItem, GraphicsBackend, Lighting, LightingModel, MaterialDesc, MaterialId, MeshData, MeshId, Projection, RenderTargetDesc, RenderView, TextureId, Vertex, Viewport};
use hecs::{Entity, World};

use crate::components::Camera;
use crate::resources::Resources;

/// Colors one renderer without a material of its own (Unity's MaterialPropertyBlock `_Color` /
/// `Renderer.SetPropertyBlock`): multiplies the shared material's color for this object only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropertyBlock {
    pub color: Color,
}

impl Default for PropertyBlock {
    fn default() -> Self {
        PropertyBlock { color: Color::WHITE }
    }
}

/// Screen effects on a camera (PiaEngine's Post Effect component, Unity's `OnRenderImage`): the
/// camera draws into a texture, then each material is drawn over the whole view with the previous
/// image as its texture (`_MainTex`), the last one into the camera's real destination.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PostEffect {
    pub materials: Vec<MaterialId>,
}

/// Script gizmos (`OnDrawGizmos` with `Gizmos.DrawWireSphere`, `Handles.Label`...): world line
/// segments and labels the editor's Scene view draws. Filled by whoever runs the scripts' gizmo
/// methods (the ScriptPlugin while playing, the editor in Edit mode), read by the editor.
#[derive(Default, Debug)]
pub struct GizmoQueue {
    pub lines: Vec<(Vec3, Vec3, Color)>,
    pub labels: Vec<(Vec3, String, Color)>,
}

impl GizmoQueue {
    pub fn clear(&mut self) {
        self.lines.clear();
        self.labels.clear();
    }
}

/// Present while an editor shows gizmos: the ScriptPlugin calls `onDrawGizmos` (and
/// `onDrawGizmosSelected` for `selected`) every frame into the [`GizmoQueue`].
#[derive(Default, Debug, Clone)]
pub struct DrawGizmos {
    pub selected: Vec<Entity>,
}

/// Where a blit draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlitDest {
    Texture(TextureId),
    /// The Main display.
    Screen,
    /// A camera's own destination (its display + viewport, or its target texture): onRenderImage's
    /// `dst`.
    Camera(Entity),
}

/// A texture drawn over a destination with a material (`Graphics.Blit`) or into a screen
/// rectangle (`Graphics.DrawTexture`).
#[derive(Clone, Copy, Debug)]
pub struct Blit {
    pub source: TextureId,
    pub dest: BlitDest,
    pub material: Option<MaterialId>,
    /// Pixels of the destination from its top-left (x, y, width, height); None = all of it.
    pub rect: Option<[f32; 4]>,
    /// Alpha blended (DrawTexture) instead of replacing.
    pub alpha: bool,
    pub color: Color,
}

/// A mesh drawn for one frame (`Graphics.DrawMesh`).
#[derive(Clone, Copy, Debug)]
pub struct QueuedDraw {
    pub mesh: MeshId,
    pub material: MaterialId,
    pub model: Mat4,
    pub color: Color,
    pub layer: u8,
    /// Only this camera (None = every camera whose culling mask has `layer`).
    pub camera: Option<Entity>,
}

/// `GL.Begin` modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlMode {
    Lines,
    LineStrip,
    Triangles,
    TriangleStrip,
    Quads,
}

/// Where GL vertices are: the world, the screen 0..1 (`GL.LoadOrtho`) or screen pixels
/// (`GL.LoadPixelMatrix`); both screen spaces start at the bottom-left like Unity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlSpace {
    World,
    Screen01,
    Pixels,
}

struct Prim {
    /// Line segments (pairs) or triangles (triples).
    verts: Vec<(Vec3, Color)>,
    lines: bool,
    space: GlSpace,
    depth_test: bool,
}

struct DebugLine {
    a: Vec3,
    b: Vec3,
    color: Color,
    until: f32,
    depth_test: bool,
}

/// Line width on screen, in pixels.
const LINE_PX: f32 = 1.5;

/// The frame's immediate-mode drawing (one per engine).
#[derive(Default)]
pub struct GraphicsQueue {
    pub draws: Vec<QueuedDraw>,
    debug_lines: Vec<DebugLine>,
    prims: Vec<Prim>,
    blits: Vec<Blit>,
    /// Cameras whose scripts define onRenderImage: their source texture (drawn into this frame).
    image_effects: HashMap<Entity, (TextureId, u32, u32)>,
    image_effects_wanted: Vec<Entity>,
    now: f32,
    // GL state
    gl_mode: Option<GlMode>,
    gl_color: Color,
    gl_verts: Vec<(Vec3, Color)>,
    gl_matrix: Mat4,
    gl_space: GlSpace,
    gl_stack: Vec<(Mat4, GlSpace)>,
    // GPU resources reused frame to frame
    meshes: Vec<MeshId>,
    meshes_used: usize,
    meshes_last: usize,
    line_materials: [Option<MaterialId>; 2],
    quad: Option<MeshId>,
    blit_materials: HashMap<(Option<MaterialId>, TextureId, bool), MaterialId>,
    post_targets: HashMap<(Entity, usize), (TextureId, u32, u32)>,
}

impl Default for GlSpace {
    fn default() -> Self {
        GlSpace::World
    }
}

impl GraphicsQueue {
    /// Start of a frame: drops last frame's submissions (timed debug lines stay until they end).
    pub fn begin_frame(&mut self, now: f32) {
        self.now = now;
        self.draws.clear();
        self.prims.clear();
        self.blits.clear();
        self.image_effects_wanted.clear();
        self.debug_lines.retain(|l| l.until > now);
        self.gl_mode = None;
        self.gl_verts.clear();
        self.gl_matrix = Mat4::IDENTITY;
        self.gl_space = GlSpace::World;
        self.gl_stack.clear();
    }

    /// Everything submitted cleared (Stop in the editor).
    pub fn clear_all(&mut self) {
        self.begin_frame(self.now);
        self.debug_lines.clear();
    }

    /// `Graphics.DrawMesh`.
    pub fn draw_mesh(&mut self, mesh: MeshId, material: MaterialId, model: Mat4, color: Color, layer: u8, camera: Option<Entity>) {
        self.draws.push(QueuedDraw { mesh, material, model, color, layer: layer.min(31), camera });
    }

    /// `Graphics.DrawMeshInstanced`: one draw per matrix (colors: per instance, else white).
    pub fn draw_mesh_instanced(&mut self, mesh: MeshId, material: MaterialId, matrices: &[Mat4], colors: Option<&[Color]>, layer: u8) {
        self.draws.reserve(matrices.len());
        for (i, m) in matrices.iter().enumerate() {
            let color = colors.and_then(|c| c.get(i)).copied().unwrap_or(Color::WHITE);
            self.draws.push(QueuedDraw { mesh, material, model: *m, color, layer: layer.min(31), camera: None });
        }
    }

    /// `Debug.DrawLine` (world space, `duration` seconds; 0 = this frame).
    pub fn debug_line(&mut self, a: Vec3, b: Vec3, color: Color, duration: f32, depth_test: bool) {
        self.debug_lines.push(DebugLine { a, b, color, until: self.now + duration.max(0.0) + 1e-4, depth_test });
    }

    /// `Graphics.Blit(source, dest, material)`: dest None = the screen (Main display).
    pub fn blit(&mut self, source: TextureId, dest: Option<TextureId>, material: Option<MaterialId>) {
        let dest = match dest {
            Some(t) => BlitDest::Texture(t),
            None => BlitDest::Screen,
        };
        self.blits.push(Blit { source, dest, material, rect: None, alpha: false, color: Color::WHITE });
    }

    /// Any blit (see [`Blit`]): `Graphics.Blit` into a camera's destination (onRenderImage), or
    /// `Graphics.DrawTexture` (a screen rectangle, alpha blended, tinted).
    pub fn blit_with(&mut self, b: Blit) {
        self.blits.push(b);
    }

    /// onRenderImage: this camera draws into a texture this frame (made / resized here); returns it.
    pub fn image_effect_source(&mut self, gfx: &mut dyn GraphicsBackend, camera: Entity, width: u32, height: u32) -> Option<TextureId> {
        let (w, h) = (width.max(1), height.max(1));
        let t = match self.image_effects.get(&camera) {
            Some((t, tw, th)) if (*tw, *th) == (w, h) => *t,
            old => {
                if let Some((t, _, _)) = old {
                    gfx.destroy_texture(*t);
                }
                let t = gfx.create_render_target(&RenderTargetDesc { width: w, height: h, depth: true }).ok()?;
                self.image_effects.insert(camera, (t, w, h));
                t
            }
        };
        if !self.image_effects_wanted.contains(&camera) {
            self.image_effects_wanted.push(camera);
        }
        Some(t)
    }

    // ------------------------------------------------------------------ GL
    pub fn gl_begin(&mut self, mode: GlMode) {
        self.gl_mode = Some(mode);
        self.gl_verts.clear();
    }
    pub fn gl_color(&mut self, c: Color) {
        self.gl_color = c;
    }
    pub fn gl_vertex(&mut self, p: Vec3) {
        if self.gl_mode.is_some() {
            let p = self.gl_matrix.transform_point3(p);
            self.gl_verts.push((p, self.gl_color));
        }
    }
    pub fn gl_push_matrix(&mut self) {
        self.gl_stack.push((self.gl_matrix, self.gl_space));
    }
    pub fn gl_pop_matrix(&mut self) {
        if let Some((m, s)) = self.gl_stack.pop() {
            self.gl_matrix = m;
            self.gl_space = s;
        }
    }
    /// `GL.LoadOrtho` / `GL.LoadPixelMatrix` / back to world (`GL.LoadIdentity` keeps the space).
    pub fn gl_load_space(&mut self, space: GlSpace) {
        self.gl_space = space;
        self.gl_matrix = Mat4::IDENTITY;
    }
    pub fn gl_load_identity(&mut self) {
        self.gl_matrix = Mat4::IDENTITY;
    }
    pub fn gl_mult_matrix(&mut self, m: Mat4) {
        self.gl_matrix *= m;
    }
    pub fn gl_end(&mut self) {
        let Some(mode) = self.gl_mode.take() else { return };
        let v = std::mem::take(&mut self.gl_verts);
        let mut out = Vec::new();
        let lines = matches!(mode, GlMode::Lines | GlMode::LineStrip);
        match mode {
            GlMode::Lines => out.extend(v.chunks_exact(2).flatten().copied()),
            GlMode::LineStrip => {
                for w in v.windows(2) {
                    out.extend_from_slice(w);
                }
            }
            GlMode::Triangles => out.extend(v.chunks_exact(3).flatten().copied()),
            GlMode::TriangleStrip => {
                for i in 2..v.len() {
                    if i % 2 == 0 {
                        out.extend_from_slice(&[v[i - 2], v[i - 1], v[i]]);
                    } else {
                        out.extend_from_slice(&[v[i - 1], v[i - 2], v[i]]);
                    }
                }
            }
            GlMode::Quads => {
                for q in v.chunks_exact(4) {
                    out.extend_from_slice(&[q[0], q[1], q[2], q[0], q[2], q[3]]);
                }
            }
        }
        if !out.is_empty() {
            self.prims.push(Prim { verts: out, lines, space: self.gl_space, depth_test: true });
        }
    }

    fn next_mesh(&mut self, gfx: &mut dyn GraphicsBackend, data: &MeshData) -> Option<MeshId> {
        if self.meshes_used < self.meshes.len() {
            let id = self.meshes[self.meshes_used];
            self.meshes_used += 1;
            gfx.update_mesh(id, data).ok()?;
            return Some(id);
        }
        let id = gfx.create_mesh(data).ok()?;
        self.meshes.push(id);
        self.meshes_used += 1;
        Some(id)
    }

    fn line_material(&mut self, gfx: &mut dyn GraphicsBackend, depth_test: bool) -> Option<MaterialId> {
        let slot = &mut self.line_materials[depth_test as usize];
        if slot.is_none() {
            *slot = gfx.create_material(&MaterialDesc { lighting: LightingModel::Unlit, blend: BlendMode::Alpha, cull: CullMode::None, depth_write: false, depth_test, fog: false, ..Default::default() }).ok();
        }
        *slot
    }

    fn quad(&mut self, gfx: &mut dyn GraphicsBackend) -> Option<MeshId> {
        if self.quad.is_none() {
            self.quad = gfx.create_mesh(&MeshData::quad(1.0)).ok();
        }
        self.quad
    }

    /// The material drawing `source` over a view: the user's material (its look, `source` as
    /// its texture) or a plain copy.
    fn blit_material(&mut self, gfx: &mut dyn GraphicsBackend, material: Option<MaterialId>, source: TextureId) -> Option<MaterialId> {
        self.blit_material_with(gfx, material, source, false)
    }

    fn blit_material_with(&mut self, gfx: &mut dyn GraphicsBackend, material: Option<MaterialId>, source: TextureId, alpha: bool) -> Option<MaterialId> {
        if let Some(m) = self.blit_materials.get(&(material, source, alpha)) {
            return Some(*m);
        }
        let base = material.and_then(|m| gfx.material_desc(m)).unwrap_or(MaterialDesc { lighting: LightingModel::Unlit, ..Default::default() });
        // a shader material's _MainTex slot holds the same texture as its standard mapping
        let mut slots = base.shader_textures;
        match base.texture {
            Some(old) => slots.iter_mut().filter(|s| **s == Some(old)).for_each(|s| *s = Some(source)),
            None => slots[0] = Some(source),
        }
        let blend = if alpha { BlendMode::Alpha } else { base.blend };
        let desc = MaterialDesc { texture: Some(source), shader_textures: slots, cull: CullMode::None, depth_test: false, depth_write: false, fog: false, next_pass: None, blend, ..base };
        let id = gfx.create_material(&desc).ok()?;
        self.blit_materials.insert((material, source, alpha), id);
        Some(id)
    }
}

/// Thin camera-facing quads for line segments (`px` = world units per pixel at a point).
fn line_quads(segments: &[(Vec3, Color)], eye: Vec3, forward: Vec3, ortho: bool, px: &dyn Fn(Vec3) -> f32, out: &mut MeshData) {
    for s in segments.chunks_exact(2) {
        let ((a, ca), (b, cb)) = (s[0], s[1]);
        let dir = (b - a).normalize_or(Vec3::X);
        let view = if ortho { -forward } else { (eye - (a + b) * 0.5).normalize_or(Vec3::Z) };
        let side = dir.cross(view).normalize_or(Vec3::Y);
        let (wa, wb) = (side * px(a) * LINE_PX * 0.5, side * px(b) * LINE_PX * 0.5);
        let base = out.vertices.len() as u16;
        for (p, c) in [(a - wa, ca), (a + wa, ca), (b + wb, cb), (b - wb, cb)] {
            out.vertices.push(Vertex { position: p.into(), normal: [0.0, 0.0, 1.0], uv: [0.0, 0.0], color: c.to_array() });
        }
        out.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn triangles(verts: &[(Vec3, Color)], out: &mut MeshData) {
    for (p, c) in verts {
        out.indices.push(out.vertices.len() as u16);
        out.vertices.push(Vertex { position: (*p).into(), normal: [0.0, 0.0, 1.0], uv: [0.0, 0.0], color: c.to_array() });
    }
}

fn overlay_view(display: Display, aspect: f32, depth: i32, items: Vec<DrawItem>, target: Option<TextureId>) -> RenderView {
    let _ = aspect;
    RenderView {
        display,
        viewport: Viewport::FULL,
        view: Mat4::IDENTITY,
        projection: Projection::Orthographic { height: 1.0, near: 0.01, far: 10.0 },
        clear: None,
        clear_depth: true,
        lighting: Lighting::default(),
        fog: None,
        items,
        depth,
        time: 0.0,
        post: None,
        target,
    }
}

/// A view drawing `material` over a whole destination (blits, post effects).
fn blit_view(display: Display, viewport: Viewport, target: Option<TextureId>, aspect: f32, depth: i32, quad: MeshId, material: MaterialId) -> RenderView {
    blit_view_tinted(display, viewport, target, aspect, depth, quad, material, Color::WHITE)
}

#[allow(clippy::too_many_arguments)]
fn blit_view_tinted(display: Display, viewport: Viewport, target: Option<TextureId>, aspect: f32, depth: i32, quad: MeshId, material: MaterialId, color: Color) -> RenderView {
    // the quad (1 × 1) scaled to cover the orthographic view (height 1, width = aspect)
    let model = Mat4::from_translation(Vec3::new(0.0, 0.0, -1.0)) * Mat4::from_scale(Vec3::new(aspect, 1.0, 1.0));
    let item = DrawItem { color, ..DrawItem::new(quad, material, model) };
    RenderView { viewport, clear_depth: false, ..overlay_view(display, aspect, depth, vec![item], target) }
}

/// A pixel rectangle (top-left origin) of a `w × h` destination as a viewport inside `base`.
fn sub_viewport(base: Viewport, rect: Option<[f32; 4]>, w: f32, h: f32) -> (Viewport, f32) {
    match rect {
        Some([x, y, rw, rh]) => {
            let (w, h) = (w.max(1.0), h.max(1.0));
            let vp = Viewport { x: base.x + x / w * base.width, y: base.y + y / h * base.height, width: rw / w * base.width, height: rh / h * base.height };
            (vp, rw.abs().max(1e-3) / rh.abs().max(1e-3))
        }
        None => (base, w / h.max(1.0)),
    }
}

/// Adds the frame's queue to the extracted views (`cameras[i]` drew `views[i]`): queued meshes,
/// debug / GL lines, post effects, blits. Called by the engine after `extract_views`.
/// `game_screen`: the views are the game's screen (false for the editor's Scene view, which shows
/// world lines but not the game's screen-space GL / blits).
pub fn apply(world: &World, resources: &mut Resources, gfx: &mut dyn GraphicsBackend, displays: &[goat_gfx::DisplayInfo], views: &mut Vec<RenderView>, cameras: &mut Vec<Entity>, game_screen: bool) {
    let Some(q) = resources.get_mut::<GraphicsQueue>() else { return };
    q.meshes_used = 0;
    let size_of = |v: &RenderView, gfx: &dyn GraphicsBackend| -> (f32, f32) {
        let (w, h) = match v.target {
            Some(t) => gfx.texture_size(t).map(|(w, h)| (w as f32, h as f32)).unwrap_or((256.0, 256.0)),
            None => displays.iter().find(|d| d.display == v.display).map(|d| (d.width as f32, d.height as f32)).unwrap_or((400.0, 240.0)),
        };
        ((w * v.viewport.width).max(1.0), (h * v.viewport.height).max(1.0))
    };

    // 1. meshes queued with Graphics.DrawMesh(Instanced)
    if !q.draws.is_empty() {
        for (v, cam) in views.iter_mut().zip(cameras.iter()) {
            let mask = world.get::<&Camera>(*cam).map(|c| c.culling_mask).unwrap_or(u32::MAX);
            for d in &q.draws {
                if d.camera.is_some_and(|c| c != *cam) || mask & (1 << d.layer) == 0 {
                    continue;
                }
                v.items.push(DrawItem { color: d.color, ..DrawItem::new(d.mesh, d.material, d.model) });
            }
        }
    }

    // 2. world lines / GL world geometry, per view (lines face that view's camera)
    let world_prims: Vec<usize> = q.prims.iter().enumerate().filter(|(_, p)| p.space == GlSpace::World).map(|(i, _)| i).collect();
    if !q.debug_lines.is_empty() || !world_prims.is_empty() {
        for vi in 0..views.len() {
            let (w, h) = size_of(&views[vi], gfx);
            let v = &views[vi];
            let cam_world = v.view.inverse();
            let eye = cam_world.w_axis.truncate();
            let forward = cam_world.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Z);
            let (ortho, px): (bool, Box<dyn Fn(Vec3) -> f32>) = match v.projection {
                Projection::Orthographic { height, .. } => (true, Box::new(move |_| height / h)),
                Projection::Perspective { fov_y_radians, .. } => (false, Box::new(move |p: Vec3| 2.0 * (p - eye).dot(forward).max(0.05) * (fov_y_radians * 0.5).tan() / h)),
            };
            let _ = w;
            for depth_test in [true, false] {
                let mut mesh = MeshData::default();
                let segs: Vec<(Vec3, Color)> = q.debug_lines.iter().filter(|l| l.depth_test == depth_test).flat_map(|l| [(l.a, l.color), (l.b, l.color)]).collect();
                line_quads(&segs, eye, forward, ortho, &*px, &mut mesh);
                for &i in &world_prims {
                    let p = &q.prims[i];
                    if p.depth_test != depth_test {
                        continue;
                    }
                    if p.lines {
                        line_quads(&p.verts, eye, forward, ortho, &*px, &mut mesh);
                    } else {
                        triangles(&p.verts, &mut mesh);
                    }
                }
                if mesh.vertices.is_empty() || mesh.vertices.len() > u16::MAX as usize {
                    continue;
                }
                let (Some(id), Some(mat)) = (q.next_mesh(gfx, &mesh), q.line_material(gfx, depth_test)) else { continue };
                views[vi].items.push(DrawItem { sorting_order: 30000, ..DrawItem::new(id, mat, Mat4::IDENTITY) });
            }
        }
    }

    // 3. post effects: the camera draws into a texture, its materials draw it to the real destination
    let mut inserts: Vec<(usize, RenderView)> = Vec::new();
    for (i, (v, cam)) in views.iter_mut().zip(cameras.iter()).enumerate() {
        let Ok(fx) = world.get::<&PostEffect>(*cam) else { continue };
        if fx.materials.is_empty() {
            continue;
        }
        let (w, h) = size_of(v, gfx);
        let (w, h) = (w.round() as u32, h.round() as u32);
        let mut chain = Vec::new();
        for k in 0..fx.materials.len() {
            let key = (*cam, k);
            let t = match q.post_targets.get(&key) {
                Some((t, tw, th)) if (*tw, *th) == (w, h) => Some(*t),
                old => {
                    if let Some((t, _, _)) = old {
                        gfx.destroy_texture(*t);
                    }
                    let t = gfx.create_render_target(&RenderTargetDesc { width: w, height: h, depth: true }).ok();
                    if let Some(t) = t {
                        q.post_targets.insert(key, (t, w, h));
                    }
                    t
                }
            };
            chain.push(t);
        }
        let Some(Some(first)) = chain.first().copied() else { continue };
        let Some(quad) = q.quad(gfx) else { continue };
        let (display, viewport, target, depth) = (v.display, v.viewport, v.target, v.depth);
        v.target = Some(first);
        v.viewport = Viewport::FULL;
        let aspect = w as f32 / h.max(1) as f32;
        let mut src = first;
        for (k, &m) in fx.materials.iter().enumerate() {
            let last = k + 1 == fx.materials.len();
            let Some(mat) = q.blit_material(gfx, Some(m), src) else { break };
            let dst = if last { target } else { chain.get(k + 1).copied().flatten() };
            let (vp, disp) = if last { (viewport, display) } else { (Viewport::FULL, display) };
            inserts.push((i, blit_view(disp, vp, dst, aspect, depth, quad, mat)));
            if let Some(d) = dst {
                src = d;
            }
        }
    }
    // 4. onRenderImage: the camera draws into its source texture; the script's blits into
    //    `dst` (BlitDest::Camera) draw to where the camera would have drawn
    let mut camera_dest: HashMap<Entity, (usize, Display, Viewport, Option<TextureId>, i32, f32, f32)> = HashMap::new();
    for cam in q.image_effects_wanted.clone() {
        let Some(i) = cameras.iter().position(|c| *c == cam) else { continue };
        let Some(&(rt, _, _)) = q.image_effects.get(&cam) else { continue };
        let v = &mut views[i];
        let (w, h) = size_of(v, gfx);
        camera_dest.insert(cam, (i, v.display, v.viewport, v.target, v.depth, w, h));
        v.target = Some(rt);
        v.viewport = Viewport::FULL;
    }
    let blits = std::mem::take(&mut q.blits);
    let mut tail: Vec<(RenderView, Entity)> = Vec::new();
    for b in &blits {
        let Some(quad) = q.quad(gfx) else { break };
        let Some(mat) = q.blit_material_with(gfx, b.material, b.source, b.alpha) else { continue };
        match b.dest {
            BlitDest::Camera(cam) => {
                let Some(&(i, display, viewport, target, depth, w, h)) = camera_dest.get(&cam) else { continue };
                let (vp, aspect) = sub_viewport(viewport, b.rect, w, h);
                inserts.push((i, blit_view_tinted(display, vp, target, aspect, depth, quad, mat, b.color)));
            }
            BlitDest::Texture(t) => {
                let (w, h) = gfx.texture_size(t).map(|(w, h)| (w as f32, h as f32)).unwrap_or((1.0, 1.0));
                let (vp, aspect) = sub_viewport(Viewport::FULL, b.rect, w, h);
                tail.push((blit_view_tinted(Display::Main, vp, Some(t), aspect, i32::MAX - 1, quad, mat, b.color), Entity::DANGLING));
            }
            BlitDest::Screen if game_screen => {
                let (w, h) = displays.iter().find(|d| d.display == Display::Main).map(|d| (d.width as f32, d.height as f32)).unwrap_or((1.0, 1.0));
                let (vp, aspect) = sub_viewport(Viewport::FULL, b.rect, w, h);
                tail.push((blit_view_tinted(Display::Main, vp, None, aspect, i32::MAX - 1, quad, mat, b.color), Entity::DANGLING));
            }
            BlitDest::Screen => {}
        }
    }
    q.blits = blits;
    // after their camera, in submission order (render targets draw before the displays anyway)
    inserts.sort_by_key(|(i, _)| *i);
    for (i, view) in inserts.into_iter().rev() {
        views.insert(i + 1, view);
        cameras.insert(i + 1, Entity::DANGLING);
    }
    for (v, c) in tail {
        views.push(v);
        cameras.push(c);
    }
    // 5. GL in screen space: one overlay view per display, over everything
    let screen: Vec<usize> = q.prims.iter().enumerate().filter(|(_, p)| p.space != GlSpace::World).map(|(i, _)| i).collect();
    if !screen.is_empty() && game_screen {
        let d = displays.iter().find(|d| d.display == Display::Main).copied();
        if let Some(d) = d {
            let (dw, dh) = (d.width as f32, d.height as f32);
            let aspect = dw / dh.max(1.0);
            let to_view = |p: Vec3, space: GlSpace| match space {
                GlSpace::Pixels => Vec3::new(p.x / dw * aspect - aspect * 0.5, p.y / dh - 0.5, -1.0),
                _ => Vec3::new(p.x * aspect - aspect * 0.5, p.y - 0.5, -1.0),
            };
            let mut mesh = MeshData::default();
            for &i in &screen {
                let p = &q.prims[i];
                let verts: Vec<(Vec3, Color)> = p.verts.iter().map(|(v, c)| (to_view(*v, p.space), *c)).collect();
                if p.lines {
                    line_quads(&verts, Vec3::ZERO, Vec3::NEG_Z, true, &|_| 1.0 / dh, &mut mesh);
                } else {
                    triangles(&verts, &mut mesh);
                }
            }
            if let (Some(id), Some(mat)) = (q.next_mesh(gfx, &mesh), q.line_material(gfx, false)) {
                views.push(overlay_view(Display::Main, aspect, i32::MAX, vec![DrawItem::new(id, mat, Mat4::IDENTITY)], None));
                cameras.push(Entity::DANGLING);
            }
        }
    }
    // pooled meshes used last frame but not this one draw nothing
    for &id in &q.meshes[q.meshes_used..q.meshes_last.max(q.meshes_used)] {
        let _ = gfx.update_mesh(id, &MeshData::default());
    }
    q.meshes_last = q.meshes_used;
}
