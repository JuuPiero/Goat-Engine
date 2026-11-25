//! Several scenes at once (Unity's / PiaEngine's SceneManager): every entity a scene file spawned
//! carries [`SceneMember`]; [`LoadedScenes`] lists the loaded scenes, the active one and the
//! load / unload events. Entities made at run time without a member belong to the active scene
//! (Unity puts new objects there). [`dont_destroy_on_load`] moves a root (and its children) into
//! the hidden "DontDestroyOnLoad" scene, which Single loads keep.
//!
//! Loads and unloads asked for by scripts happen at the end of the frame ([`SceneOps`], run by
//! the ScenePlugin's `late_update`), each with an id so scripts can wait for it (AsyncOperation).

use std::collections::{HashMap, HashSet};

use goat_runtime::{Context, DontSave, Entity, Parent, World};

use crate::{load_scene_impl, LoadMode, SceneError};

/// The handle of the scene an entity was loaded with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SceneMember(pub u32);

/// Unity's "DontDestroyOnLoad" scene (kept by Single loads, not counted in `sceneCount`).
pub const DONT_DESTROY_ON_LOAD: u32 = u32::MAX;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedScene {
    pub handle: u32,
    /// Asset key of the scene file.
    pub key: String,
}

impl LoadedScene {
    /// The file stem (`scenes/Level1.scene.json` → `Level1`), Unity's `Scene.name`.
    pub fn name(&self) -> String {
        name_of(&self.key)
    }
}

pub fn name_of(key: &str) -> String {
    key.rsplit('/').next().unwrap_or(key).trim_end_matches(".json").trim_end_matches(".scene").to_string()
}

/// The loaded scenes (load order), the active one, and what happened since scripts last asked.
#[derive(Default, Debug)]
pub struct LoadedScenes {
    pub list: Vec<LoadedScene>,
    /// The active scene's handle (0 = none yet).
    pub active: u32,
    next: u32,
    /// Handles loaded / (handle, key) unloaded / active changes since the last `take_events`.
    pub loaded_events: Vec<u32>,
    pub unloaded_events: Vec<(u32, String)>,
    pub active_changed: bool,
}

impl LoadedScenes {
    fn add(&mut self, key: &str) -> u32 {
        self.next += 1;
        let h = self.next;
        self.list.push(LoadedScene { handle: h, key: key.to_string() });
        self.loaded_events.push(h);
        self.trim_events();
        h
    }
    /// Events nobody asked for don't pile up.
    fn trim_events(&mut self) {
        if self.loaded_events.len() > 64 {
            self.loaded_events.drain(..self.loaded_events.len() - 64);
        }
        if self.unloaded_events.len() > 64 {
            self.unloaded_events.drain(..self.unloaded_events.len() - 64);
        }
    }
    pub fn get(&self, handle: u32) -> Option<&LoadedScene> {
        self.list.iter().find(|s| s.handle == handle)
    }
    pub fn active_scene(&self) -> Option<&LoadedScene> {
        self.get(self.active)
    }
    /// The first loaded scene named `name` (file stem) or with that key.
    pub fn by_name(&self, name: &str) -> Option<&LoadedScene> {
        self.list.iter().find(|s| s.key == name || s.name() == name)
    }
}

/// The scene an entity belongs to: its own [`SceneMember`], else its parent's, else the active
/// scene (objects made at run time).
pub fn scene_of(world: &World, scenes: &LoadedScenes, e: Entity) -> u32 {
    let mut cur = Some(e);
    for _ in 0..256 {
        let Some(x) = cur else { break };
        if let Ok(m) = world.get::<&SceneMember>(x) {
            return m.0;
        }
        cur = world.get::<&Parent>(x).ok().map(|p| p.0);
    }
    scenes.active
}

fn subtree(world: &World, root: Entity) -> Vec<Entity> {
    let mut children: HashMap<Entity, Vec<Entity>> = HashMap::new();
    for (c, p) in world.query::<&Parent>().iter() {
        children.entry(p.0).or_default().push(c);
    }
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(x) = stack.pop() {
        out.push(x);
        if let Some(cs) = children.get(&x) {
            stack.extend(cs.iter().copied());
        }
    }
    out
}

/// Unity's `DontDestroyOnLoad`: the object's root and everything below it survive Single loads.
pub fn dont_destroy_on_load(world: &mut World, e: Entity) {
    let mut root = e;
    for _ in 0..256 {
        match world.get::<&Parent>(root) {
            Ok(p) => root = p.0,
            Err(_) => break,
        }
    }
    for x in subtree(world, root) {
        let _ = world.insert_one(x, SceneMember(DONT_DESTROY_ON_LOAD));
    }
}

/// Loads a scene now (see [`crate::load_scene`]) and returns its handle.
pub fn load(ctx: &mut Context, key: &str, mode: LoadMode) -> Result<(u32, Vec<Entity>), SceneError> {
    if mode == LoadMode::Single {
        // everything but editor helpers and DontDestroyOnLoad objects
        let keep: HashSet<Entity> = ctx.world.query::<&SceneMember>().iter().filter(|(_, m)| m.0 == DONT_DESTROY_ON_LOAD).map(|(e, _)| e).collect();
        let doomed: Vec<Entity> = ctx.world.query::<()>().without::<&DontSave>().iter().map(|(e, _)| e).filter(|e| !keep.contains(e)).collect();
        for e in doomed {
            let _ = ctx.world.despawn(e);
        }
        let s = ctx.resources.get_or_default::<LoadedScenes>();
        let gone: Vec<(u32, String)> = s.list.drain(..).map(|x| (x.handle, x.key)).collect();
        s.unloaded_events.extend(gone);
        s.trim_events();
    }
    let before: HashSet<Entity> = ctx.world.iter().map(|e| e.entity()).collect();
    let roots = load_scene_impl(ctx, key)?;
    let spawned: Vec<Entity> = ctx.world.iter().map(|e| e.entity()).filter(|e| !before.contains(e)).collect();
    let s = ctx.resources.get_or_default::<LoadedScenes>();
    let h = s.add(key);
    if mode == LoadMode::Single || s.active == 0 || s.get(s.active).is_none() {
        s.active = h;
        s.active_changed = true;
    }
    for e in spawned {
        let _ = ctx.world.insert_one(e, SceneMember(h));
    }
    sync_active_key(ctx);
    Ok((h, roots))
}

/// Unloads a loaded scene: its objects (and anything parented under them) go away. The last
/// scene can't be unloaded (Unity).
pub fn unload(ctx: &mut Context, handle: u32) -> Result<(), SceneError> {
    let s = ctx.resources.get_or_default::<LoadedScenes>();
    let Some(i) = s.list.iter().position(|x| x.handle == handle) else { return Err(SceneError::Format(handle.to_string(), "not a loaded scene".into())) };
    if s.list.len() == 1 {
        return Err(SceneError::Format(s.list[0].key.clone(), "the only loaded scene can't be unloaded".into()));
    }
    let gone = s.list.remove(i);
    if s.active == handle {
        s.active = s.list[0].handle;
        s.active_changed = true;
    }
    s.unloaded_events.push((gone.handle, gone.key));
    let members: Vec<Entity> = ctx.world.query::<&SceneMember>().iter().filter(|(_, m)| m.0 == handle).map(|(e, _)| e).collect();
    let mut doomed = HashSet::new();
    for e in members {
        doomed.extend(subtree(ctx.world, e));
    }
    for e in doomed {
        let _ = ctx.world.despawn(e);
    }
    sync_active_key(ctx);
    Ok(())
}

/// Unity's `SceneManager.SetActiveScene`.
pub fn set_active(ctx: &mut Context, handle: u32) -> bool {
    let s = ctx.resources.get_or_default::<LoadedScenes>();
    if s.get(handle).is_none() {
        return false;
    }
    if s.active != handle {
        s.active = handle;
        s.active_changed = true;
    }
    sync_active_key(ctx);
    true
}

fn sync_active_key(ctx: &mut Context) {
    let key = ctx.resources.get::<LoadedScenes>().and_then(|s| s.active_scene().map(|x| x.key.clone())).unwrap_or_default();
    ctx.resources.get_or_default::<crate::ActiveScene>().0 = key;
}

/// A queued load / unload (scripts' `loadScene` / `loadSceneAsync` / `unloadSceneAsync`).
#[derive(Clone, Debug)]
pub enum SceneOp {
    Load(String, LoadMode),
    Unload(u32),
}

/// Scene operations waiting for the end of the frame, by id; `done` = ids finished (kept until
/// asked, a few hundred at most).
#[derive(Default)]
pub struct SceneOps {
    next: u64,
    pub pending: Vec<(u64, SceneOp)>,
    pub done: HashSet<u64>,
}

impl SceneOps {
    pub fn push(&mut self, op: SceneOp) -> u64 {
        self.next += 1;
        self.pending.push((self.next, op));
        self.next
    }
}

/// Runs the queued operations (ScenePlugin `late_update`).
pub fn run_ops(ctx: &mut Context) {
    let pending = match ctx.resources.get_mut::<SceneOps>() {
        Some(o) if !o.pending.is_empty() => std::mem::take(&mut o.pending),
        _ => return,
    };
    for (id, op) in pending {
        let r = match &op {
            SceneOp::Load(key, mode) => load(ctx, key, *mode).map(|_| ()),
            SceneOp::Unload(h) => unload(ctx, *h),
        };
        if let Err(e) = r {
            goat_core::error!("{}: {e}", match op { SceneOp::Load(k, _) => format!("loadScene {k}"), SceneOp::Unload(h) => format!("unloadScene {h}") });
        }
        let ops = ctx.resources.get_or_default::<SceneOps>();
        ops.done.insert(id);
        if ops.done.len() > 512 {
            let oldest = *ops.done.iter().min().unwrap_or(&0);
            ops.done.remove(&oldest);
        }
    }
}
