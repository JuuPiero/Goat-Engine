//! Scenes and prefabs (Unity's .unity / .prefab), as JSON:
//!
//! ```json
//! { "version": 1, "entities": [
//!   { "id": 1, "name": "Sun", "components": { "Transform": { "euler": [50, 30, 0] }, "DirectionalLight": { "color": "#fff8e6", "intensity": 0.8 } } },
//!   { "id": 2, "name": "Crate", "components": { "Transform": { "position": [0, 0.5, 0] },
//!       "MeshRenderer": { "mesh": "builtin:cube", "material": "materials/crate.mat.json" } },
//!     "scripts": [ { "type": "Spinner", "speed": 1.0 } ] },
//!   { "id": 3, "name": "Robot", "parent": 2, "model": "models/robot.glb" },
//!   { "id": 4, "name": "Tree", "prefab": "prefabs/tree.prefab.json", "components": { "Transform": { "position": [3, 0, 0] } } }
//! ] }
//! ```
//!
//! - `id`s are local to the file (parents refer to them); assets are referenced by key.
//! - `model`: the entity gets the model's node hierarchy as children (not stored in the file).
//! - `prefab`: the entity gets the prefab's entities as children; its own components override
//!   (position the instance). Prefab children are not stored either: editing the prefab file
//!   updates every scene using it.
//! - Components and behaviours go through the [`SceneRegistry`] (a resource): built-ins are
//!   registered, subsystems and games add theirs.
//! - Entities with [`DontSave`](goat_runtime::DontSave) are never saved.

pub mod builtins;
pub mod registry;

pub use registry::{load_serde, remove_typed, save_serde, serde_codec, ComponentCodec, LoadContext, SaveContext, SceneRegistry};

use std::collections::{HashMap, HashSet};

use goat_assets::{AssetServer, ModelInstance};
use goat_gfx::GraphicsBackend;
use goat_runtime::{Context, DontSave, Entity, GlobalTransform, Name, Parent, Plugin, Scripts, Transform, World};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const SCENE_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SceneFile {
    pub version: u32,
    pub entities: Vec<EntityEntry>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct EntityEntry {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefab: Option<String>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub components: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<Value>,
}

/// Load order of an entity (file order, increasing across loads): saving keeps it, whatever
/// entity ids the ECS reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SceneIndex(pub u64);

// u32: the 3DS (ARMv6K) has no 64-bit atomics
static NEXT_INDEX: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// On the entity that holds a prefab instance (its children come from the prefab file).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefabInstance {
    pub key: String,
}

#[derive(Debug)]
pub enum SceneError {
    Asset(goat_assets::AssetError),
    Format(String, String),
    Missing(&'static str),
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SceneError::Asset(e) => write!(f, "{e}"),
            SceneError::Format(k, e) => write!(f, "{k}: {e}"),
            SceneError::Missing(what) => write!(f, "{what} missing (add AssetsPlugin and ScenePlugin)"),
        }
    }
}

impl std::error::Error for SceneError {}

impl From<goat_assets::AssetError> for SceneError {
    fn from(e: goat_assets::AssetError) -> Self {
        SceneError::Asset(e)
    }
}

/// How [`load_scene`] treats what is already in the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadMode {
    /// Despawn everything first (except `DontSave` helpers), like Unity's LoadScene(Single).
    Single,
    /// Add to the world (Unity's Additive).
    Additive,
}

pub fn parse(source: &str, text: &str) -> Result<SceneFile, SceneError> {
    let file: SceneFile = serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(text)).map_err(|e| SceneError::Format(source.into(), e.to_string()))?;
    if file.version > SCENE_VERSION {
        return Err(SceneError::Format(source.into(), format!("scene version {} is newer than this engine ({SCENE_VERSION})", file.version)));
    }
    Ok(file)
}

const MAX_PREFAB_DEPTH: u32 = 16;

/// Spawns a scene file's entities. Root entities get `parent` (if any). Returns the roots.
pub fn spawn_file(world: &mut World, gfx: &mut dyn GraphicsBackend, assets: &mut AssetServer, registry: &SceneRegistry, file: &SceneFile, source: &str, parent: Option<Entity>) -> Result<Vec<Entity>, SceneError> {
    spawn_file_depth(world, gfx, assets, registry, file, source, parent, 0)
}

#[allow(clippy::too_many_arguments)]
fn spawn_file_depth(world: &mut World, gfx: &mut dyn GraphicsBackend, assets: &mut AssetServer, registry: &SceneRegistry, file: &SceneFile, source: &str, parent: Option<Entity>, depth: u32) -> Result<Vec<Entity>, SceneError> {
    if depth > MAX_PREFAB_DEPTH {
        return Err(SceneError::Format(source.into(), "prefabs nested too deep (a prefab containing itself?)".into()));
    }
    let mut ids: HashMap<u64, Entity> = HashMap::new();
    for entry in &file.entities {
        let index = SceneIndex(NEXT_INDEX.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u64);
        let e = world.spawn((GlobalTransform::default(), index));
        if let Some(n) = &entry.name {
            let _ = world.insert_one(e, Name(n.clone()));
        }
        ids.insert(entry.id, e);
    }
    let mut roots = Vec::new();
    for entry in &file.entities {
        let e = ids[&entry.id];
        {
            let mut lc = LoadContext { world: &mut *world, gfx: &mut *gfx, assets: &mut *assets, source };
            for codec in registry.components() {
                if let Some(v) = entry.components.get(codec.name) {
                    (codec.load)(&mut lc, e, v).map_err(|err| SceneError::Format(source.into(), format!("entity {} {}: {err}", entry.id, codec.name)))?;
                }
            }
        }
        for name in entry.components.keys() {
            if registry.find_component(name).is_none() {
                goat_core::warn!("{source}: entity {}: unknown component {name} (skipped)", entry.id);
            }
        }
        if world.get::<&Transform>(e).is_err() {
            let _ = world.insert_one(e, Transform::default());
        }
        match entry.parent.and_then(|p| ids.get(&p)) {
            Some(&p) => {
                let _ = world.insert_one(e, Parent(p));
            }
            None => {
                if let Some(p) = parent {
                    let _ = world.insert_one(e, Parent(p));
                }
                roots.push(e);
            }
        }
        if !entry.scripts.is_empty() {
            let mut scripts = Scripts::new();
            for s in &entry.scripts {
                match registry.load_behaviour(s) {
                    Ok(b) => scripts.add_boxed(b),
                    Err(err) => goat_core::warn!("{source}: entity {}: {err}", entry.id),
                }
            }
            let _ = world.insert_one(e, scripts);
        }
        if let Some(model) = &entry.model {
            assets.spawn_model_into(world, gfx, model, e)?;
        }
        if let Some(prefab) = &entry.prefab {
            let text = assets.io.read_string(prefab)?;
            let pf = parse(prefab, &text)?;
            spawn_file_depth(world, gfx, assets, registry, &pf, prefab, Some(e), depth + 1)?;
            let _ = world.insert_one(e, PrefabInstance { key: prefab.clone() });
        }
    }
    Ok(roots)
}

/// Builds a scene file from the world: every entity with a Transform, except `DontSave` ones and
/// the children generated by model / prefab instances.
pub fn save_world(world: &World, assets: &AssetServer, registry: &SceneRegistry) -> SceneFile {
    let parents: HashMap<Entity, Entity> = world.query::<&Parent>().iter().map(|(e, p)| (e, p.0)).collect();
    let instance_roots: HashSet<Entity> = world
        .query::<()>()
        .with::<&ModelInstance>()
        .iter()
        .map(|(e, _)| e)
        .chain(world.query::<()>().with::<&PrefabInstance>().iter().map(|(e, _)| e))
        .collect();
    let generated = |e: Entity| {
        let mut cur = parents.get(&e).copied();
        let mut guard = 0;
        while let Some(p) = cur {
            if instance_roots.contains(&p) {
                return true;
            }
            guard += 1;
            if guard > 256 {
                break;
            }
            cur = parents.get(&p).copied();
        }
        false
    };
    let mut entities: Vec<Entity> = world
        .query::<()>()
        .with::<&Transform>()
        .without::<&DontSave>()
        .iter()
        .map(|(e, _)| e)
        .filter(|&e| !generated(e))
        .collect();
    // load order (then creation order for entities made since): a reloaded scene saves back identically
    entities.sort_by_key(|e| (world.get::<&SceneIndex>(*e).map(|i| i.0).unwrap_or(u64::MAX), e.id()));
    let ids: HashMap<Entity, u64> = entities.iter().enumerate().map(|(i, &e)| (e, i as u64 + 1)).collect();

    let ctx = SaveContext { world, assets };
    let mut out = SceneFile { version: SCENE_VERSION, entities: Vec::with_capacity(entities.len()) };
    for &e in &entities {
        let mut entry = EntityEntry { id: ids[&e], ..Default::default() };
        entry.name = world.get::<&Name>(e).ok().map(|n| n.0.clone());
        entry.parent = parents.get(&e).and_then(|p| ids.get(p)).copied();
        entry.model = world.get::<&ModelInstance>(e).ok().map(|m| m.key.clone());
        entry.prefab = world.get::<&PrefabInstance>(e).ok().map(|p| p.key.clone());
        for codec in registry.components() {
            if let Some(v) = (codec.save)(&ctx, e) {
                // an all-default transform is just {}
                if !(codec.name == "Transform" && v.as_object().is_some_and(|o| o.is_empty())) {
                    entry.components.insert(codec.name.to_string(), v);
                }
            }
        }
        if let Ok(scripts) = world.get::<&Scripts>(e) {
            for b in scripts.iter() {
                match registry.save_behaviour(b) {
                    Some(v) => entry.scripts.push(v),
                    None => goat_core::warn!("scene save: behaviour {} isn't registered (skipped)", b.name()),
                }
            }
        }
        out.entities.push(entry);
    }
    out
}

// ---------------------------------------------------------------------- game-facing API

/// Runs `f` with the world, the GPU, the asset server and the registry (taken out of the
/// resources for the duration).
fn with_parts<R>(ctx: &mut Context, f: impl FnOnce(&mut World, &mut dyn GraphicsBackend, &mut AssetServer, &SceneRegistry) -> R) -> Result<R, SceneError> {
    let registry = ctx.resources.take::<SceneRegistry>().unwrap_or_default();
    let result = match ctx.resources.get_mut::<AssetServer>() {
        Some(assets) => Ok(f(ctx.world, ctx.gfx, assets, &registry)),
        None => Err(SceneError::Missing("AssetServer")),
    };
    ctx.resources.put_back(registry);
    result
}

/// Loads `key` (a `.scene.json` under the asset root). Returns its root entities.
pub fn load_scene(ctx: &mut Context, key: &str, mode: LoadMode) -> Result<Vec<Entity>, SceneError> {
    if mode == LoadMode::Single {
        let doomed: Vec<Entity> = ctx.world.query::<()>().without::<&DontSave>().iter().map(|(e, _)| e).collect();
        for e in doomed {
            let _ = ctx.world.despawn(e);
        }
    }
    with_parts(ctx, |world, gfx, assets, registry| {
        let text = assets.io.read_string(key)?;
        let file = parse(key, &text)?;
        spawn_file(world, gfx, assets, registry, &file, key, None)
    })?
}

/// Instantiates a prefab file: a new entity named after it (with [`PrefabInstance`]) whose
/// children are the prefab's entities. Returns that entity.
pub fn instantiate(ctx: &mut Context, key: &str, parent: Option<Entity>) -> Result<Entity, SceneError> {
    with_parts(ctx, |world, gfx, assets, registry| {
        let text = assets.io.read_string(key)?;
        let file = parse(key, &text)?;
        let name = key.rsplit('/').next().unwrap_or(key).trim_end_matches(".json").trim_end_matches(".prefab").to_string();
        let root = world.spawn((Name(name), Transform::default(), GlobalTransform::default(), PrefabInstance { key: key.to_string() }));
        if let Some(p) = parent {
            let _ = world.insert_one(root, Parent(p));
        }
        spawn_file(world, gfx, assets, registry, &file, key, Some(root))?;
        Ok(root)
    })?
}

/// The world as a scene file (JSON text, pretty-printed).
pub fn save_scene(ctx: &mut Context) -> Result<String, SceneError> {
    let registry = ctx.resources.take::<SceneRegistry>().unwrap_or_default();
    let text = match ctx.resources.get::<AssetServer>() {
        Some(assets) => Ok(serde_json::to_string_pretty(&save_world(ctx.world, assets, &registry)).unwrap_or_default()),
        None => Err(SceneError::Missing("AssetServer")),
    };
    ctx.resources.put_back(registry);
    text
}

/// The registry, to add behaviours / components: `goat_scene::registry(ctx).behaviour::<Spinner>("Spinner")`.
pub fn registry<'a>(ctx: &'a mut Context) -> &'a mut SceneRegistry {
    ctx.resources.get_or_default::<SceneRegistry>()
}

/// Inserts the [`SceneRegistry`] (built-in components registered). Put it after `AssetsPlugin`.
#[derive(Default)]
pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&mut self, ctx: &mut Context) {
        ctx.resources.get_or_default::<SceneRegistry>();
    }
}

#[cfg(test)]
mod tests;
