//! Scene editing tools of the editor engine: a generic component API (through the scene
//! registry's codecs), entity create / delete / instantiate, the grid, and Move / Rotate / Scale
//! gizmos (screen-space picking + dragging, drawn over everything).

use goat_assets::AssetServer;
use goat_core::math::{Mat4, Quat, Vec2, Vec3};
use goat_core::Color;
use goat_gfx::{BlendMode, CullMode, GraphicsBackend, LightingModel, MaterialDesc, MeshData, Vertex};
use goat_runtime::*;
use goat_scene::{LoadContext, SaveContext, SceneRegistry};
use serde_json::Value;

use crate::core::{EditorCore, EDITOR_LAYER};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GizmoMode {
    #[default]
    Move,
    Rotate,
    Scale,
}

pub struct Tools {
    pub mode: GizmoMode,
    grid: Option<Entity>,
    handles: [Option<Entity>; 3],
    pub dragging: Option<usize>,
    /// Handle axes: the active object's (Local) or the world's (Global).
    pub local: bool,
    /// Several objects: the gizmo at their centre (Center) or at the active one (Pivot).
    pub center: bool,
    /// Ctrl-drag increments: move (units), rotate (degrees), scale (factor).
    pub snap: [f32; 3],
    pub(crate) drag: Option<GizmoDrag>,
}

impl Default for Tools {
    fn default() -> Self {
        Tools { mode: GizmoMode::Move, grid: None, handles: [None; 3], dragging: None, local: false, center: false, snap: [0.25, 15.0, 0.1], drag: None }
    }
}

pub(crate) struct DragObject {
    entity: Entity,
    start: Transform,
    world: Mat4,
    parent: Mat4,
}

/// A gizmo drag in progress: the frame it started in, pixels dragged along the axis so far and
/// the transforms it started from (each step recomputes from them, so snapping is exact).
pub(crate) struct GizmoDrag {
    axis: usize,
    origin: Vec3,
    basis: Quat,
    size: f32,
    total: f32,
    objects: Vec<DragObject>,
}

const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];

/// Grid lines on the XZ plane (thin quads), brighter every 5 units.
fn grid_mesh(half: i32) -> MeshData {
    let mut m = MeshData::default();
    let mut line = |a: Vec3, b: Vec3, w: f32, alpha: f32| {
        let d = (b - a).normalize();
        let side = Vec3::new(-d.z, 0.0, d.x) * w;
        let base = m.vertices.len() as u16;
        for p in [a - side, a + side, b + side, b - side] {
            m.vertices.push(Vertex { position: [p.x, 0.002, p.z], normal: [0.0, 1.0, 0.0], uv: [0.0, 0.0], color: [1.0, 1.0, 1.0, alpha] });
        }
        m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    let h = half as f32;
    for i in -half..=half {
        let (w, a) = if i == 0 { (0.03, 0.55) } else if i % 5 == 0 { (0.02, 0.32) } else { (0.01, 0.16) };
        let f = i as f32;
        line(Vec3::new(f, 0.0, -h), Vec3::new(f, 0.0, h), w, a);
        line(Vec3::new(-h, 0.0, f), Vec3::new(h, 0.0, f), w, a);
    }
    m
}

impl EditorCore {
    // ------------------------------------------------------------------ components
    pub fn component_names(&self) -> Vec<String> {
        self.edit.resources.get::<SceneRegistry>().map(|r| r.components().iter().map(|c| c.name.to_string()).collect()).unwrap_or_default()
    }

    /// `[{ name, value }]` of every registered component the entity has.
    pub fn components(&mut self, e: Entity) -> Value {
        let registry = self.edit.resources.take::<SceneRegistry>().unwrap_or_default();
        let mut out = Vec::new();
        if let Some(assets) = self.edit.resources.get::<AssetServer>() {
            let ctx = SaveContext { world: &self.edit.world, assets };
            for c in registry.components() {
                if let Some(v) = (c.save)(&ctx, e) {
                    out.push(serde_json::json!({ "name": c.name, "value": v }));
                }
            }
        }
        self.edit.resources.put_back(registry);
        Value::Array(out)
    }

    /// Adds or replaces a component from its scene JSON (`{}` = defaults).
    pub fn set_component(&mut self, gfx: &mut dyn GraphicsBackend, e: Entity, name: &str, value: &Value) -> Result<(), String> {
        let registry = self.edit.resources.take::<SceneRegistry>().unwrap_or_default();
        let result = match (registry.find_component(name).copied(), self.edit.resources.get_mut::<AssetServer>()) {
            (Some(codec), Some(assets)) => {
                let mut lc = LoadContext { world: &mut self.edit.world, gfx, assets, source: "inspector" };
                (codec.load)(&mut lc, e, value)
            }
            (None, _) => Err(format!("unknown component {name}")),
            (_, None) => Err("no asset server".into()),
        };
        self.edit.resources.put_back(registry);
        result
    }

    pub fn remove_component(&mut self, e: Entity, name: &str) {
        let codec = self.edit.resources.get::<SceneRegistry>().and_then(|r| r.find_component(name).copied());
        if let Some(c) = codec {
            (c.remove)(&mut self.edit.world, e);
        }
    }

    // ------------------------------------------------------------------ entities
    pub fn create_entity(&mut self, name: &str, parent: Option<Entity>) -> Entity {
        let e = self.edit.world.spawn((Name(name.into()), Transform::default(), GlobalTransform::default()));
        if let Some(p) = parent.filter(|p| self.edit.world.contains(*p)) {
            let _ = self.edit.world.insert_one(e, Parent(p));
        }
        e
    }

    /// Deletes an entity and its children.
    pub fn delete_entity(&mut self, e: Entity) {
        let pairs: Vec<(Entity, Entity)> = self.edit.world.query::<&Parent>().iter().map(|(c, p)| (p.0, c)).collect();
        let mut doomed = vec![e];
        let mut i = 0;
        while i < doomed.len() {
            let x = doomed[i];
            doomed.extend(pairs.iter().filter(|(p, _)| *p == x).map(|(_, c)| *c));
            i += 1;
        }
        for d in doomed {
            let _ = self.edit.world.despawn(d);
        }
        self.selection.retain(|s| self.edit.world.contains(*s));
        if self.selected.is_some_and(|s| !self.edit.world.contains(s)) {
            self.selected = self.selection.last().copied();
        }
    }

    /// Re-parents (None = root), keeping the world pose.
    pub fn set_parent(&mut self, e: Entity, parent: Option<Entity>) {
        if parent == Some(e) {
            return;
        }
        let world_m = self.edit.world.get::<&GlobalTransform>(e).map(|g| g.0).unwrap_or(Mat4::IDENTITY);
        let parent_m = parent.and_then(|p| self.edit.world.get::<&GlobalTransform>(p).ok().map(|g| g.0)).unwrap_or(Mat4::IDENTITY);
        let (s, r, t) = (parent_m.inverse() * world_m).to_scale_rotation_translation();
        if let Ok(mut tr) = self.edit.world.get::<&mut Transform>(e) {
            *tr = Transform { position: t, rotation: r, scale: s };
        }
        match parent {
            Some(p) => {
                let _ = self.edit.world.insert_one(e, Parent(p));
            }
            None => {
                let _ = self.edit.world.remove_one::<Parent>(e);
            }
        }
    }

    /// A model (.glb / .gltf) or prefab (.prefab.json) dropped into the scene, at `at`.
    pub fn instantiate(&mut self, gfx: &mut dyn GraphicsBackend, key: &str, at: Vec3) -> Result<Entity, String> {
        let name = key.rsplit('/').next().unwrap_or(key).split('.').next().unwrap_or(key).to_string();
        let root = self.edit.world.spawn((Name(name), Transform::from_position(at), GlobalTransform::default()));
        let lower = key.to_ascii_lowercase();
        let result = if lower.ends_with(".glb") || lower.ends_with(".gltf") {
            match self.edit.resources.get_mut::<AssetServer>() {
                Some(a) => a.spawn_model_into(&mut self.edit.world, gfx, key, root).map_err(|e| e.to_string()),
                None => Err("no asset server".into()),
            }
        } else if lower.ends_with(".prefab.json") {
            let registry = self.edit.resources.take::<SceneRegistry>().unwrap_or_default();
            let r = match self.edit.resources.get_mut::<AssetServer>() {
                Some(a) => goat_scene::spawn_prefab_instance(&mut self.edit.world, gfx, a, &registry, key, root).map_err(|e| e.to_string()),
                None => Err("no asset server".into()),
            };
            self.edit.resources.put_back(registry);
            r
        } else {
            Err(format!("{key}: only models and prefabs can be dropped into a scene"))
        };
        if let Err(e) = result {
            let _ = self.edit.world.despawn(root);
            return Err(e);
        }
        Ok(root)
    }

    /// Where a point of the view (0..1) hits the ground plane (y = 0), or a point in front.
    pub fn ground_point(&self, x: f32, y: f32) -> Vec3 {
        let (o, d) = self.ray(x, y);
        if d.y < -1e-3 {
            let t = -o.y / d.y;
            if t > 0.0 && t < 200.0 {
                return o + d * t;
            }
        }
        o + d * 5.0
    }

    // ------------------------------------------------------------------ camera math
    pub fn view_proj(&self) -> Mat4 {
        Mat4::perspective_rh_gl(55f32.to_radians(), self.aspect, 0.05, 500.0) * self.camera_matrix().inverse()
    }
    /// World point → view coordinates (0..1, y down); None behind the camera.
    pub fn project(&self, p: Vec3) -> Option<Vec2> {
        let c = self.view_proj() * p.extend(1.0);
        if c.w <= 1e-4 {
            return None;
        }
        Some(Vec2::new(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5))
    }
    pub fn ray(&self, x: f32, y: f32) -> (Vec3, Vec3) {
        let fov = 55f32.to_radians();
        let ndc = (x * 2.0 - 1.0, 1.0 - y * 2.0);
        let dir = (self.orbit.rotation() * Vec3::new(ndc.0 * (fov * 0.5).tan() * self.aspect, ndc.1 * (fov * 0.5).tan(), -1.0)).normalize();
        (self.orbit.position(), dir)
    }

    // ------------------------------------------------------------------ gizmos
    /// Where the gizmo is drawn: origin, axes (Global = world, Local = the active object's
    /// rotation; Scale is always local, like Unity) and handle length. Frozen while dragging.
    pub(crate) fn gizmo_frame(&self) -> Option<(Vec3, Quat, f32)> {
        if let Some(d) = &self.tools.drag {
            return Some((d.origin, d.basis, d.size));
        }
        let e = self.selected?;
        let g = self.edit.world.get::<&GlobalTransform>(e).ok()?.0;
        let (_, rot, pos) = g.to_scale_rotation_translation();
        let roots = self.selection_roots();
        let p = if self.tools.center && roots.len() > 1 {
            let ps: Vec<Vec3> = roots.iter().filter_map(|r| self.edit.world.get::<&GlobalTransform>(*r).ok().map(|g| g.0.w_axis.truncate())).collect();
            ps.iter().copied().sum::<Vec3>() / ps.len().max(1) as f32
        } else {
            pos
        };
        let basis = if self.tools.local || self.tools.mode == GizmoMode::Scale { rot.normalize() } else { Quat::IDENTITY };
        let dist = (p - self.orbit.position()).length().max(0.5);
        Some((p, basis, dist * 0.16))
    }

    /// The gizmo axis under a view point (pixels of a `w × h` view), if any.
    pub fn gizmo_hit(&self, x: f32, y: f32, w: f32, h: f32) -> Option<usize> {
        let (o, basis, size) = self.gizmo_frame()?;
        let px = |v: Vec2| Vec2::new(v.x * w, v.y * h);
        let p = Vec2::new(x * w, y * h);
        let a = px(self.project(o)?);
        let mut best: Option<(f32, usize)> = None;
        for (i, axis) in AXES.iter().enumerate() {
            let Some(b) = self.project(o + basis * *axis * size).map(px) else { continue };
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-4)).clamp(0.0, 1.0);
            let d = (a + ab * t - p).length();
            if d < 9.0 && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// Starts dragging `axis`: remembers the selection's transforms (every selected object
    /// without a selected ancestor moves).
    pub fn gizmo_start(&mut self, axis: usize) {
        let Some((origin, basis, size)) = self.gizmo_frame() else { return };
        let w = &self.edit.world;
        let parent_world = |e: Entity| w.get::<&Parent>(e).ok().and_then(|p| w.get::<&GlobalTransform>(p.0).ok().map(|g| g.0)).unwrap_or(Mat4::IDENTITY);
        let mut roots = self.selection_roots();
        if roots.is_empty() {
            roots.extend(self.selected);
        }
        let objects = roots
            .into_iter()
            .filter_map(|e| {
                let t = *w.get::<&Transform>(e).ok()?;
                let g = w.get::<&GlobalTransform>(e).ok()?.0;
                Some(DragObject { entity: e, start: t, world: g, parent: parent_world(e) })
            })
            .collect();
        self.tools.dragging = Some(axis);
        self.tools.drag = Some(GizmoDrag { axis: axis.min(2), origin, basis, size, total: 0.0, objects });
    }

    pub fn gizmo_stop(&mut self) {
        self.tools.dragging = None;
        self.tools.drag = None;
    }

    /// Applies a drag of (dx, dy) pixels on the dragged axis in a `w × h` view; `snap` (Ctrl
    /// held) rounds to the snap increments (Unity's Increment Snapping).
    pub fn gizmo_drag(&mut self, dx: f32, dy: f32, w: f32, h: f32, snap: bool) {
        let Some(d) = self.tools.drag.as_ref() else { return };
        let dir_world = d.basis * AXES[d.axis];
        let (Some(a), Some(b)) = (self.project(d.origin), self.project(d.origin + dir_world * d.size)) else { return };
        let screen = Vec2::new((b.x - a.x) * w, (b.y - a.y) * h);
        let len = screen.length().max(1.0);
        let along = Vec2::new(dx, dy).dot(screen / len);
        let mode = self.tools.mode;
        let steps = self.tools.snap;
        let d = self.tools.drag.as_mut().unwrap();
        d.total += along;
        let round = |v: f32, step: f32| if snap && step > 0.0 { (v / step).round() * step } else { v };
        let (origin, axis) = (d.origin, d.axis);
        for o in &d.objects {
            let (_, start_rot, start_pos) = o.world.to_scale_rotation_translation();
            let inv_parent = o.parent.inverse();
            let (_, parent_rot, _) = o.parent.to_scale_rotation_translation();
            let mut t = o.start;
            match mode {
                GizmoMode::Move => {
                    let amount = round(d.total / len * d.size, steps[0]);
                    t.position += inv_parent.transform_vector3(dir_world * amount);
                }
                GizmoMode::Rotate => {
                    let angle = round(d.total * 0.01, steps[1].to_radians());
                    let r = Quat::from_axis_angle(dir_world.normalize_or(Vec3::Y), angle);
                    t.rotation = (parent_rot.inverse() * (r * start_rot)).normalize();
                    t.position += inv_parent.transform_vector3(origin + r * (start_pos - origin) - start_pos);
                }
                GizmoMode::Scale => {
                    let f = (1.0 + round(d.total * 0.01, steps[2])).max(0.01);
                    t.scale[axis] = o.start.scale[axis] * f;
                    // several objects scale apart from the centre too
                    let off = start_pos - origin;
                    let along_axis = dir_world.normalize_or(Vec3::X);
                    t.position += inv_parent.transform_vector3(along_axis * off.dot(along_axis) * (f - 1.0));
                }
            }
            if let Ok(mut cur) = self.edit.world.get::<&mut Transform>(o.entity) {
                *cur = t;
            }
        }
    }

    /// Grid + gizmo handles of the edit world (created on first use, hidden in Play).
    pub(crate) fn update_tools(&mut self, gfx: &mut dyn GraphicsBackend) {
        let unlit = |c: Color, depth_test: bool, blend: BlendMode| MaterialDesc { base_color: c, lighting: LightingModel::Unlit, blend, cull: CullMode::None, depth_write: false, depth_test, fog: false, ..Default::default() };
        if self.tools.grid.is_none() {
            if let (Ok(mesh), Ok(mat)) = (gfx.create_mesh(&grid_mesh(30)), gfx.create_material(&unlit(Color::rgba(0.8, 0.85, 0.95, 1.0), true, BlendMode::Alpha))) {
                let g = self.edit.world.spawn((Name("Grid".into()), DontSave, Transform::default(), GlobalTransform::default(), MeshRenderer::new(mesh, mat).with_layer(EDITOR_LAYER).with_sorting_order(-1000)));
                self.tools.grid = Some(g);
            }
        }
        if self.tools.handles[0].is_none() {
            let colors = [Color::hex(0xff5a5a), Color::hex(0x6ee05a), Color::hex(0x5a9dff)];
            let Ok(cube) = gfx.create_mesh(&MeshData::cube(1.0)) else { return };
            for i in 0..3 {
                if let Ok(mat) = gfx.create_material(&unlit(colors[i], false, BlendMode::Alpha)) {
                    let h = self.edit.world.spawn((Name("Gizmo".into()), DontSave, Transform::default(), GlobalTransform::default(), MeshRenderer::new(cube, mat).with_layer(EDITOR_LAYER).with_sorting_order(2000 + i as i32)));
                    self.tools.handles[i] = Some(h);
                }
            }
        }
        let frame = self.gizmo_frame();
        for (i, h) in self.tools.handles.iter().enumerate() {
            let Some(h) = *h else { continue };
            let hot = self.tools.dragging == Some(i);
            if let Ok(mut r) = self.edit.world.get::<&mut MeshRenderer>(h) {
                r.visible = frame.is_some();
            }
            if let (Some((o, basis, size)), Ok(mut t)) = (frame, self.edit.world.get::<&mut Transform>(h)) {
                let thick = size * if hot { 0.07 } else { 0.045 };
                let head = matches!(self.tools.mode, GizmoMode::Scale);
                let along = AXES[i];
                let dims = along * size + (Vec3::ONE - along) * thick;
                *t = Transform::from_position(o + basis * along * size * 0.5).with_rotation(basis).with_scale(if head { dims + (Vec3::ONE - along) * thick } else { dims });
            }
        }
    }
}
