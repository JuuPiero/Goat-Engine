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
    /// Unity's Transform tool (Y): move arrows + rotation rings (+ uniform scale in 2D).
    Transform,
}

pub struct Tools {
    pub mode: GizmoMode,
    /// The grid entity of the world the Scene view shows (core `swap_scene_helpers`).
    pub(crate) grid: Option<Entity>,
    /// its mesh + material, shared by the edit and play worlds' grids
    grid_gpu: Option<(goat_gfx::MeshId, goat_gfx::MaterialId)>,
    /// The handle being dragged / under the pointer (`handles::AXIS`…).
    pub dragging: Option<usize>,
    pub hover: Option<usize>,
    /// Size of the Scene view in pixels (handles are sized in pixels).
    pub view_px: Vec2,
    /// Handle axes: the active object's (Local) or the world's (Global).
    pub local: bool,
    /// Several objects: the gizmo at their centre (Center) or at the active one (Pivot).
    pub center: bool,
    /// Ctrl-drag increments: move (units), rotate (degrees), scale (factor).
    pub snap: [f32; 3],
    pub(crate) drag: Option<crate::handles::GizmoDrag>,
}

impl Default for Tools {
    fn default() -> Self {
        Tools { mode: GizmoMode::Move, grid: None, grid_gpu: None, dragging: None, hover: None, view_px: Vec2::new(800.0, 600.0), local: false, center: false, snap: [0.25, 15.0, 0.1], drag: None }
    }
}

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
        let eng = self.play.as_ref().unwrap_or(&self.edit);
        eng.resources.get::<SceneRegistry>().map(|r| r.components().iter().map(|c| c.name.to_string()).collect()).unwrap_or_default()
    }

    /// `[{ name, value }]` of every registered component the entity has.
    pub fn components(&mut self, e: Entity) -> Value {
        // the play world while playing (Unity: Play-mode edits, gone on Stop)
        let eng = self.play.as_mut().unwrap_or(&mut self.edit);
        let registry = eng.resources.take::<SceneRegistry>().unwrap_or_default();
        let mut out = Vec::new();
        if let Some(assets) = eng.resources.get::<AssetServer>() {
            let ctx = SaveContext { world: &eng.world, assets };
            for c in registry.components() {
                if let Some(v) = (c.save)(&ctx, e) {
                    out.push(serde_json::json!({ "name": c.name, "value": v }));
                }
            }
        }
        eng.resources.put_back(registry);
        Value::Array(out)
    }

    /// Adds or replaces a component from its scene JSON (`{}` = defaults).
    pub fn set_component(&mut self, gfx: &mut dyn GraphicsBackend, e: Entity, name: &str, value: &Value) -> Result<(), String> {
        // the play world while playing (Unity: Play-mode edits, gone on Stop)
        let eng = self.play.as_mut().unwrap_or(&mut self.edit);
        let registry = eng.resources.take::<SceneRegistry>().unwrap_or_default();
        let result = match (registry.find_component(name).copied(), eng.resources.get_mut::<AssetServer>()) {
            (Some(codec), Some(assets)) => {
                let mut lc = LoadContext { world: &mut eng.world, gfx, assets, source: "inspector" };
                (codec.load)(&mut lc, e, value)
            }
            (None, _) => Err(format!("unknown component {name}")),
            (_, None) => Err("no asset server".into()),
        };
        eng.resources.put_back(registry);
        result
    }

    pub fn remove_component(&mut self, e: Entity, name: &str) {
        // the play world while playing (Unity: Play-mode edits, gone on Stop)
        let eng = self.play.as_mut().unwrap_or(&mut self.edit);
        let codec = eng.resources.get::<SceneRegistry>().and_then(|r| r.find_component(name).copied());
        if let Some(c) = codec {
            (c.remove)(&mut eng.world, e);
        }
    }

    // ------------------------------------------------------------------ entities
    pub fn create_entity(&mut self, name: &str, parent: Option<Entity>) -> Entity {
        // the play world while playing (Unity: Play-mode edits, gone on Stop)
        let eng = self.play.as_mut().unwrap_or(&mut self.edit);
        let e = eng.world.spawn((Name(name.into()), Transform::default(), GlobalTransform::default()));
        if let Some(p) = parent.filter(|p| eng.world.contains(*p)) {
            let _ = eng.world.insert_one(e, Parent(p));
        }
        e
    }

    /// Deletes an entity and its children.
    pub fn delete_entity(&mut self, e: Entity) {
        // the play world while playing (Unity: Play-mode edits, gone on Stop)
        let eng = self.play.as_mut().unwrap_or(&mut self.edit);
        let pairs: Vec<(Entity, Entity)> = eng.world.query::<&Parent>().iter().map(|(c, p)| (p.0, c)).collect();
        let mut doomed = vec![e];
        let mut i = 0;
        while i < doomed.len() {
            let x = doomed[i];
            doomed.extend(pairs.iter().filter(|(p, _)| *p == x).map(|(_, c)| *c));
            i += 1;
        }
        for d in doomed {
            let _ = eng.world.despawn(d);
        }
        self.selection.retain(|s| eng.world.contains(*s));
        if self.selected.is_some_and(|s| !eng.world.contains(s)) {
            self.selected = self.selection.last().copied();
        }
    }

    /// Re-parents (None = root), keeping the world pose.
    pub fn set_parent(&mut self, e: Entity, parent: Option<Entity>) {
        // the play world while playing (Unity: Play-mode edits, gone on Stop)
        let eng = self.play.as_mut().unwrap_or(&mut self.edit);
        if parent == Some(e) {
            return;
        }
        let world_m = eng.world.get::<&GlobalTransform>(e).map(|g| g.0).unwrap_or(Mat4::IDENTITY);
        let parent_m = parent.and_then(|p| eng.world.get::<&GlobalTransform>(p).ok().map(|g| g.0)).unwrap_or(Mat4::IDENTITY);
        let (s, r, t) = (parent_m.inverse() * world_m).to_scale_rotation_translation();
        if let Ok(mut tr) = eng.world.get::<&mut Transform>(e) {
            *tr = Transform { position: t, rotation: r, scale: s };
        }
        match parent {
            Some(p) => {
                let _ = eng.world.insert_one(e, Parent(p));
            }
            None => {
                let _ = eng.world.remove_one::<Parent>(e);
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
        self.projection_matrix() * self.camera_matrix().inverse()
    }
    /// The editor camera's projection (perspective 55°, or orthographic in 2D / Iso views).
    pub fn projection_matrix(&self) -> Mat4 {
        if self.view.ortho {
            let h = self.ortho_height() * 0.5;
            let w = h * self.aspect;
            Mat4::orthographic_rh_gl(-w, w, -h, h, 0.05, 2000.0)
        } else {
            Mat4::perspective_rh_gl(55f32.to_radians(), self.aspect, 0.05, 500.0)
        }
    }
    /// Height of the orthographic view: what the perspective view shows at the target distance.
    pub fn ortho_height(&self) -> f32 {
        2.0 * self.orbit.distance * (55f32.to_radians() * 0.5).tan()
    }
    /// World point → view coordinates (0..1, y down); None behind the camera.
    pub fn project(&self, p: Vec3) -> Option<Vec2> {
        let c = self.view_proj() * p.extend(1.0);
        if c.w <= 1e-4 {
            return None;
        }
        Some(Vec2::new(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5))
    }
    /// The ray through a view point (0..1 from the top-left): from the eye (perspective) or
    /// from the view plane (orthographic).
    pub fn ray(&self, x: f32, y: f32) -> (Vec3, Vec3) {
        let ndc = (x * 2.0 - 1.0, 1.0 - y * 2.0);
        let r = self.orbit.rotation();
        if self.view.ortho {
            let h = self.ortho_height() * 0.5;
            let origin = self.orbit.position() + r * Vec3::new(ndc.0 * h * self.aspect, ndc.1 * h, 0.0);
            return (origin, r * Vec3::NEG_Z);
        }
        let fov = 55f32.to_radians();
        let dir = (r * Vec3::new(ndc.0 * (fov * 0.5).tan() * self.aspect, ndc.1 * (fov * 0.5).tan(), -1.0)).normalize();
        (self.orbit.position(), dir)
    }

    /// The grid of the world the Scene view shows (created on first use). Handles: handles.rs (UI overlay).
    pub(crate) fn update_tools(&mut self, gfx: &mut dyn GraphicsBackend) {
        let unlit = |c: Color, depth_test: bool, blend: BlendMode| MaterialDesc { base_color: c, lighting: LightingModel::Unlit, blend, cull: CullMode::None, depth_write: false, depth_test, fog: false, ..Default::default() };
        if self.tools.grid.is_none() {
            if self.tools.grid_gpu.is_none() {
                if let (Ok(mesh), Ok(mat)) = (gfx.create_mesh(&grid_mesh(30)), gfx.create_material(&unlit(Color::rgba(0.8, 0.85, 0.95, 1.0), true, BlendMode::Alpha))) {
                    self.tools.grid_gpu = Some((mesh, mat));
                }
            }
            if let Some((mesh, mat)) = self.tools.grid_gpu {
                let g = crate::core::view_world!(mut self).spawn((Name("Grid".into()), DontSave, Transform::default(), GlobalTransform::default(), MeshRenderer::new(mesh, mat).with_layer(EDITOR_LAYER).with_sorting_order(-1000)));
                self.tools.grid = Some(g);
            }
        }
        if let Some(g) = self.tools.grid {
            let (show, mode2d) = (self.view.grid, self.view.mode2d);
            let world = crate::core::view_world!(mut self);
            if let Ok(mut r) = world.get::<&mut MeshRenderer>(g) {
                r.visible = show;
            }
            // the 2D view looks along −Z: the grid stands up in the XY plane
            if let Ok(mut t) = world.get::<&mut Transform>(g) {
                t.rotation = if mode2d { Quat::from_rotation_x(std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY };
            }
        }
    }
}
