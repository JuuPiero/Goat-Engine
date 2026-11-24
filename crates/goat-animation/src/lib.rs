//! Animation (Unity's legacy `Animation` component, glTF clips) + CPU skinning.
//!
//! - [`Animator`] on a model instance root (an entity spawned with `spawn_model` / a scene
//!   `model` entry) plays the model's clips by name. Several states can play at once with
//!   weights (layers on different nodes, crossfades on the same nodes); [`Animator::crossfade`]
//!   ramps weights over a duration.
//! - With an Animator Controller ([`controller`], `.controller.json`), the Animator runs a state
//!   machine instead: parameters (`set_float` / `set_bool` / `set_integer` / `set_trigger`),
//!   transitions, 1D blend trees, override layers â€” Unity's Mecanim workflow.
//! - [`AnimationPlugin::update`] samples the clips (step / linear; rotations nlerp'd with sign
//!   alignment) into the node entities' Transforms, before transform propagation.
//! - [`AnimationPlugin::late_update`] deforms skinned meshes on the CPU (linear blend skinning,
//!   4 joints per vertex) into a per-instance mesh: portable — the 3DS's vertex shader has too
//!   few uniforms for real skeletons.

pub mod clip;
pub mod controller;
#[cfg(feature = "scene")]
pub mod scene;
#[cfg(feature = "script")]
pub mod script;

pub use clip::{AnimClip, AnimClips, Animation, Curve, Interp, Key};
pub use controller::{ClipEvent, ControllerData, ControllerState, LayerBlending};
use std::sync::Arc;

use std::collections::{BTreeMap, HashMap, HashSet};

use goat_assets::{AnimPath, AssetServer, Interpolation, ModelAnimation, ModelNodes, SkinnedMesh};
use goat_core::math::{Mat4, Quat, Vec3, Vec4};
use goat_gfx::{MeshData, Vertex};
use goat_runtime::{Context, Entity, GlobalTransform, MeshRenderer, Plugin, Transform, World};

#[derive(Clone, Debug, PartialEq)]
pub struct AnimState {
    pub clip: String,
    pub time: f32,
    pub speed: f32,
    pub weight: f32,
    pub looping: bool,
    /// Weight change per second (crossfades); the state is dropped when it fades to 0.
    pub fade: f32,
}

/// Plays a model's clips on its node entities â€” directly (`states`) or through an Animator
/// Controller state machine (`controller`).
#[derive(Clone, Debug)]
pub struct Animator {
    pub states: Vec<AnimState>,
    /// Pauses every state.
    pub paused: bool,
    /// The state machine driving this animator (replaces `states` when set).
    pub controller: Option<ControllerState>,
    /// Asset key of the controller (saved in scenes).
    pub controller_key: Option<String>,
    /// Playback speed multiplier (controller mode).
    pub speed: f32,
    /// Clip events without a controller (a controller brings its own `events`).
    pub events: BTreeMap<String, Vec<ClipEvent>>,
    /// Events crossed by the last update (read them in behaviours / scripts; replaced next frame).
    pub fired: Vec<FiredEvent>,
    last_times: HashMap<String, f32>,
    /// Node indices of each controller layer's mask (None = every node), built on first use.
    masks: Option<Vec<Option<HashSet<usize>>>>,
}

/// An animation event that happened (Unity calls a method named `name` on the object).
#[derive(Clone, Debug, PartialEq)]
pub struct FiredEvent {
    pub name: String,
    pub parameter: String,
    pub clip: String,
}

impl Default for Animator {
    fn default() -> Self {
        Animator { states: Vec::new(), paused: false, controller: None, controller_key: None, speed: 1.0, events: BTreeMap::new(), fired: Vec::new(), last_times: HashMap::new(), masks: None }
    }
}

/// Events of `list` crossed going from `last` to `t` (seconds; `t < last` = looped past `dur`).
fn crossed(list: &[ClipEvent], last: Option<f32>, t: f32, dur: f32, out: &mut Vec<FiredEvent>, clip: &str) {
    let last = last.unwrap_or(-1e-6);
    let mut push = |e: &ClipEvent| out.push(FiredEvent { name: e.name.clone(), parameter: e.parameter.clone(), clip: clip.into() });
    if t >= last {
        list.iter().filter(|e| e.time > last && e.time <= t).for_each(&mut push);
    } else {
        list.iter().filter(|e| (e.time > last && e.time <= dur) || e.time <= t).for_each(&mut push);
    }
}

/// Node indices named in `mask` and all their descendants.
fn mask_nodes(model: &goat_assets::ModelData, mask: &[String]) -> HashSet<usize> {
    let mut set = HashSet::new();
    let mut stack: Vec<usize> = model.nodes.iter().enumerate().filter(|(_, n)| mask.iter().any(|m| *m == n.name)).map(|(i, _)| i).collect();
    while let Some(i) = stack.pop() {
        if set.insert(i) {
            stack.extend(model.nodes[i].children.iter().copied());
        }
    }
    set
}

/// Reads a `.controller.json` asset.
pub fn load_controller(assets: &AssetServer, key: &str) -> Result<Arc<ControllerData>, String> {
    let text = assets.io.read_string(key).map_err(|e| e.to_string())?;
    ControllerData::parse(&text).map(Arc::new).map_err(|e| format!("{key}: {e}"))
}

impl Animator {
    /// An animator driven by a controller.
    pub fn with_controller(data: Arc<ControllerData>) -> Self {
        Animator { controller: Some(ControllerState::new(data)), speed: 1.0, ..Default::default() }
    }
    /// Loads `key` (a `.controller.json`) and drives the animator with it.
    pub fn from_controller_asset(assets: &AssetServer, key: &str) -> Result<Self, String> {
        let mut a = Animator::with_controller(load_controller(assets, key)?);
        a.controller_key = Some(key.into());
        Ok(a)
    }
    pub fn set_float(&mut self, name: &str, v: f32) {
        if let Some(c) = &mut self.controller {
            c.set(name, v);
        }
    }
    pub fn set_integer(&mut self, name: &str, v: i32) {
        self.set_float(name, v as f32);
    }
    pub fn set_bool(&mut self, name: &str, v: bool) {
        self.set_float(name, v as u8 as f32);
    }
    pub fn set_trigger(&mut self, name: &str) {
        self.set_float(name, 1.0);
    }
    pub fn reset_trigger(&mut self, name: &str) {
        self.set_float(name, 0.0);
    }
    pub fn get_float(&self, name: &str) -> f32 {
        self.controller.as_ref().and_then(|c| c.get(name)).unwrap_or(0.0)
    }
    pub fn get_integer(&self, name: &str) -> i32 {
        self.get_float(name) as i32
    }
    pub fn get_bool(&self, name: &str) -> bool {
        self.get_float(name) != 0.0
    }
    /// The current state of a controller layer.
    pub fn current_state(&self, layer: usize) -> Option<&str> {
        self.controller.as_ref()?.current_state(layer)
    }
    pub fn is_in_transition(&self, layer: usize) -> bool {
        self.controller.as_ref().is_some_and(|c| c.is_in_transition(layer))
    }
    /// Playing `clip` (looping) from the start.
    pub fn playing(clip: &str) -> Self {
        let mut a = Animator::default();
        a.play(clip);
        a
    }
    /// Plays `clip` alone, from the start (controller: jumps to the state named `clip`).
    pub fn play(&mut self, clip: &str) {
        if let Some(c) = &mut self.controller {
            c.play(clip, 0, 0.0);
            return;
        }
        self.states = vec![AnimState { clip: clip.into(), time: 0.0, speed: 1.0, weight: 1.0, looping: true, fade: 0.0 }];
    }
    /// Adds a clip on top (other nodes animate together; same nodes blend by weight).
    pub fn add(&mut self, clip: &str, weight: f32) {
        self.states.retain(|s| s.clip != clip);
        self.states.push(AnimState { clip: clip.into(), time: 0.0, speed: 1.0, weight, looping: true, fade: 0.0 });
    }
    /// Fades `clip` in and every other state out over `seconds`.
    pub fn crossfade(&mut self, clip: &str, seconds: f32) {
        if let Some(c) = &mut self.controller {
            c.play(clip, 0, seconds);
            return;
        }
        let rate = 1.0 / seconds.max(1e-3);
        for s in &mut self.states {
            s.fade = -rate;
        }
        match self.states.iter_mut().find(|s| s.clip == clip) {
            Some(s) => s.fade = rate,
            None => self.states.push(AnimState { clip: clip.into(), time: 0.0, speed: 1.0, weight: 0.0, looping: true, fade: rate }),
        }
    }
    pub fn stop(&mut self) {
        self.states.clear();
    }
    pub fn is_playing(&self, clip: &str) -> bool {
        self.states.iter().any(|s| s.clip == clip && s.weight > 0.0)
    }
}

/// Value of one channel at `t` (xyz(w)).
fn sample(times: &[f32], values: &[Vec4], interpolation: Interpolation, rotation: bool, t: f32) -> Vec4 {
    let i = times.partition_point(|&k| k <= t);
    if i == 0 {
        return values[0];
    }
    if i >= times.len() {
        return values[times.len() - 1];
    }
    let (a, b) = (values[i - 1], values[i]);
    if interpolation == Interpolation::Step {
        return a;
    }
    let span = (times[i] - times[i - 1]).max(1e-6);
    let f = (t - times[i - 1]) / span;
    if rotation {
        let b = if a.dot(b) < 0.0 { -b } else { b };
        a.lerp(b, f).normalize()
    } else {
        a.lerp(b, f)
    }
}

/// Weighted accumulation of the animated values of a node.
#[derive(Default, Clone, Copy)]
struct Acc {
    t: Vec4,
    tw: f32,
    r: Vec4,
    rw: f32,
    s: Vec4,
    sw: f32,
}

/// Samples the states into node transforms (`node index → entity`).
pub fn apply(world: &mut World, nodes: &[Option<Entity>], clips: &[(&ModelAnimation, f32, f32)]) {
    apply_layer(world, nodes, clips, None, false);
}

/// Like [`apply`], limited to the `mask` nodes; `additive` adds each clip's change from its
/// first frame to the current pose instead of replacing it.
pub fn apply_layer(world: &mut World, nodes: &[Option<Entity>], clips: &[(&ModelAnimation, f32, f32)], mask: Option<&HashSet<usize>>, additive: bool) {
    if additive {
        for (anim, time, weight) in clips {
            if *weight <= 0.0 {
                continue;
            }
            for ch in &anim.channels {
                if mask.is_some_and(|m| !m.contains(&ch.node)) {
                    continue;
                }
                let Some(Some(e)) = nodes.get(ch.node) else { continue };
                let Ok(mut t) = world.get::<&mut Transform>(*e) else { continue };
                let rot = ch.path == AnimPath::Rotation;
                let v = sample(&ch.times, &ch.values, ch.interpolation, rot, *time);
                let v0 = ch.values[0];
                match ch.path {
                    AnimPath::Translation => t.position += (v - v0).truncate() * *weight,
                    AnimPath::Rotation => {
                        let q = Quat::from_xyzw(v.x, v.y, v.z, v.w).normalize();
                        let q0 = Quat::from_xyzw(v0.x, v0.y, v0.z, v0.w).normalize();
                        let delta = Quat::IDENTITY.slerp(q * q0.inverse(), *weight);
                        t.rotation = (delta * t.rotation).normalize();
                    }
                    AnimPath::Scale => {
                        let r = v.truncate() / v0.truncate().max(Vec3::splat(1e-6));
                        t.scale *= Vec3::ONE.lerp(r, *weight);
                    }
                }
            }
        }
        return;
    }
    let mut acc: HashMap<usize, Acc> = HashMap::new();
    for (anim, time, weight) in clips {
        if *weight <= 0.0 {
            continue;
        }
        for ch in &anim.channels {
            if mask.is_some_and(|m| !m.contains(&ch.node)) {
                continue;
            }
            let rot = ch.path == AnimPath::Rotation;
            let v = sample(&ch.times, &ch.values, ch.interpolation, rot, *time);
            let a = acc.entry(ch.node).or_default();
            match ch.path {
                AnimPath::Translation => {
                    a.t += v * *weight;
                    a.tw += weight;
                }
                AnimPath::Rotation => {
                    // keep quaternions in one hemisphere before summing
                    let v = if a.rw > 0.0 && a.r.dot(v) < 0.0 { -v } else { v };
                    a.r += v * *weight;
                    a.rw += weight;
                }
                AnimPath::Scale => {
                    a.s += v * *weight;
                    a.sw += weight;
                }
            }
        }
    }
    for (node, a) in acc {
        let Some(Some(e)) = nodes.get(node) else { continue };
        let Ok(mut t) = world.get::<&mut Transform>(*e) else { continue };
        // partial weights blend with the current (rest) pose
        if a.tw > 0.0 {
            let v = (a.t / a.tw).truncate();
            t.position = t.position.lerp(v, a.tw.min(1.0));
        }
        if a.rw > 0.0 {
            let q = a.r / a.rw;
            let q = Quat::from_xyzw(q.x, q.y, q.z, q.w).normalize();
            t.rotation = t.rotation.slerp(q, a.rw.min(1.0));
        }
        if a.sw > 0.0 {
            let v = (a.s / a.sw).truncate();
            t.scale = t.scale.lerp(v, a.sw.min(1.0));
        }
    }
}

/// Linear blend skinning of a bind-pose mesh by joint matrices (`joint world * inverse bind`,
/// already in the mesh's space).
pub fn skin(base: &MeshData, joints: &[[u16; 4]], weights: &[[f32; 4]], palette: &[Mat4], out: &mut MeshData) {
    out.indices.clone_from(&base.indices);
    out.vertices.resize(base.vertices.len(), Vertex::default());
    for (i, v) in base.vertices.iter().enumerate() {
        let (j, w) = (joints[i], weights[i]);
        let mut m = Mat4::ZERO;
        let mut total = 0.0;
        for k in 0..4 {
            if w[k] > 0.0 {
                if let Some(p) = palette.get(j[k] as usize) {
                    m += *p * w[k];
                    total += w[k];
                }
            }
        }
        let m = if total > 0.0 { m * (1.0 / total) } else { Mat4::IDENTITY };
        let mut o = *v;
        o.position = m.transform_point3(Vec3::from(v.position)).into();
        o.normal = m.transform_vector3(Vec3::from(v.normal)).normalize_or(Vec3::Y).into();
        out.vertices[i] = o;
    }
}

/// Advances animators, poses node transforms, skins meshes.
#[derive(Default)]
pub struct AnimationPlugin {
    scratch: MeshData,
}

impl Plugin for AnimationPlugin {
    fn build(&mut self, _ctx: &mut Context) {
        #[cfg(feature = "scene")]
        {
            goat_scene::registry(_ctx).component(scene::CODEC).component(goat_scene::serde_codec::<Animation>("Animation"));
            goat_scene::registry(_ctx).describe("Animation", goat_scene::ComponentInfo::new("Miscellaneous/Animation").asset("clip", "animation").tip("clip", "An .anim.json clip (Window ▸ Animation)"));
        }
        #[cfg(feature = "script")]
        script::register(goat_script::extensions(_ctx));
    }

    fn update(&mut self, ctx: &mut Context) {
        play_authored_clips(ctx);
        let dt = ctx.time.delta;
        let Some(assets) = ctx.resources.get::<AssetServer>() else { return };
        let roots: Vec<Entity> = ctx.world.query::<&Animator>().without::<&goat_runtime::InactiveInHierarchy>().iter().map(|(e, _)| e).collect();
        for root in roots {
            let Ok(key) = ctx.world.get::<&goat_assets::ModelInstance>(root).map(|m| m.key.clone()) else { continue };
            let Some(model) = assets.loaded_model(&key) else { continue };
            let Ok(nodes) = ctx.world.get::<&ModelNodes>(root).map(|n| n.0.clone()) else { continue };
            let mut sampled = Vec::new();
            // controller mode: (layer weight, clips) per layer
            let mut layers = None;
            if let Ok(mut anim) = ctx.world.get::<&mut Animator>(root) {
                let paused = anim.paused;
                let step = if paused { 0.0 } else { dt * anim.speed };
                if let Some(c) = anim.controller.as_mut() {
                    let durations = |name: &str| model.data.animation(name).map(|a| a.duration);
                    layers = Some(c.update(step, &durations));
                }
                for s in anim.states.iter_mut().filter(|_| layers.is_none()) {
                    let Some(clip) = model.data.animation(&s.clip) else { continue };
                    if !paused {
                        s.time += dt * s.speed;
                        s.weight = (s.weight + s.fade * dt).clamp(0.0, 1.0);
                        if s.fade > 0.0 && s.weight >= 1.0 {
                            s.fade = 0.0;
                        }
                    }
                    let d = clip.duration.max(1e-4);
                    let t = if s.looping { s.time.rem_euclid(d) } else { s.time.min(d) };
                    sampled.push((s.clip.clone(), t, s.weight));
                }
                anim.states.retain(|s| !(s.fade < 0.0 && s.weight <= 0.0));
            }
            // events crossed this frame (clips weighing at least half)
            let (all, data) = match &layers {
                Some(l) => (l.iter().flat_map(|(lw, c)| c.iter().map(move |(n, t, w)| (n.clone(), *t, *w * lw))).collect::<Vec<_>>(), ctx.world.get::<&Animator>(root).ok().and_then(|a| a.controller.as_ref().map(|c| c.data.clone()))),
                None => (sampled.clone(), None),
            };
            if let Ok(mut anim) = ctx.world.get::<&mut Animator>(root) {
                let anim = &mut *anim;
                anim.fired.clear();
                let mut seen = HashSet::new();
                for (clip, t, w) in &all {
                    seen.insert(clip.clone());
                    let last = anim.last_times.insert(clip.clone(), *t);
                    if *w < 0.5 {
                        continue;
                    }
                    let list = data.as_ref().and_then(|d| d.events.get(clip)).or_else(|| anim.events.get(clip));
                    if let Some(list) = list {
                        let dur = model.data.animation(clip).map(|a| a.duration).unwrap_or(0.0);
                        crossed(list, last, *t, dur, &mut anim.fired, clip);
                    }
                }
                anim.last_times.retain(|k, _| seen.contains(k));
                // layer masks (once)
                if let (Some(d), None) = (&data, &anim.masks) {
                    anim.masks = Some(d.layers.iter().map(|l| (!l.mask.is_empty()).then(|| mask_nodes(&model.data, &l.mask))).collect());
                }
            }
            if let Some(layers) = layers {
                // layer 0 poses the model; the next ones override it (or add to it) by weight
                let masks = ctx.world.get::<&Animator>(root).ok().and_then(|a| a.masks.clone()).unwrap_or_default();
                for (li, (weight, clips)) in layers.into_iter().enumerate() {
                    let clips: Vec<(&ModelAnimation, f32, f32)> = clips.iter().filter_map(|(n, t, w)| model.data.animation(n).map(|a| (a, *t, *w * weight))).collect();
                    let additive = data.as_ref().and_then(|d| d.layers.get(li)).is_some_and(|l| l.blending == LayerBlending::Additive);
                    apply_layer(ctx.world, &nodes, &clips, masks.get(li).and_then(|m| m.as_ref()), additive);
                }
                continue;
            }
            let clips: Vec<(&ModelAnimation, f32, f32)> = sampled.iter().filter_map(|(c, t, w)| model.data.animation(c).map(|a| (a, *t, *w))).collect();
            apply(ctx.world, &nodes, &clips);
        }
    }

    fn late_update(&mut self, ctx: &mut Context) {
        let Some(assets) = ctx.resources.get::<AssetServer>() else { return };
        let skinned: Vec<Entity> = ctx.world.query::<&SkinnedMesh>().iter().map(|(e, _)| e).collect();
        for e in skinned {
            let Ok(sm) = ctx.world.get::<&SkinnedMesh>(e).map(|s| (*s).clone()) else { continue };
            let Some(model) = assets.loaded_model(&sm.model) else { continue };
            let Some(prim) = model.data.meshes.get(sm.mesh).and_then(|m| m.get(sm.primitive)) else { continue };
            let (Some(sv), Some(model_skin)) = (&prim.skin_vertices, model.data.skins.get(sm.skin)) else { continue };
            let Ok(nodes) = ctx.world.get::<&ModelNodes>(sm.root).map(|n| n.0.clone()) else { continue };
            let mesh_world = ctx.world.get::<&GlobalTransform>(e).map(|g| g.0).unwrap_or(Mat4::IDENTITY);
            let to_mesh = mesh_world.inverse();
            let palette: Vec<Mat4> = model_skin
                .joints
                .iter()
                .zip(&model_skin.inverse_bind)
                .map(|(&j, ibm)| {
                    let jw = nodes.get(j).copied().flatten().and_then(|je| ctx.world.get::<&GlobalTransform>(je).ok().map(|g| g.0)).unwrap_or(Mat4::IDENTITY);
                    to_mesh * jw * *ibm
                })
                .collect();
            skin(&prim.mesh, &sv.joints, &sv.weights, &palette, &mut self.scratch);
            // a mesh of this instance's own (the shared one stays in bind pose)
            let instance = match sm.instance {
                Some(m) => m,
                None => match ctx.gfx.create_mesh(&self.scratch) {
                    Ok(m) => {
                        if let Ok(mut s) = ctx.world.get::<&mut SkinnedMesh>(e) {
                            s.instance = Some(m);
                        }
                        if let Ok(mut r) = ctx.world.get::<&mut MeshRenderer>(e) {
                            r.mesh = m;
                        }
                        continue;
                    }
                    Err(_) => continue,
                },
            };
            let _ = ctx.gfx.update_mesh(instance, &self.scratch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goat_assets::{AssetIo, AssetServer};
    use goat_gfx::NullBackend;

    #[test]
    fn sampling_interpolates_and_clamps() {
        let times = [0.0, 1.0, 2.0];
        let values = [Vec4::ZERO, Vec4::new(2.0, 0.0, 0.0, 0.0), Vec4::new(2.0, 4.0, 0.0, 0.0)];
        assert_eq!(sample(&times, &values, Interpolation::Linear, false, 0.5).x, 1.0);
        assert_eq!(sample(&times, &values, Interpolation::Step, false, 1.5).y, 0.0);
        assert_eq!(sample(&times, &values, Interpolation::Linear, false, 9.0).y, 4.0);
        assert_eq!(sample(&times, &values, Interpolation::Linear, false, -1.0), Vec4::ZERO);
    }

    #[test]
    fn the_sample_tentacle_sways_and_skins() {
        let mut gfx = NullBackend::new();
        let mut assets = AssetServer::new(AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets")));
        let mut world = World::new();
        let root = assets.spawn_model(&mut world, &mut gfx, "models/tentacle.glb", None).unwrap();
        let nodes = world.get::<&ModelNodes>(root).unwrap().0.clone();
        let model = assets.loaded_model("models/tentacle.glb").unwrap();
        let sway = model.data.animation("Sway").unwrap();
        // pose at 0.5 s vs rest: some bone rotated
        let bone = sway.channels.iter().find(|c| c.path == AnimPath::Rotation).unwrap().node;
        let rest = world.get::<&Transform>(nodes[bone].unwrap()).unwrap().rotation;
        apply(&mut world, &nodes, &[(sway, 0.5, 1.0)]);
        let posed = world.get::<&Transform>(nodes[bone].unwrap()).unwrap().rotation;
        assert!(rest.angle_between(posed) > 0.05, "bone moved");
        // skinning with an identity palette returns the bind pose
        let prim = model.data.meshes.iter().flatten().find(|p| p.skin_vertices.is_some()).unwrap();
        let sv = prim.skin_vertices.as_ref().unwrap();
        let mut out = MeshData::default();
        skin(&prim.mesh, &sv.joints, &sv.weights, &vec![Mat4::IDENTITY; 4], &mut out);
        assert_eq!(out.vertices.len(), prim.mesh.vertices.len());
        assert!((Vec3::from(out.vertices[7].position) - Vec3::from(prim.mesh.vertices[7].position)).length() < 1e-5);
        // and moving every joint up moves the mesh up
        skin(&prim.mesh, &sv.joints, &sv.weights, &vec![Mat4::from_translation(Vec3::Y); 4], &mut out);
        assert!((out.vertices[7].position[1] - prim.mesh.vertices[7].position[1] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn the_sample_controller_drives_the_tentacle() {
        struct G;
        impl goat_runtime::Game for G {
            fn start(&mut self, ctx: &mut Context) {
                let mut assets = AssetServer::new(AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets")));
                let root = assets.spawn_model(ctx.world, ctx.gfx, "models/tentacle.glb", None).unwrap();
                let anim = Animator::from_controller_asset(&assets, "animators/tentacle.controller.json").unwrap();
                ctx.world.insert(root, (anim, goat_runtime::Name("T".into()))).unwrap();
                ctx.resources.insert(assets);
            }
            fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
                vec![Box::new(AnimationPlugin::default())]
            }
        }
        let mut gfx = NullBackend::new();
        let mut e = goat_runtime::Engine::new(G);
        e.frame(0.0, &mut gfx);
        let root = e.world.query::<&goat_runtime::Name>().iter().find(|(_, n)| n.0 == "T").unwrap().0;
        let nodes = e.world.get::<&ModelNodes>(root).unwrap().0.clone();
        let bone = e.resources.get::<AssetServer>().unwrap().loaded_model("models/tentacle.glb").unwrap().data.animation("Sway").unwrap().channels.iter().find(|c| c.path == AnimPath::Rotation).unwrap().node;
        let rest = e.world.get::<&Transform>(nodes[bone].unwrap()).unwrap().rotation;
        for _ in 0..30 {
            e.frame(1.0 / 60.0, &mut gfx);
        }
        let posed = e.world.get::<&Transform>(nodes[bone].unwrap()).unwrap().rotation;
        assert!(rest.angle_between(posed) > 0.01, "the Idle blend tree posed the bone");
        e.world.get::<&mut Animator>(root).unwrap().set_trigger("poke");
        e.frame(1.0 / 60.0, &mut gfx);
        e.frame(0.2, &mut gfx);
        assert_eq!(e.world.get::<&Animator>(root).unwrap().current_state(0), Some("Poke"));
        for _ in 0..120 {
            e.frame(1.0 / 60.0, &mut gfx);
        }
        assert_eq!(e.world.get::<&Animator>(root).unwrap().current_state(0), Some("Idle"), "back after the exit time");
    }

    #[test]
    fn crossfade_moves_weight_over_time() {
        let mut a = Animator::playing("A");
        a.crossfade("B", 0.5);
        assert_eq!(a.states.len(), 2);
        assert!(a.states.iter().all(|s| s.fade != 0.0));
    }

    #[test]
    fn events_fire_once_per_crossing_and_masks_limit_layers() {
        let mut out = Vec::new();
        let ev = [ClipEvent { time: 0.5, name: "step".into(), parameter: String::new() }];
        crossed(&ev, Some(0.4), 0.6, 1.0, &mut out, "Walk");
        crossed(&ev, Some(0.6), 0.9, 1.0, &mut out, "Walk");
        crossed(&ev, Some(0.9), 0.55, 1.0, &mut out, "Walk"); // looped: 0.9 → 1.0 → 0.55
        crossed(&ev, None, 0.0, 1.0, &mut out, "Walk");
        assert_eq!(out.len(), 2);
        // the tentacle playing Sway with an event, through the plugin
        struct G;
        impl goat_runtime::Game for G {
            fn start(&mut self, ctx: &mut Context) {
                let mut assets = AssetServer::new(AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets")));
                let root = assets.spawn_model(ctx.world, ctx.gfx, "models/tentacle.glb", None).unwrap();
                let mut a = Animator::playing("Sway");
                a.events.insert("Sway".into(), vec![ClipEvent { time: 0.1, name: "creak".into(), parameter: "x".into() }]);
                ctx.world.insert(root, (a, goat_runtime::Name("T".into()))).unwrap();
                ctx.resources.insert(assets);
            }
            fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
                vec![Box::new(AnimationPlugin::default())]
            }
        }
        let mut gfx = NullBackend::new();
        let mut e = goat_runtime::Engine::new(G);
        e.frame(0.0, &mut gfx);
        let root = e.world.query::<&goat_runtime::Name>().iter().find(|(_, n)| n.0 == "T").unwrap().0;
        let mut fired = 0;
        for _ in 0..12 {
            e.frame(1.0 / 60.0, &mut gfx);
            fired += e.world.get::<&Animator>(root).unwrap().fired.iter().filter(|f| f.name == "creak" && f.parameter == "x").count();
        }
        assert_eq!(fired, 1, "crossed 0.1 s once in 0.2 s");
        // mask: only the named node and its children
        let assets = e.resources.get::<AssetServer>().unwrap();
        let model = &assets.loaded_model("models/tentacle.glb").unwrap().data;
        let first = model.nodes.iter().position(|n| !n.children.is_empty()).unwrap();
        let set = mask_nodes(model, &[model.nodes[first].name.clone()]);
        assert!(set.contains(&first) && model.nodes[first].children.iter().all(|c| set.contains(c)));
        assert!(set.len() < model.nodes.len() || model.roots.contains(&first));
    }
}

/// The clip of an `.anim.json` key (loaded once).
pub fn load_anim_clip(ctx: &mut Context, key: &str) -> Option<Arc<AnimClip>> {
    if let Some(c) = ctx.resources.get::<AnimClips>().and_then(|c| c.0.get(key)) {
        return Some(c.clone());
    }
    let text = ctx.resources.get::<AssetServer>()?.io.read_string(key).ok()?;
    match AnimClip::parse(&text) {
        Ok(c) => {
            let c = Arc::new(c);
            ctx.resources.get_or_default::<AnimClips>().0.insert(key.to_string(), c.clone());
            Some(c)
        }
        Err(e) => {
            goat_core::warn!("{key}: {e}");
            None
        }
    }
}

/// Animation components: advances and samples their authored clip.
fn play_authored_clips(ctx: &mut Context) {
    let dt = ctx.time.delta;
    let list: Vec<(Entity, Option<String>)> = ctx
        .world
        .query_mut::<&mut Animation>()
        .without::<&goat_runtime::InactiveInHierarchy>()
        .into_iter()
        .map(|(e, a)| {
            if a.playing.is_none() && a.play_automatically && !a.clip.is_empty() && a.time == 0.0 {
                let c = a.clip.clone();
                a.play(&c);
            }
            a.time += dt * a.speed;
            (e, a.playing.clone())
        })
        .collect();
    for (e, key) in list {
        let Some(key) = key else { continue };
        let Some(clip) = load_anim_clip(ctx, &key) else { continue };
        let t = ctx.world.get::<&Animation>(e).map(|a| a.time).unwrap_or(0.0);
        if !clip.looping && t > clip.duration() {
            if let Ok(mut a) = ctx.world.get::<&mut Animation>(e) {
                a.time = clip.duration();
            }
        }
        let samples = clip::sample_clip(ctx.world, e, &clip, t);
        apply_component_samples(ctx, &samples);
    }
}

/// Transform values directly, component numbers through the scene codecs (feature `scene`).
pub fn apply_component_samples(ctx: &mut Context, samples: &[(Entity, String, f32)]) {
    #[cfg(feature = "scene")]
    {
        let registry = ctx.resources.take::<goat_scene::SceneRegistry>();
        let mut pending: Vec<(Entity, String, Vec<String>, f32)> = Vec::new();
        clip::apply_samples(ctx.world, samples, |_, e, comp, path, v| pending.push((e, comp.to_string(), path.iter().map(|s| s.to_string()).collect(), v)));
        if let (Some(reg), false) = (registry.as_ref(), pending.is_empty()) {
            for (e, comp, path, v) in pending {
                let Some(codec) = reg.find_component(&comp).copied() else { continue };
                let value = ctx.resources.get::<AssetServer>().and_then(|a| (codec.save)(&goat_scene::SaveContext { world: ctx.world, assets: a }, e));
                let Some(mut value) = value else { continue };
                let parts: Vec<&str> = path.iter().map(String::as_str).collect();
                if !clip::set_json_number(&mut value, &parts, v) {
                    continue;
                }
                if let Some(assets) = ctx.resources.get_mut::<AssetServer>() {
                    let mut lc = goat_scene::LoadContext { world: ctx.world, gfx: ctx.gfx, assets, source: "animation" };
                    let _ = (codec.load)(&mut lc, e, &value);
                }
            }
        }
        if let Some(r) = registry {
            ctx.resources.put_back(r);
        }
    }
    #[cfg(not(feature = "scene"))]
    clip::apply_samples(ctx.world, samples, |_, _, _, _, _| {});
}
