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
use goat_gfx::{GraphicsBackend, Projection};
use goat_runtime::*;
use goat_scene::SceneRegistry;
use serde::{Deserialize, Serialize};

/// The game whose code runs in the editor (behaviours, plugins).
pub use hello_game as game;

/// Layer of editor-only drawables (selection box); scene cameras don't draw it.
pub const EDITOR_LAYER: u8 = 31;

/// The world the Scene view shows and edits: the play world while playing (Unity's Play-mode
/// Scene view: grid, gizmos, picking and handles on the running objects). A macro so the borrow
/// covers `play` / `edit` only, not the whole core.
macro_rules! view_world {
    ($s:expr) => {
        match $s.play.as_ref() {
            Some(p) => &p.world,
            None => &$s.edit.world,
        }
    };
    (mut $s:expr) => {
        match $s.play.as_mut() {
            Some(p) => &mut p.world,
            None => &mut $s.edit.world,
        }
    };
}
pub(crate) use view_world;

/// The Game view draws without the editor's drawables (grid, gizmos): hidden for its pass.
fn hide_editor_drawables(world: &mut hecs::World) -> Vec<Entity> {
    let mut hidden = Vec::new();
    for (e, r) in world.query_mut::<&mut MeshRenderer>() {
        if r.layer == EDITOR_LAYER && r.visible {
            r.visible = false;
            hidden.push(e);
        }
    }
    hidden
}

fn show_again(world: &mut hecs::World, hidden: Vec<Entity>) {
    for e in hidden {
        if let Ok(mut r) = world.get::<&mut MeshRenderer>(e) {
            r.visible = true;
        }
    }
}

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

/// The Hierarchy icon of an entity (Unity shows the most important component's icon).
fn icon_of(w: &hecs::World, e: Entity) -> &'static str {
    if w.get::<&Camera>(e).is_ok() {
        "camera"
    } else if w.get::<&DirectionalLight>(e).is_ok() || w.get::<&PointLight>(e).is_ok() || w.get::<&SpotLight>(e).is_ok() {
        "light"
    } else if w.get::<&goat_particles::ParticleSystem>(e).is_ok() {
        "particles"
    } else if w.get::<&goat_audio::AudioSource>(e).is_ok() {
        "audio"
    } else if w.get::<&MeshRenderer>(e).is_ok() {
        "mesh"
    } else {
        "empty"
    }
}

/// Scene view options (Unity's Scene view bar: 2D, Persp / Iso, Grid, Gizmos).
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct ViewOptions {
    pub mode2d: bool,
    pub ortho: bool,
    pub grid: bool,
    pub gizmos: bool,
    /// The canvas shows the Scene view (editor camera; the play world through it while playing);
    /// false = the Game view (the scene's / game's own cameras, editor drawables hidden).
    pub scene_view: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        ViewOptions { mode2d: false, ortho: false, grid: true, gizmos: true, scene_view: true }
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
    /// activeInHierarchy (inactive rows are dimmed) / activeSelf.
    pub active: bool,
    pub active_self: bool,
    pub tag: String,
    /// Hierarchy icon: camera / light / mesh / particles / audio / empty
    pub icon: &'static str,
    /// Play mode: the handle of the loaded scene the object belongs to (0 in Edit mode).
    pub scene: u32,
}

/// A scene loaded in the running game (the Hierarchy's scene headers while playing).
#[derive(Serialize)]
pub struct PlayScene {
    pub handle: u32,
    pub name: String,
    pub key: String,
    pub active: bool,
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
    pub(crate) undo: Vec<crate::editing::UndoStep>,
    pub(crate) redo: Vec<crate::editing::UndoStep>,
    pub(crate) nav_preview: crate::navpreview::NavPreview,
    pub(crate) gizmos: crate::gizmos::Gizmos,
    /// Script fields / kinds for the Inspector (scripting.rs).
    pub(crate) scripts: crate::scripting::ScriptInfo,
    /// The Animation window's preview (animation.rs).
    pub(crate) anim: crate::animation::AnimPreview,
    /// Play mode paused (the game view keeps drawing) / one frame requested.
    pub paused: bool,
    /// Script debugging while playing (console, Error Pause, Debug.break, hot reload).
    pub debug: crate::debugging::DebugState,
    /// Paused, but a console expression needs a (zero-time) frame.
    pub step_zero: bool,
    pub step_once: bool,
    pub tools: crate::tools::Tools,
    /// Scene view options (2D, orthographic, grid, gizmos, Scene view while playing).
    pub view: ViewOptions,
    /// the Scene view's camera inside the play world (Scene view shown while playing)
    play_camera: Option<Entity>,
    /// the other world's grid / gizmo entities (play: the edit world's, swapped back on Stop)
    stashed_helpers: (Option<Entity>, Option<Entity>),
    /// the 3D orbit kept while the view is in 2D mode
    orbit3d: Option<Orbit>,
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
            undo: Vec::new(),
            redo: Vec::new(),
            nav_preview: Default::default(),
            gizmos: Default::default(),
            scripts: Default::default(),
            anim: Default::default(),
            paused: false,
            debug: Default::default(),
            step_zero: false,
            step_once: false,
            tools: Default::default(),
            view: ViewOptions::default(),
            play_camera: None,
            stashed_helpers: (None, None),
            orbit3d: None,
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

    /// The world the Hierarchy / Inspector show: the play world while playing.
    pub fn active_mut(&mut self) -> &mut Engine {
        self.play.as_mut().unwrap_or(&mut self.edit)
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
        // the Animation window's preview isn't part of the scene: saved with the values it hides
        let saved = std::mem::take(&mut self.anim);
        let hidden = saved.transforms_snapshot();
        let current: Vec<(Entity, Transform)> = hidden.iter().filter_map(|(e, _)| self.edit.world.get::<&Transform>(*e).ok().map(|t| (*e, *t))).collect();
        for (e, t) in &hidden {
            if let Ok(mut cur) = self.edit.world.get::<&mut Transform>(*e) {
                *cur = *t;
            }
        }
        let r = self.save_scene_inner();
        for (e, t) in current {
            if let Ok(mut cur) = self.edit.world.get::<&mut Transform>(e) {
                *cur = t;
            }
        }
        self.anim = saved;
        r
    }

    fn save_scene_inner(&mut self) -> Result<String, String> {
        let registry = self.edit.resources.take::<SceneRegistry>().unwrap_or_default();
        let out = self.edit.resources.get::<AssetServer>().map(|a| serde_json::to_string_pretty(&goat_scene::save_world(&self.edit.world, a, &registry)).unwrap_or_default());
        self.edit.resources.put_back(registry);
        out.ok_or_else(|| "no asset server".into())
    }

    /// The running game's loaded scenes (+ DontDestroyOnLoad when it holds objects); empty in
    /// Edit mode.
    pub fn play_scenes(&self) -> Vec<PlayScene> {
        let Some(play) = self.play.as_ref() else { return Vec::new() };
        let Some(s) = play.resources.get::<goat_scene::LoadedScenes>() else { return Vec::new() };
        let mut out: Vec<PlayScene> = s.list.iter().map(|x| PlayScene { handle: x.handle, name: x.name(), key: x.key.clone(), active: x.handle == s.active }).collect();
        if play.world.query::<&goat_scene::SceneMember>().iter().any(|(_, m)| m.0 == goat_scene::DONT_DESTROY_ON_LOAD) {
            out.push(PlayScene { handle: goat_scene::DONT_DESTROY_ON_LOAD, name: "DontDestroyOnLoad".into(), key: String::new(), active: false });
        }
        out
    }

    pub fn hierarchy(&self) -> Vec<HierarchyRow> {
        let eng = self.play.as_ref().unwrap_or(&self.edit);
        let w = &eng.world;
        let loaded = self.play.as_ref().and_then(|p| p.resources.get::<goat_scene::LoadedScenes>());
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
                (e, HierarchyRow { id: id_of(e), name: n.map(|n| n.0.clone()).unwrap_or_else(|| "Entity".into()), parent: p.map(|p| id_of(p.0)), kind: kind.into(), active: w.get::<&InactiveInHierarchy>(e).is_err(), active_self: w.get::<&Inactive>(e).is_err(), tag: w.get::<&Tag>(e).map(|t| t.0.clone()).unwrap_or_else(|_| "Untagged".into()), icon: icon_of(w, e), scene: loaded.map(|s| goat_scene::scenes::scene_of(w, s, e)).unwrap_or(0) })
            })
            .collect();
        rows.sort_by_key(|(e, _)| e.id());
        rows.into_iter().map(|(_, r)| r).collect()
    }

    fn is_generated(&self, e: Entity) -> bool {
        let eng = self.play.as_ref().unwrap_or(&self.edit);
        let w = &eng.world;
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
        let eng = self.play.as_ref().unwrap_or(&self.edit);
        let t = *eng.world.get::<&Transform>(e).ok()?;
        let (y, x, z) = t.rotation.to_euler(EulerRot::YXZ);
        Some(TransformEdit { position: t.position.into(), euler: [x.to_degrees(), y.to_degrees(), z.to_degrees()], scale: t.scale.into() })
    }

    pub fn set_transform(&mut self, e: Entity, t: &TransformEdit) {
        let eng = self.play.as_mut().unwrap_or(&mut self.edit);
        if let Ok(mut tr) = eng.world.get::<&mut Transform>(e) {
            tr.position = Vec3::from(t.position);
            tr.rotation = Quat::from_euler(EulerRot::YXZ, t.euler[1].to_radians(), t.euler[0].to_radians(), t.euler[2].to_radians());
            tr.scale = Vec3::from(t.scale);
        }
    }

    pub fn rename(&mut self, e: Entity, name: &str) {
        let eng = self.play.as_mut().unwrap_or(&mut self.edit);
        let _ = eng.world.insert_one(e, Name(name.to_string()));
    }

    // ------------------------------------------------------------------ editor camera
    pub fn orbit_by(&mut self, dx: f32, dy: f32) {
        if self.view.mode2d {
            // 2D views don't rotate (Unity: right drag pans there)
            return self.pan_by(dx, dy);
        }
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
    /// Unity's 2D button: an orthographic view down −Z (X right, Y up) that only pans / zooms;
    /// leaving it restores the 3D view.
    pub fn set_2d(&mut self, on: bool) {
        if on == self.view.mode2d {
            return;
        }
        if on {
            self.orbit3d = Some(self.orbit);
            self.orbit.yaw = 0.0;
            self.orbit.pitch = 0.0;
            self.view.ortho = true;
        } else {
            if let Some(o) = self.orbit3d.take() {
                self.orbit.yaw = o.yaw;
                self.orbit.pitch = o.pitch;
            }
            self.view.ortho = false;
        }
        self.view.mode2d = on;
    }
    /// Persp / Iso (the label under the view gizmo).
    pub fn set_ortho(&mut self, on: bool) {
        if !self.view.mode2d {
            self.view.ortho = on;
        }
    }
    /// Clicking an axis of the view gizmo: look along it (`"x"` = from +X towards −X, `"-y"` = from
    /// below...). Unity switches to orthographic for these views.
    pub fn view_axis(&mut self, axis: &str) {
        if self.view.mode2d {
            return;
        }
        use std::f32::consts::FRAC_PI_2;
        let (yaw, pitch) = match axis {
            "x" => (FRAC_PI_2, 0.0),
            "-x" => (-FRAC_PI_2, 0.0),
            "y" => (0.0, -FRAC_PI_2 + 1e-3),
            "-y" => (0.0, FRAC_PI_2 - 1e-3),
            "-z" => (std::f32::consts::PI, 0.0),
            _ => (0.0, 0.0),
        };
        self.orbit.yaw = yaw;
        self.orbit.pitch = pitch;
        self.view.ortho = true;
    }
    /// The editor camera's rotation and view options (the UI draws the view gizmo from it).
    pub fn view_state(&self) -> serde_json::Value {
        let q = self.orbit.rotation();
        serde_json::json!({ "rotation": [q.x, q.y, q.z, q.w], "distance": self.orbit.distance, "view": self.view })
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
        let w = view_world!(self);
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
        // camera / light icons first (objects without renderers)
        if let Some(e) = self.icon_hit(x, y) {
            return Some(e);
        }
        let w = view_world!(self);
        let (origin, dir) = self.ray(x, y);

        let mut best: Option<(f32, Entity)> = None;
        for (e, (mr, g)) in w.query::<(&MeshRenderer, &GlobalTransform)>().without::<&DontSave>().without::<&InactiveInHierarchy>().iter() {
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
        // Play runs the scene as it is being edited (unsaved changes included, like Unity): the
        // edit world is written over the scene key in memory (the file on disk is untouched)
        if let Some(scene) = self.scene.clone() {
            match self.save_scene() {
                Ok(text) => self.io.put(&scene, text.into_bytes()),
                Err(e) => goat_core::warn!("play: the edited scene couldn't be serialized ({e}); playing the saved file"),
            }
        }
        let mut engine = Engine::new(PlayGame { scene: self.scene.clone(), inner: game::HelloGame });
        engine.resources.insert(AssetServer::new(self.io.clone()));
        engine.resources.insert(goat_script::core_api::EditorMarker);
        engine.resources.get_or_default::<Profiler>().keep_history = 300;
        self.play = Some(engine);
        self.swap_scene_helpers();
        // ids belong to one world: the Hierarchy now lists the play world
        self.selection.clear();
        self.selected = None;
        self.step_once = false;
        self.held = [false; 14];
        self.axes = [0.0; 4];
    }
    pub fn stop(&mut self) {
        if self.play.take().is_some() {
            self.swap_scene_helpers();
            self.stashed_helpers = (None, None);
        }
        self.tools.drag = None;
        self.tools.dragging = None;
        self.play_camera = None;
        self.selection.clear();
        self.selected = None;
        self.paused = false;
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
    /// One frame on one canvas: the Scene view or the Game view, whichever `view.scene_view` says.
    pub fn frame(&mut self, gfx: &mut dyn GraphicsBackend, dt: f32) {
        let pass = if self.view.scene_view { ViewPass::Scene } else { ViewPass::Game };
        self.frame_pass(gfx, dt, pass, true);
    }

    /// One drawing of a view (Unity's Scene and Game views are independent: the editor draws
    /// both every frame on two canvases). `simulate`: this pass runs the frame (the edit world's
    /// tick, or the game while playing); the other pass only draws.
    pub fn frame_pass(&mut self, gfx: &mut dyn GraphicsBackend, dt: f32, pass: ViewPass, simulate: bool) {
        self.view.scene_view = pass == ViewPass::Scene;
        if self.play.is_some() {
            self.sync_play_camera();
        }
        if self.play.is_some() {
            // the running scripts draw their onDrawGizmos while the Scene view shows gizmos
            let (gizmos, selected) = (self.view.gizmos, self.selection.clone());
            if let Some(play) = self.play.as_mut() {
                if gizmos {
                    play.resources.insert(goat_runtime::DrawGizmos { selected });
                } else if play.resources.remove::<goat_runtime::DrawGizmos>().is_some() {
                    play.resources.get_or_default::<goat_runtime::GizmoQueue>().clear();
                }
            }
            // the Scene view keeps its grid / gizmos / outlines on the running objects
            if pass == ViewPass::Scene {
                // the game's frame first (no drawing), then the helpers from its new state, then
                // the drawing: gizmos / outlines never trail the objects
                if simulate {
                    self.play_pass(gfx, dt, true, false);
                }
                self.update_scene_helpers(gfx);
                if let Some(play) = self.play.as_mut() {
                    play.render_only(gfx);
                }
                return;
            }
            let play = self.play.as_mut().unwrap();
            let hidden = hide_editor_drawables(&mut play.world);
            self.play_pass(gfx, dt, simulate, true);
            if let Some(play) = self.play.as_mut() {
                show_again(&mut play.world, hidden);
            }
            return;
        }
        if let Ok(mut t) = self.edit.world.get::<&mut Transform>(self.camera) {
            t.position = self.orbit.position();
            t.rotation = self.orbit.rotation();
        }
        let projection = self.camera_projection();
        if let Ok(mut c) = self.edit.world.get::<&mut Camera>(self.camera) {
            c.projection = projection;
        }
        let game_view = !self.view.scene_view;
        if !game_view {
            self.draw_edit_gizmos(gfx);
            self.update_scene_helpers(gfx);
            self.update_nav_preview(gfx);
        }
        // Game view in edit mode: the scene's cameras draw (Unity), without the editor's drawables
        goat_runtime::set_active(&mut self.edit.world, self.camera, !game_view);
        self.edit.view_camera = if game_view { None } else { Some(self.camera) };
        let hidden = if game_view { hide_editor_drawables(&mut self.edit.world) } else { Vec::new() };
        if simulate {
            self.edit.frame(dt, gfx);
        } else {
            self.edit.render_only(gfx);
        }
        // the Scene view draws them again (both views every frame)
        show_again(&mut self.edit.world, hidden);
        if game_view {
            goat_runtime::set_active(&mut self.edit.world, self.camera, true);
        }
    }

    /// Grid, selection, component gizmos of the world the Scene view shows, from this frame's
    /// world matrices (else they trail moving objects).
    fn update_scene_helpers(&mut self, gfx: &mut dyn GraphicsBackend) {
        goat_runtime::engine::propagate_transforms(view_world!(mut self));
        self.update_selection_box(gfx);
        self.update_tools(gfx);
        self.update_gizmos(gfx);
    }

    /// The running game's frame (or only its drawing); `draw` false = run without drawing.
    fn play_pass(&mut self, gfx: &mut dyn GraphicsBackend, dt: f32, simulate: bool, draw: bool) {
        let Some(play) = self.play.as_mut() else { return };
        {
            // Pause: draw only; Step: one 1/60 s frame
            if !simulate || (self.paused && !self.step_once && !self.step_zero) {
                if draw {
                    play.render_only(gfx);
                }
                return;
            }
            let dt = if self.step_once { 1.0 / 60.0 } else if self.step_zero && self.paused { 0.0 } else { dt };
            self.step_once = false;
            self.step_zero = false;
            play.input.begin_frame();
            for (i, b) in Button::ALL.iter().enumerate() {
                play.input.set(*b, self.held[i]);
            }
            play.input.set_axis(Axis::LeftX, self.axes[0]);
            play.input.set_axis(Axis::LeftY, self.axes[1]);
            if draw {
                play.frame(dt, gfx);
            } else {
                play.step(dt, gfx);
            }
            if play.should_quit() {
                self.stop();
                return;
            }
            self.after_play_frame();
            return;
        }
    }

    /// Drops deleted entities from the selection (outlines: handles.rs `scene_overlay`).
    fn update_selection_box(&mut self, _gfx: &mut dyn GraphicsBackend) {
        let w = view_world!(self);
        self.selection.retain(|e| w.contains(*e));
        if self.selected.is_some_and(|e| !w.contains(e)) {
            self.selected = self.selection.last().copied();
        }
    }
    /// The profiler of the running game (or of the edit world): smoothed sections, the last
    /// frames' history, draw calls.
    pub fn profiler_json(&mut self) -> serde_json::Value {
        let engine = self.play.as_mut().unwrap_or(&mut self.edit);
        let p = engine.resources.get_or_default::<Profiler>();
        if p.keep_history == 0 {
            p.keep_history = 300;
        }
        serde_json::json!({
            "sections": p.top(64).into_iter().map(|(k, v)| serde_json::json!([k, v])).collect::<Vec<_>>(),
            "draw_calls": p.draw_calls,
            "triangles": p.triangles,
            "history": p.history.iter().map(|f| serde_json::json!({ "s": f.sections.iter().map(|(k, v)| serde_json::json!([k, v])).collect::<Vec<_>>(), "d": f.draw_calls, "t": f.triangles })).collect::<Vec<_>>(),
        })
    }

    fn camera_projection(&self) -> Projection {
        if self.view.ortho {
            Projection::Orthographic { height: self.ortho_height(), near: 0.05, far: 2000.0 }
        } else {
            Projection::Perspective { fov_y_radians: 55f32.to_radians(), near: 0.05, far: 500.0 }
        }
    }

    /// The Scene view while playing: the play world drawn through a copy of the editor camera
    /// (spawned in the play world only while the Scene view shows; the game's cameras otherwise).
    fn sync_play_camera(&mut self) {
        let projection = self.camera_projection();
        let (pos, rot) = (self.orbit.position(), self.orbit.rotation());
        let Some(play) = self.play.as_mut() else { return };
        if self.view.scene_view {
            let cam = match self.play_camera.filter(|e| play.world.contains(*e)) {
                Some(e) => e,
                None => {
                    let e = play.world.spawn((Name("Scene Camera".into()), DontSave, Transform::default(), GlobalTransform::default(), Camera { clear: Some(Color::hex(0x23262e)), projection, ..Default::default() }));
                    self.play_camera = Some(e);
                    e
                }
            };
            if let Ok(mut t) = play.world.get::<&mut Transform>(cam) {
                t.position = pos;
                t.rotation = rot;
            }
            if let Ok(mut c) = play.world.get::<&mut Camera>(cam) {
                c.projection = projection;
            }
            goat_runtime::set_active(&mut play.world, cam, true);
            play.view_camera = Some(cam);
        } else {
            // kept for the Scene view's next pass, switched off for the game's cameras
            if let Some(e) = self.play_camera.filter(|e| play.world.contains(*e)) {
                goat_runtime::set_active(&mut play.world, e, false);
            }
            play.view_camera = None;
        }
    }

    /// Moves the Scene view's own drawables (grid, gizmo mesh) between the edit world and the
    /// play world: each world gets its own entities, the edit world's come back on Stop.
    fn swap_scene_helpers(&mut self) {
        std::mem::swap(&mut self.tools.grid, &mut self.stashed_helpers.0);
        std::mem::swap(&mut self.gizmos.entity, &mut self.stashed_helpers.1);
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

    #[test]
    fn gizmo_moves_rotates_the_selection_with_snapping() {
        use crate::tools::GizmoMode;
        use goat_core::math::Vec2;
        let io = AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets"));
        let mut gfx = NullBackend::new();
        let mut c = EditorCore::new(io);
        c.first_frame(&mut gfx);
        c.open_scene(&mut gfx, "scenes/showcase.scene.json").unwrap();
        let a = c.create_entity("A", None);
        let b = c.create_entity("B", None);
        c.edit.world.get::<&mut Transform>(a).unwrap().position = Vec3::new(-1.0, 0.0, 0.0);
        c.edit.world.get::<&mut Transform>(b).unwrap().position = Vec3::new(1.0, 0.0, 0.0);
        c.frame(&mut gfx, 1.0 / 60.0);
        c.select_many(vec![a, b]);
        let (w, h) = (800.0, 600.0);
        let pos = |c: &EditorCore, e| c.edit.world.get::<&Transform>(e).unwrap().position;

        // Move, Ctrl held: both objects by the same snapped amount
        c.tools.view_px = Vec2::new(w, h);
        let (o, _, _) = c.gizmo_frame().unwrap();
        let start = c.project(o).unwrap() * Vec2::new(w, h);
        c.gizmo_start(crate::handles::AXIS, start);
        let d = c.project(o + Vec3::X * 0.6).unwrap() * Vec2::new(w, h) - start; // 0.6 units → snaps to 0.5
        c.gizmo_drag(d.x, d.y, w, h, true);
        // mid-drag (next frame): the handles are where the objects are now, not at the press
        c.frame(&mut gfx, 1.0 / 60.0);
        let mid = c.gizmo_frame().unwrap().0;
        assert!((mid - (o + Vec3::X * 0.5)).length() < 1e-4, "handles follow the drag: {mid} vs {o}");
        c.gizmo_stop();
        assert!((pos(&c, a).x + 0.5).abs() < 1e-4 && (pos(&c, b).x - 1.5).abs() < 1e-4, "{} {}", pos(&c, a), pos(&c, b));

        // Rotate around the centre (Center): 90° about Y swaps them onto the Z axis
        c.frame(&mut gfx, 1.0 / 60.0);
        c.tools.center = true;
        c.tools.mode = GizmoMode::Rotate;
        let centre = c.gizmo_frame().unwrap().0;
        assert!((centre.x - 0.5).abs() < 1e-4, "centre of the selection: {centre}");
        // grab the Y ring at +X, drag (in steps, like a pointer) to a bit past −Z: 90° about Y
        let (o, _, unit) = c.gizmo_frame().unwrap();
        let r = 0.5 * unit;
        let px = |c: &EditorCore, p: Vec3| c.project(p).unwrap() * Vec2::new(w, h);
        let mut at = px(&c, o + Vec3::X * r);
        c.gizmo_start(crate::handles::RING + 1, at);
        for k in 1..=10 {
            let a = (93f32 * k as f32 / 10.0).to_radians();
            let next = px(&c, o + Quat::from_rotation_y(a) * Vec3::X * r);
            c.gizmo_drag(next.x - at.x, next.y - at.y, w, h, true);
            at = next;
        }
        c.gizmo_stop();
        let (pa, pb) = (pos(&c, a), pos(&c, b));
        assert!((pa.x - 0.5).abs() < 1e-3 && (pb.x - 0.5).abs() < 1e-3 && (pa.z + pb.z).abs() < 1e-3 && (pa.z - pb.z).abs() > 1.9, "{pa} {pb}");
        let rot = c.edit.world.get::<&Transform>(a).unwrap().rotation;
        assert!((rot.angle_between(Quat::IDENTITY) - 90f32.to_radians()).abs() < 1e-3, "each object turned 90°");
    }

    #[test]
    fn play_runs_unsaved_edits() {
        let io = AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets"));
        let mut gfx = NullBackend::new();
        let mut c = EditorCore::new(io.clone());
        c.first_frame(&mut gfx);
        c.open_scene(&mut gfx, "scenes/showcase.scene.json").unwrap();
        let on_disk = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets/scenes/showcase.scene.json")).unwrap();
        c.create_entity("Not Saved Yet", None);
        c.play();
        c.frame(&mut gfx, 1.0 / 60.0);
        let w = &c.play.as_ref().unwrap().world;
        assert!(w.query::<&Name>().iter().any(|(_, n)| n.0 == "Not Saved Yet"), "the play world has the unsaved object");
        let still = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets/scenes/showcase.scene.json")).unwrap();
        assert_eq!(on_disk, still, "the file on disk is untouched");
        c.stop();
    }

    #[test]
    fn scene_view_modes_and_play_world_hierarchy() {
        let io = AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets"));
        let mut gfx = NullBackend::new();
        let mut c = EditorCore::new(io);
        c.first_frame(&mut gfx);
        c.open_scene(&mut gfx, "scenes/showcase.scene.json").unwrap();
        // 2D: orthographic, looking down −Z, orbiting pans instead
        c.set_2d(true);
        assert!(c.view.ortho && c.view.mode2d);
        let (o1, d1) = c.ray(0.2, 0.5);
        let (o2, d2) = c.ray(0.8, 0.5);
        assert!((d1 - d2).length() < 1e-5 && (d1 - Vec3::NEG_Z).length() < 1e-4, "parallel rays down −Z");
        assert!((o2 - o1).x > 0.1, "ortho rays start across the view plane");
        let before = c.orbit.yaw;
        c.orbit_by(50.0, 0.0);
        assert_eq!(c.orbit.yaw, before, "2D doesn't rotate");
        c.set_2d(false);
        assert!(!c.view.ortho);
        c.view_axis("y");
        assert!(c.orbit.pitch < -1.5 && c.view.ortho, "top view");
        c.frame(&mut gfx, 1.0 / 60.0);
        // Play: the Hierarchy lists the play world; the Scene view gets its own camera there
        c.play();
        c.view.scene_view = true;
        c.frame(&mut gfx, 1.0 / 60.0);
        c.frame(&mut gfx, 1.0 / 60.0);
        let names: Vec<String> = c.hierarchy().into_iter().map(|r| r.name).collect();
        assert!(names.iter().any(|n| n == "Main Camera"), "{names:?}");
        assert!(c.play.as_ref().unwrap().view_camera.is_some());
        c.view.scene_view = false;
        c.frame(&mut gfx, 1.0 / 60.0);
        assert!(c.play.as_ref().unwrap().view_camera.is_none());
        c.stop();
    }

    #[test]
    fn prefab_overrides_revert_and_apply() {
        let io = AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets"));
        let mut gfx = NullBackend::new();
        let mut c = EditorCore::new(io.clone());
        c.first_frame(&mut gfx);
        c.open_scene(&mut gfx, "scenes/showcase.scene.json").unwrap();
        c.frame(&mut gfx, 1.0 / 60.0);
        let find = |c: &EditorCore, n: &str| c.edit.world.query::<&Name>().iter().find(|(_, x)| x.0 == n).map(|(e, _)| e).unwrap();
        let pedestal = find(&c, "Pedestal");
        let info = c.prefab_info(pedestal);
        assert_eq!(info["key"], "prefabs/shrine.prefab.json");
        let base_overrides = info["overrides"].as_array().unwrap().len();
        // an edit inside the instance shows up as an override; Revert undoes it
        c.edit.world.get::<&mut Transform>(pedestal).unwrap().position.y += 2.0;
        let y_before = c.edit.world.get::<&Transform>(pedestal).unwrap().position.y - 2.0;
        assert!(c.prefab_info(pedestal)["overrides"].as_array().unwrap().len() > base_overrides || base_overrides > 0);
        c.revert_prefab(&mut gfx, pedestal).unwrap();
        c.frame(&mut gfx, 1.0 / 60.0);
        let pedestal = find(&c, "Pedestal");
        assert!(c.prefab_info(pedestal)["overrides"].as_array().unwrap().is_empty(), "{}", c.prefab_info(pedestal));
        let _ = y_before;
        // Apply: the change goes into the prefab file, the instance has no overrides left
        c.rename(pedestal, "Big Pedestal");
        let out = c.apply_prefab(&mut gfx, pedestal).unwrap();
        assert!(out["text"].as_str().unwrap().contains("Big Pedestal"), "the prefab now holds the edit");
        c.frame(&mut gfx, 1.0 / 60.0);
        let p = find(&c, "Big Pedestal");
        assert!(c.prefab_info(p)["overrides"].as_array().unwrap().is_empty(), "applied: {}", c.prefab_info(p));
        assert!(io.read_string("prefabs/shrine.prefab.json").unwrap().contains("Big Pedestal"));
    }
}
/// Which view a drawing pass is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewPass {
    Scene,
    Game,
}
