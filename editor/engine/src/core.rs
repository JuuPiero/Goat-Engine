//! The editor's engine, platform-free (tested on the host with NullBackend; the web build wraps
//! it with a canvas in `web.rs`).
//!
//! - **Edit**: an `Engine` in editing mode holding the open scene; an editor camera (orbit /
//!   pan / zoom) is the only view; click picking; selection box; transform edits; save.
//! - **Play**: a second `Engine` running the game (its plugins, behaviours, physics) on the open
//!   scene — or the game's own start when no scene is open. Stop drops it: the edited scene
//!   never changed, like Unity's play mode.

use goat_assets::{AssetIo, AssetServer};
use goat_core::math::{EulerRot, Mat4, Quat, Vec3};
use goat_core::Color;
use goat_gfx::{BlendMode, CullMode, GraphicsBackend, LightingModel, MaterialDesc, MeshData, Projection};
use goat_runtime::*;
use goat_scene::SceneRegistry;
use serde::{Deserialize, Serialize};

/// The game whose code runs in the editor (behaviours, plugins).
pub use hello_game as game;

/// Layer of editor-only drawables (selection box); scene cameras don't draw it.
pub const EDITOR_LAYER: u8 = 31;

struct EditGame;
impl Game for EditGame {
    fn start(&mut self, _ctx: &mut Context) {}
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        game::plugins()
    }
}

/// Play mode: the open scene with the game's code, or the game itself.
struct PlayGame {
    scene: Option<String>,
    inner: game::HelloGame,
}
impl Game for PlayGame {
    fn start(&mut self, ctx: &mut Context) {
        game::register(goat_scene::registry(ctx));
        match &self.scene {
            Some(key) => {
                if let Err(e) = goat_scene::load_scene(ctx, key, goat_scene::LoadMode::Additive) {
                    goat_core::error!("play {key}: {e}");
                }
            }
            None => self.inner.start(ctx),
        }
    }
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        game::plugins()
    }
}

/// Orbit camera of the Scene view.
#[derive(Clone, Copy, Debug)]
pub struct Orbit {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

impl Orbit {
    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }
    pub fn position(&self) -> Vec3 {
        self.target + self.rotation() * Vec3::new(0.0, 0.0, self.distance)
    }
}

/// One row of the Hierarchy.
#[derive(Serialize)]
pub struct HierarchyRow {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    /// "model" / "prefab" instance roots, "generated" for their children (not saved).
    pub kind: String,
}

#[derive(Serialize, Deserialize)]
pub struct TransformEdit {
    pub position: [f32; 3],
    /// Degrees (Unity's order: Z, X, Y).
    pub euler: [f32; 3],
    pub scale: [f32; 3],
}

pub fn id_of(e: Entity) -> String {
    e.to_bits().get().to_string()
}
pub fn entity_of(id: &str) -> Option<Entity> {
    id.parse::<u64>().ok().and_then(Entity::from_bits)
}

pub struct EditorCore {
    pub io: AssetIo,
    pub edit: Engine,
    pub play: Option<Engine>,
    pub camera: Entity,
    pub orbit: Orbit,
    pub scene: Option<String>,
    /// The active selected entity (Inspector, gizmo); `selection` holds all of them.
    pub selected: Option<Entity>,
    pub selection: Vec<Entity>,
    selection_boxes: Vec<Entity>,
    pub(crate) undo: Vec<crate::editing::UndoStep>,
    pub(crate) redo: Vec<crate::editing::UndoStep>,
    pub tools: crate::tools::Tools,
    /// Buttons held (Play mode input), by Button index.
    held: [bool; 14],
    axes: [f32; 4],
    pub aspect: f32,
}

impl EditorCore {
    /// The editor engine; call `first_frame` once the graphics device exists.
    pub fn new(io: AssetIo) -> Self {
        let mut edit = Engine::new(EditGame);
        edit.editing = true;
        // edits of objects inside prefab instances are saved as overrides
        goat_scene::set_track_overrides(true);
        edit.resources.insert(AssetServer::new(io.clone()));
        let camera = edit.world.spawn((
            Name("Editor Camera".into()),
            DontSave,
            Transform::default(),
            GlobalTransform::default(),
            Camera { clear: Some(Color::hex(0x23262e)), projection: Projection::Perspective { fov_y_radians: 55f32.to_radians(), near: 0.05, far: 500.0 }, ..Default::default() },
        ));
        edit.view_camera = Some(camera);
        EditorCore {
            io,
            edit,
            play: None,
            camera,
            orbit: Orbit { target: Vec3::new(0.0, 0.5, 0.0), yaw: 0.5, pitch: -0.45, distance: 11.0 },
            scene: None,
            selected: None,
            selection: Vec::new(),
            selection_boxes: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            tools: Default::default(),
            held: [false; 14],
            axes: [0.0; 4],
            aspect: 16.0 / 9.0,
        }
    }

    /// Builds the plugins (they register their scene codecs) and the game's behaviours.
    pub fn first_frame(&mut self, gfx: &mut dyn GraphicsBackend) {
        self.edit.frame(0.0, gfx);
        game::register(self.edit.resources.get_or_default::<SceneRegistry>());
    }

    pub fn is_playing(&self) -> bool {
        self.play.is_some()
    }

    /// Opens a scene file for editing (replaces the edit world's content).
    pub fn open_scene(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Result<(), String> {
        let text = self.io.read_string(key).map_err(|e| e.to_string())?;
        let file = goat_scene::parse(key, &text).map_err(|e| e.to_string())?;
        self.clear_scene();
        let registry = self.edit.resources.take::<SceneRegistry>().unwrap_or_default();
        let result = match self.edit.resources.get_mut::<AssetServer>() {
            Some(assets) => goat_scene::spawn_file(&mut self.edit.world, gfx, assets, &registry, &file, key, None).map(|_| ()).map_err(|e| e.to_string()),
            None => Err("no asset server".into()),
        };
        self.edit.resources.put_back(registry);
        self.scene = Some(key.to_string());
        self.selected = None;
        self.selection.clear();
        self.clear_undo();
        result
    }

    pub(crate) fn clear_scene_entities(&mut self) {
        self.clear_scene();
        self.selected = None;
        self.selection.clear();
    }

    fn clear_scene(&mut self) {
        let doomed: Vec<Entity> = self.edit.world.query::<()>().without::<&DontSave>().iter().map(|(e, _)| e).collect();
        for e in doomed {
            let _ = self.edit.world.despawn(e);
        }
    }

    /// The scene as JSON (what Save writes).
    pub fn save_scene(&mut self) -> Result<String, String> {
        let registry = self.edit.resources.take::<SceneRegistry>().unwrap_or_default();
        let out = self.edit.resources.get::<AssetServer>().map(|a| serde_json::to_string_pretty(&goat_scene::save_world(&self.edit.world, a, &registry)).unwrap_or_default());
        self.edit.resources.put_back(registry);
        out.ok_or_else(|| "no asset server".into())
    }

    pub fn hierarchy(&self) -> Vec<HierarchyRow> {
        let w = &self.edit.world;
        let mut rows: Vec<(Entity, HierarchyRow)> = w
            .query::<(Option<&Name>, Option<&Parent>)>()
            .without::<&DontSave>()
            .iter()
            .map(|(e, (n, p))| {
                let kind = if w.get::<&goat_scene::PrefabInstance>(e).is_ok() {
                    "prefab"
                } else if w.get::<&goat_assets::ModelInstance>(e).is_ok() {
                    "model"
                } else if self.is_generated(e) {
                    "generated"
                } else {
                    "entity"
                };
                (e, HierarchyRow { id: id_of(e), name: n.map(|n| n.0.clone()).unwrap_or_else(|| "Entity".into()), parent: p.map(|p| id_of(p.0)), kind: kind.into() })
            })
            .collect();
        rows.sort_by_key(|(e, _)| e.id());
        rows.into_iter().map(|(_, r)| r).collect()
    }

    fn is_generated(&self, e: Entity) -> bool {
        let w = &self.edit.world;
        let mut cur = w.get::<&Parent>(e).ok().map(|p| p.0);
        for _ in 0..64 {
            let Some(p) = cur else { return false };
            if w.get::<&goat_scene::PrefabInstance>(p).is_ok() || w.get::<&goat_assets::ModelInstance>(p).is_ok() {
                return true;
            }
            cur = w.get::<&Parent>(p).ok().map(|p| p.0);
        }
        false
    }

    pub fn select(&mut self, e: Option<Entity>) {
        self.select_many(e.into_iter().collect());
    }

    pub fn transform(&self, e: Entity) -> Option<TransformEdit> {
        let t = *self.edit.world.get::<&Transform>(e).ok()?;
        let (y, x, z) = t.rotation.to_euler(EulerRot::YXZ);
        Some(TransformEdit { position: t.position.into(), euler: [x.to_degrees(), y.to_degrees(), z.to_degrees()], scale: t.scale.into() })
    }

    pub fn set_transform(&mut self, e: Entity, t: &TransformEdit) {
        if let Ok(mut tr) = self.edit.world.get::<&mut Transform>(e) {
            tr.position = Vec3::from(t.position);
            tr.rotation = Quat::from_euler(EulerRot::YXZ, t.euler[1].to_radians(), t.euler[0].to_radians(), t.euler[2].to_radians());
            tr.scale = Vec3::from(t.scale);
        }
    }

    pub fn rename(&mut self, e: Entity, name: &str) {
        let _ = self.edit.world.insert_one(e, Name(name.to_string()));
    }

    // ------------------------------------------------------------------ editor camera
    pub fn orbit_by(&mut self, dx: f32, dy: f32) {
        self.orbit.yaw -= dx * 0.008;
        self.orbit.pitch = (self.orbit.pitch - dy * 0.008).clamp(-1.55, 1.55);
    }
    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        let r = self.orbit.rotation();
        let s = self.orbit.distance * 0.0016;
        self.orbit.target += r * Vec3::new(-dx * s, dy * s, 0.0);
    }
    pub fn zoom_by(&mut self, delta: f32) {
        self.orbit.distance = (self.orbit.distance * (1.0 + delta * 0.001)).clamp(0.3, 400.0);
    }
    /// Centers the camera on the selection (F key).
    pub fn frame_selected(&mut self, gfx: &dyn GraphicsBackend) {
        if let Some((c, r)) = self.selected.and_then(|e| self.world_sphere(gfx, e)) {
            self.orbit.target = c;
            self.orbit.distance = (r * 3.0).max(1.5);
        }
    }

    /// World bounding sphere of an entity and its renderer children.
    fn world_sphere(&self, gfx: &dyn GraphicsBackend, e: Entity) -> Option<(Vec3, f32)> {
        let w = &self.edit.world;
        let mut spheres = Vec::new();
        let mut stack = vec![e];
        let children: Vec<(Entity, Entity)> = w.query::<&Parent>().iter().map(|(c, p)| (p.0, c)).collect();
        while let Some(x) = stack.pop() {
            if let (Ok(r), Ok(g)) = (w.get::<&MeshRenderer>(x), w.get::<&GlobalTransform>(x)) {
                if let Some(b) = gfx.mesh_bounds(r.mesh) {
                    let s = g.0.to_scale_rotation_translation().0.abs().max_element();
                    spheres.push((g.0.transform_point3(b.center), b.radius * s));
                }
            }
            stack.extend(children.iter().filter(|(p, _)| *p == x).map(|(_, c)| *c));
        }
        if spheres.is_empty() {
            return w.get::<&GlobalTransform>(e).ok().map(|g| (g.0.w_axis.truncate(), 0.5));
        }
        let center = spheres.iter().map(|s| s.0).sum::<Vec3>() / spheres.len() as f32;
        let radius = spheres.iter().map(|(c, r)| (*c - center).length() + r).fold(0.0, f32::max);
        Some((center, radius))
    }

    /// The entity under a point of the Scene view (x, y in 0..1 from the top-left): the nearest
    /// renderer whose bounding sphere the ray hits, mapped to its selectable owner (the instance
    /// root for generated children).
    pub fn pick(&self, gfx: &dyn GraphicsBackend, x: f32, y: f32) -> Option<Entity> {
        let w = &self.edit.world;
        let fov = 55f32.to_radians();
        let ndc = (x * 2.0 - 1.0, 1.0 - y * 2.0);
        let r = self.orbit.rotation();
        let dir = (r * Vec3::new(ndc.0 * (fov * 0.5).tan() * self.aspect, ndc.1 * (fov * 0.5).tan(), -1.0)).normalize();
        let origin = self.orbit.position();
        let mut best: Option<(f32, Entity)> = None;
        for (e, (mr, g)) in w.query::<(&MeshRenderer, &GlobalTransform)>().without::<&DontSave>().iter() {
            let Some(b) = gfx.mesh_bounds(mr.mesh) else { continue };
            let c = g.0.transform_point3(b.center);
            let rad = b.radius * g.0.to_scale_rotation_translation().0.abs().max_element();
            let oc = c - origin;
            let t = oc.dot(dir);
            let d2 = oc.length_squared() - t * t;
            if t > 0.0 && d2 <= rad * rad && best.is_none_or(|(bt, _)| t < bt) {
                best = Some((t, e));
            }
        }
        let mut e = best?.1;
        // climb out of generated children to the instance root
        for _ in 0..64 {
            if !self.is_generated(e) {
                break;
            }
            match w.get::<&Parent>(e) {
                Ok(p) => e = p.0,
                Err(_) => break,
            }
        }
        Some(e)
    }

    // ------------------------------------------------------------------ play mode
    pub fn play(&mut self) {
        let mut engine = Engine::new(PlayGame { scene: self.scene.clone(), inner: game::HelloGame });
        engine.resources.insert(AssetServer::new(self.io.clone()));
        self.play = Some(engine);
        self.held = [false; 14];
        self.axes = [0.0; 4];
    }
    pub fn stop(&mut self) {
        self.play = None;
    }

    /// Play-mode input (keyboard mapped like the desktop player).
    pub fn key(&mut self, code: &str, down: bool) {
        let b = match code {
            "KeyK" | "Space" => Some(Button::A),
            "KeyJ" => Some(Button::B),
            "KeyI" => Some(Button::X),
            "KeyU" => Some(Button::Y),
            "KeyQ" => Some(Button::L),
            "KeyE" => Some(Button::R),
            "Digit1" => Some(Button::ZL),
            "Digit3" => Some(Button::ZR),
            "Enter" => Some(Button::Start),
            "Backspace" => Some(Button::Select),
            "ArrowUp" => Some(Button::Up),
            "ArrowDown" => Some(Button::Down),
            "ArrowLeft" => Some(Button::Left),
            "ArrowRight" => Some(Button::Right),
            _ => None,
        };
        if let Some(b) = b {
            self.held[b as usize] = down;
        }
        let v = if down { 1.0 } else { 0.0 };
        match code {
            "KeyA" => self.axes[0] = -v,
            "KeyD" => self.axes[0] = v,
            "KeyW" => self.axes[1] = v,
            "KeyS" => self.axes[1] = -v,
            _ => {}
        }
    }

    // ------------------------------------------------------------------ frame
    pub fn frame(&mut self, gfx: &mut dyn GraphicsBackend, dt: f32) {
        if let Some(play) = self.play.as_mut() {
            play.input.begin_frame();
            for (i, b) in Button::ALL.iter().enumerate() {
                play.input.set(*b, self.held[i]);
            }
            play.input.set_axis(Axis::LeftX, self.axes[0]);
            play.input.set_axis(Axis::LeftY, self.axes[1]);
            play.frame(dt, gfx);
            if play.should_quit() {
                self.play = None;
            }
            return;
        }
        if let Ok(mut t) = self.edit.world.get::<&mut Transform>(self.camera) {
            t.position = self.orbit.position();
            t.rotation = self.orbit.rotation();
        }
        self.update_selection_box(gfx);
        self.update_tools(gfx);
        self.edit.frame(dt, gfx);
    }

    /// Translucent boxes around the selected entities (editor layer); the active one brighter.
    fn update_selection_box(&mut self, gfx: &mut dyn GraphicsBackend) {
        self.selection.retain(|e| self.edit.world.contains(*e));
        if self.selected.is_some_and(|e| !self.edit.world.contains(e)) {
            self.selected = self.selection.last().copied();
        }
        let spheres: Vec<(Vec3, f32, bool)> = self.selection.iter().filter_map(|&e| self.world_sphere(gfx, e).map(|(c, r)| (c, r, Some(e) == self.selected))).collect();
        while self.selection_boxes.len() < spheres.len() {
            let (Ok(mesh), Ok(mat)) = (
                gfx.create_mesh(&MeshData::cube(1.0)),
                gfx.create_material(&MaterialDesc { base_color: Color::rgba(1.0, 0.65, 0.15, 0.18), lighting: LightingModel::Unlit, blend: BlendMode::Additive, cull: CullMode::None, depth_write: false, fog: false, ..Default::default() }),
            ) else {
                return;
            };
            let e = self.edit.world.spawn((Name("Selection".into()), DontSave, Transform::default(), GlobalTransform::default(), MeshRenderer::new(mesh, mat).with_layer(EDITOR_LAYER).with_sorting_order(1000)));
            self.selection_boxes.push(e);
        }
        for (i, &b) in self.selection_boxes.iter().enumerate() {
            let s = spheres.get(i).copied();
            if let Ok(mut r) = self.edit.world.get::<&mut MeshRenderer>(b) {
                r.visible = s.is_some();
            }
            if let (Some((c, r, active)), Ok(mut t)) = (s, self.edit.world.get::<&mut Transform>(b)) {
                // inactive members of a multi-selection: a slightly smaller box
                *t = Transform::from_position(c).with_scale(Vec3::splat(r * if active { 2.0 } else { 1.9 }));
            }
        }
    }

    /// World matrix of the editor camera (gizmos in the UI).
    pub fn camera_matrix(&self) -> Mat4 {
        Mat4::from_rotation_translation(self.orbit.rotation(), self.orbit.position())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goat_gfx::NullBackend;

    fn core() -> (EditorCore, NullBackend) {
        let io = AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets"));
        let mut gfx = NullBackend::new();
        let mut c = EditorCore::new(io);
        c.first_frame(&mut gfx);
        (c, gfx)
    }

    #[test]
    fn edit_pick_move_save_play_stop() {
        let (mut c, mut gfx) = core();
        c.open_scene(&mut gfx, "scenes/showcase.scene.json").unwrap();
        c.frame(&mut gfx, 1.0 / 60.0);
        let rows = c.hierarchy();
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        for n in ["Sun", "Main Camera", "Ground", "Shrine", "Tentacle", "Spinning Crate"] {
            assert!(names.contains(&n), "{n} in {names:?}");
        }
        assert!(rows.iter().any(|r| r.kind == "generated"), "prefab / model children listed as generated");
        // only the editor camera renders in edit mode
        assert_eq!(gfx.last_views.len(), 1);

        // pick the crate by aiming the camera at it
        let crate_e = rows.iter().find(|r| r.name == "Spinning Crate").and_then(|r| entity_of(&r.id)).unwrap();
        c.selected = Some(crate_e);
        c.frame_selected(&gfx);
        c.frame(&mut gfx, 0.016);
        assert_eq!(c.pick(&gfx, 0.5, 0.5), Some(crate_e));

        // edit its transform, save: the file text carries the change; behaviours didn't run
        let mut t = c.transform(crate_e).unwrap();
        t.position = [4.0, 1.0, 0.0];
        c.set_transform(crate_e, &t);
        let saved = c.save_scene().unwrap();
        assert!(saved.contains("\"Spinner\""), "behaviours saved");
        let v: serde_json::Value = serde_json::from_str(&saved).unwrap();
        let crate_entry = v["entities"].as_array().unwrap().iter().find(|e| e["name"] == "Spinning Crate").unwrap();
        assert_eq!(crate_entry["components"]["Transform"]["position"], serde_json::json!([4.0, 1.0, 0.0]));
        assert_eq!(v["entities"].as_array().unwrap().len(), 9, "the generated children aren't saved");

        // play: the scene's cameras render, behaviours run; stop: the edit world is untouched
        c.play();
        for _ in 0..30 {
            c.frame(&mut gfx, 1.0 / 60.0);
        }
        assert!(c.is_playing());
        assert!(!gfx.last_views.is_empty());
        let play_rot = {
            let w = &c.play.as_ref().unwrap().world;
            w.query::<(&Name, &Transform)>().iter().find(|(_, (n, _))| n.0 == "Spinning Crate").map(|(_, (_, t))| t.rotation).unwrap()
        };
        assert!(play_rot.angle_between(Quat::IDENTITY) > 0.1, "the Spinner ran in play mode");
        {
            // scripts in three languages + the Animator Controller
            let w = &c.play.as_ref().unwrap().world;
            let t = |name: &str| w.query::<(&Name, &Transform)>().iter().find(|(_, (n, _))| n.0 == name).map(|(_, (_, t))| *t).unwrap();
            let lamp = t("Lamp");
            assert!((lamp.position.y - 1.5).abs() > 0.05, "Hover.ts moved the lamp: {}", lamp.position);
            let ball = t("Ball");
            assert!((ball.scale.x - 1.2).abs() > 0.01, "Pulse.cs scaled the ball: {}", ball.scale);
            let state = w.query::<(&Name, &goat_animation::Animator)>().iter().find(|(_, (n, _))| n.0 == "Tentacle").and_then(|(_, (_, a))| a.current_state(0).map(str::to_string));
            assert_eq!(state.as_deref(), Some("Idle"), "the tentacle runs its controller");
        }
        c.stop();
        let edit_rot = c.edit.world.get::<&Transform>(crate_e).unwrap().rotation;
        assert!(edit_rot.angle_between(Quat::IDENTITY) < 1e-4, "edit world unchanged by play");
    }
}

#[cfg(test)]
mod playground_tests {
    use super::*;
    use goat_gfx::NullBackend;

    /// Physics / particles / audio / UI components all load from a scene file, save back
    /// identically, and simulate in Play.
    #[test]
    fn playground_scene_round_trips_and_simulates() {
        let io = AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets"));
        let mut gfx = NullBackend::new();
        let mut c = EditorCore::new(io);
        c.first_frame(&mut gfx);
        c.open_scene(&mut gfx, "scenes/playground.scene.json").unwrap();
        c.frame(&mut gfx, 1.0 / 60.0);
        let a = c.save_scene().unwrap();
        for name in ["\"RigidBody\"", "\"Collider\"", "\"ParticleSystem\"", "\"AudioSource\"", "\"AudioListener\"", "\"Canvas\"", "\"RectTransform\"", "\"UiText\"", "\"UiToggle\""] {
            assert!(a.contains(name), "{name} saved");
        }
        // save → open → save is stable
        let io2 = c.io.clone();
        io2.put("scenes/_resaved.scene.json", a.clone().into_bytes());
        c.open_scene(&mut gfx, "scenes/_resaved.scene.json").unwrap();
        let b = c.save_scene().unwrap();
        if let Some((i, (x, y))) = a.lines().zip(b.lines()).enumerate().find(|(_, (x, y))| x != y) {
            panic!("resave differs at line {i}: {x:?} vs {y:?}
{}", a.lines().skip(i.saturating_sub(12)).take(14).collect::<Vec<_>>().join("
"));
        }
        assert_eq!(a.lines().count(), b.lines().count());

        // play: the balls fall onto the ground and the fire emits
        c.open_scene(&mut gfx, "scenes/playground.scene.json").unwrap();
        c.play();
        for _ in 0..90 {
            c.frame(&mut gfx, 1.0 / 60.0);
        }
        let w = &c.play.as_ref().unwrap().world;
        let ball_y = w.query::<(&Name, &Transform)>().iter().find(|(_, (n, _))| n.0 == "Ball A").map(|(_, (_, t))| t.position.y).unwrap();
        assert!(ball_y < 3.0, "Ball A fell (y = {ball_y})");
        let particles = w.query::<&goat_particles::ParticleSystem>().iter().map(|(_, p)| p.particles.len()).sum::<usize>();
        assert!(particles > 10, "fire particles alive: {particles}");
    }

    #[test]
    fn physics2d_scene_settles_and_round_trips() {
        let io = AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets"));
        let mut gfx = NullBackend::new();
        let mut c = EditorCore::new(io);
        c.first_frame(&mut gfx);
        c.open_scene(&mut gfx, "scenes/physics2d.scene.json").unwrap();
        c.frame(&mut gfx, 1.0 / 60.0);
        let a = c.save_scene().unwrap();
        assert!(a.contains("\"Rigidbody2D\"") && a.contains("\"Collider2D\"") && a.contains("\"circle\""), "2D physics saved");
        c.play();
        for _ in 0..300 {
            c.frame(&mut gfx, 1.0 / 60.0);
        }
        let w = &c.play.as_ref().unwrap().world;
        let pos = |name: &str| w.query::<(&Name, &Transform)>().iter().find(|(_, (n, _))| n.0 == name).map(|(_, (_, t))| t.position).unwrap();
        let ball = pos("Ball");
        assert!(ball.y < 2.0 && ball.y > -1.3, "the ball rolled down the ramp onto the ground: {ball}");
        assert_eq!(ball.z, 0.0);
        // the crates (1.2 units) settle on the ground (top at y = -1.2) or on each other, at rest
        for name in ["Crate 1", "Crate 2", "Crate 3"] {
            let p = pos(name);
            assert!(p.y > -0.7 && p.y < 3.0, "{name} on the ground / the stack: {p}");
            let v = w.query::<(&Name, &goat_physics2d::Rigidbody2D)>().iter().find(|(_, (n, _))| n.0 == name).map(|(_, (_, r))| r.velocity.length()).unwrap();
            assert!(v < 0.2, "{name} at rest: {v}");
        }
    }
}

