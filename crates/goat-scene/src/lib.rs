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
mod component_info;
pub mod prefab;
pub mod registry;

pub use prefab::{apply_overrides, find_in_instance, resolve_file, set_track_overrides, PrefabBaseline, PrefabOverride, PrefabSource};

pub use registry::{load_serde, remove_typed, save_serde, serde_codec, ActionFn, ComponentCodec, ComponentInfo, FieldHint, LoadContext, SaveContext, SceneRegistry};

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
    /// Prefab variants: the prefab this one is based on ([`prefab`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Prefab variants: changes to the base's entities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<PrefabOverride>,
    #[serde(default)]
    pub entities: Vec<EntityEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EntityEntry {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `false` = switched off (Unity's `m_IsActive`); omitted when active.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub active: bool,
    /// Unity's tag (`Player`…); omitted when untagged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    /// With `parent` = a prefab instance: the id path of the real parent inside it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parent_target: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefab: Option<String>,
    /// Changes to entities inside the prefab instance ([`prefab`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<PrefabOverride>,
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
#[derive(Clone, Debug, PartialEq)]
pub struct PrefabInstance {
    pub key: String,
    /// The overrides it was loaded with (saved back, plus new edits when tracking).
    pub overrides: Vec<PrefabOverride>,
}

impl PrefabInstance {
    pub fn new(key: impl Into<String>) -> Self {
        PrefabInstance { key: key.into(), overrides: Vec::new() }
    }
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

pub(crate) const MAX_PREFAB_DEPTH: u32 = 16;

/// Spawns a scene file's entities. Root entities get `parent` (if any). Returns the roots.
/// A file with a `base` (prefab variant) is resolved first.
pub fn spawn_file(world: &mut World, gfx: &mut dyn GraphicsBackend, assets: &mut AssetServer, registry: &SceneRegistry, file: &SceneFile, source: &str, parent: Option<Entity>) -> Result<Vec<Entity>, SceneError> {
    let file = prefab::resolve_variant(assets, source, file.clone(), 0)?;
    spawn_file_depth(world, gfx, assets, registry, &file, source, parent, 0, false)
}

/// Makes `root` an instance of the prefab `key`: its entities spawn under it (the editor's
/// drag and drop; [`instantiate`] for games).
pub fn spawn_prefab_instance(world: &mut World, gfx: &mut dyn GraphicsBackend, assets: &mut AssetServer, registry: &SceneRegistry, key: &str, root: Entity) -> Result<(), SceneError> {
    spawn_prefab(world, gfx, assets, registry, key, &[], root, 1)?;
    let _ = world.insert_one(root, PrefabInstance::new(key));
    Ok(())
}

/// Spawns a prefab's entities (marked with [`PrefabSource`], baselines when tracking) under `parent`.
#[allow(clippy::too_many_arguments)]
fn spawn_prefab(world: &mut World, gfx: &mut dyn GraphicsBackend, assets: &mut AssetServer, registry: &SceneRegistry, key: &str, overrides: &[PrefabOverride], parent: Entity, depth: u32) -> Result<(), SceneError> {
    if depth > MAX_PREFAB_DEPTH {
        return Err(SceneError::Format(key.into(), "prefabs nested too deep (a prefab containing itself?)".into()));
    }
    let mut pf = resolve_file(assets, key)?;
    apply_overrides(&mut pf, overrides, key);
    spawn_file_depth(world, gfx, assets, registry, &pf, key, Some(parent), depth, true)?;
    Ok(())
}

/// Components + behaviours of an entity as saved (an all-default transform is left out).
fn save_parts(ctx: &SaveContext, registry: &SceneRegistry, e: Entity) -> (Map<String, Value>, Vec<Value>) {
    let mut components = Map::new();
    for codec in registry.components() {
        if let Some(v) = (codec.save)(ctx, e) {
            if !(codec.name == "Transform" && v.as_object().is_some_and(|o| o.is_empty())) {
                components.insert(codec.name.to_string(), v);
            }
        }
    }
    let mut scripts = Vec::new();
    if let Ok(list) = ctx.world.get::<&Scripts>(e) {
        for b in list.iter() {
            match registry.save_behaviour(b) {
                Some(v) => scripts.push(v),
                None => goat_core::warn!("scene save: behaviour {} isn't registered (skipped)", b.name()),
            }
        }
    }
    (components, scripts)
}

#[allow(clippy::too_many_arguments)]
fn spawn_file_depth(world: &mut World, gfx: &mut dyn GraphicsBackend, assets: &mut AssetServer, registry: &SceneRegistry, file: &SceneFile, source: &str, parent: Option<Entity>, depth: u32, from_prefab: bool) -> Result<Vec<Entity>, SceneError> {
    if depth > MAX_PREFAB_DEPTH {
        return Err(SceneError::Format(source.into(), "prefabs nested too deep (a prefab containing itself?)".into()));
    }
    let mut ids: HashMap<u64, Entity> = HashMap::new();
    for entry in &file.entities {
        let index = SceneIndex(NEXT_INDEX.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u64);
        let e = world.spawn((GlobalTransform::default(), index));
        if !entry.active {
            let _ = world.insert_one(e, goat_runtime::Inactive);
        }
        if let Some(t) = entry.tag.as_ref().filter(|t| !t.is_empty() && *t != "Untagged") {
            let _ = world.insert_one(e, goat_runtime::Tag(t.clone()));
        }
        if let Some(n) = &entry.name {
            let _ = world.insert_one(e, Name(n.clone()));
        }
        if from_prefab {
            let _ = world.insert_one(e, PrefabSource(entry.id));
        }
        ids.insert(entry.id, e);
    }
    // entities parented inside prefab instances: resolved once those are spawned
    let mut deferred_parents: Vec<(Entity, Entity, &[u64])> = Vec::new();
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
            Some(&p) if !entry.parent_target.is_empty() => {
                let _ = world.insert_one(e, Parent(p));
                deferred_parents.push((e, p, &entry.parent_target));
            }
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
            spawn_prefab(world, gfx, assets, registry, prefab, &entry.overrides, e, depth + 1)?;
            let _ = world.insert_one(e, PrefabInstance { key: prefab.clone(), overrides: entry.overrides.clone() });
        }
    }
    for (e, root, path) in deferred_parents {
        match find_in_instance(world, root, path) {
            Some(p) => {
                let _ = world.insert_one(e, Parent(p));
            }
            None => goat_core::warn!("{source}: parent {path:?} isn't in the prefab instance any more (kept under the instance)"),
        }
    }
    if from_prefab && prefab::track_overrides() {
        let ctx = SaveContext { world: &*world, assets: &*assets };
        let baselines: Vec<(Entity, PrefabBaseline)> = ids
            .values()
            .map(|&e| {
                let (components, scripts) = save_parts(&ctx, registry, e);
                (e, PrefabBaseline { name: world.get::<&Name>(e).ok().map(|n| n.0.clone()), active: world.get::<&goat_runtime::Inactive>(e).is_err(), components, scripts })
            })
            .collect();
        for (e, b) in baselines {
            let _ = world.insert_one(e, b);
        }
    }
    Ok(roots)
}

/// Builds a scene file from the world: every entity with a Transform, except `DontSave` ones and
/// the children generated by model / prefab instances.
pub fn save_world(world: &World, assets: &AssetServer, registry: &SceneRegistry) -> SceneFile {
    save_subset(world, assets, registry, None)
}

/// The entities a scene saves (generated instance children and `DontSave` left out), in file
/// order: [`save_world`] writes entity `i` of this list with id `i + 1`.
pub fn saved_entities(world: &World) -> Vec<Entity> {
    let mut out = Vec::new();
    save_subset_impl(world, None, &mut |list| out = list.to_vec());
    out
}

/// Some entities and their descendants as a scene file (copy / paste, duplicate): parents
/// outside the set are dropped (the copies become roots of the file).
pub fn save_entities(world: &World, assets: &AssetServer, registry: &SceneRegistry, roots: &[Entity]) -> SceneFile {
    let parents: HashMap<Entity, Entity> = world.query::<&Parent>().iter().map(|(e, p)| (e, p.0)).collect();
    let set: HashSet<Entity> = world
        .query::<()>()
        .iter()
        .map(|(e, _)| e)
        .filter(|e| {
            let mut cur = Some(*e);
            let mut guard = 0;
            while let Some(c) = cur {
                if roots.contains(&c) {
                    return true;
                }
                guard += 1;
                if guard > 256 {
                    break;
                }
                cur = parents.get(&c).copied();
            }
            false
        })
        .collect();
    save_subset(world, assets, registry, Some(&set))
}

/// Model nodes and entities spawned from a prefab are generated (children added under them are saved).
fn save_subset_impl(world: &World, only: Option<&HashSet<Entity>>, f: &mut dyn FnMut(&[Entity])) {
    let parents: HashMap<Entity, Entity> = world.query::<&Parent>().iter().map(|(e, p)| (e, p.0)).collect();
    let instance_roots: HashSet<Entity> = world.query::<()>().with::<&ModelInstance>().iter().map(|(e, _)| e).chain(world.query::<()>().with::<&PrefabInstance>().iter().map(|(e, _)| e)).collect();
    let generated = |e: Entity| {
        let mut cur = parents.get(&e).copied();
        let mut guard = 0;
        while let Some(p) = cur {
            if world.get::<&ModelInstance>(p).is_ok() {
                return true;
            }
            if instance_roots.contains(&p) {
                return world.get::<&PrefabSource>(e).is_ok();
            }
            guard += 1;
            if guard > 256 {
                break;
            }
            cur = parents.get(&p).copied();
        }
        false
    };
    let mut entities: Vec<Entity> = world.query::<()>().with::<&Transform>().without::<&DontSave>().iter().map(|(e, _)| e).filter(|&e| !generated(e) && only.is_none_or(|s| s.contains(&e))).collect();
    entities.sort_by_key(|e| (world.get::<&SceneIndex>(*e).map(|i| i.0).unwrap_or(u64::MAX), e.id()));
    f(&entities);
}

fn save_subset(world: &World, assets: &AssetServer, registry: &SceneRegistry, only: Option<&HashSet<Entity>>) -> SceneFile {
    let parents: HashMap<Entity, Entity> = world.query::<&Parent>().iter().map(|(e, p)| (e, p.0)).collect();
    let instance_roots: HashSet<Entity> = world
        .query::<()>()
        .with::<&ModelInstance>()
        .iter()
        .map(|(e, _)| e)
        .chain(world.query::<()>().with::<&PrefabInstance>().iter().map(|(e, _)| e))
        .collect();
    // load order (then creation order for entities made since): a reloaded scene saves back identically
    let mut entities = Vec::new();
    save_subset_impl(world, only, &mut |list| entities = list.to_vec());
    let ids: HashMap<Entity, u64> = entities.iter().enumerate().map(|(i, &e)| (e, i as u64 + 1)).collect();

    let ctx = SaveContext { world, assets };
    // the outermost prefab instance a generated entity belongs to (the one the scene stores)
    let top_instance = |e: Entity| {
        let mut cur = parents.get(&e).copied();
        let mut top = None;
        let mut guard = 0;
        while let Some(p) = cur {
            if instance_roots.contains(&p) && world.get::<&PrefabInstance>(p).is_ok() {
                top = Some(p);
            }
            guard += 1;
            if guard > 256 {
                break;
            }
            cur = parents.get(&p).copied();
        }
        top
    };
    // edits of entities inside instances since they were loaded → overrides of their instance
    let mut new_overrides: HashMap<Entity, Vec<PrefabOverride>> = HashMap::new();
    for (g, base) in world.query::<&PrefabBaseline>().iter() {
        let Some(root) = top_instance(g) else { continue };
        let Some(target) = prefab::path_in_instance(world, &parents, g, root) else { continue };
        let (components, scripts) = save_parts(&ctx, registry, g);
        let name = world.get::<&Name>(g).ok().map(|n| n.0.clone());
        let active = world.get::<&goat_runtime::Inactive>(g).is_err();
        let mut ov = PrefabOverride { target, ..Default::default() };
        if active != base.active {
            ov.active = Some(active);
        }
        if name.is_some() && name != base.name {
            ov.name = name;
        }
        if let Some(Value::Object(d)) = prefab::diff(&Value::Object(base.components.clone()), &Value::Object(components)) {
            ov.components = d;
        }
        if scripts != base.scripts {
            ov.scripts = Some(scripts);
        }
        if !ov.is_empty() {
            new_overrides.entry(root).or_default().push(ov);
        }
    }

    let mut out = SceneFile { version: SCENE_VERSION, entities: Vec::with_capacity(entities.len()), ..Default::default() };
    for &e in &entities {
        let mut entry = EntityEntry { id: ids[&e], ..Default::default() };
        entry.name = world.get::<&Name>(e).ok().map(|n| n.0.clone());
        entry.active = world.get::<&goat_runtime::Inactive>(e).is_err();
        entry.tag = world.get::<&goat_runtime::Tag>(e).ok().map(|t| t.0.clone());
        if let Some(&p) = parents.get(&e) {
            match ids.get(&p) {
                Some(&id) => entry.parent = Some(id),
                // added under an entity inside a prefab instance
                None => {
                    if let Some(root) = top_instance(p).or(Some(p)).filter(|r| ids.contains_key(r)) {
                        if let Some(path) = prefab::path_in_instance(world, &parents, p, root) {
                            entry.parent = Some(ids[&root]);
                            entry.parent_target = path;
                        }
                    }
                }
            }
        }
        entry.model = world.get::<&ModelInstance>(e).ok().map(|m| m.key.clone());
        if let Ok(pi) = world.get::<&PrefabInstance>(e) {
            entry.prefab = Some(pi.key.clone());
            entry.overrides = prefab::merge_overrides(&pi.overrides, new_overrides.remove(&e).unwrap_or_default());
        }
        let (components, scripts) = save_parts(&ctx, registry, e);
        entry.components = components;
        entry.scripts = scripts;
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
        let file = resolve_file(assets, key)?;
        spawn_file_depth(world, gfx, assets, registry, &file, key, None, 0, false)
    })?
}

/// Instantiates a prefab file: a new entity named after it (with [`PrefabInstance`]) whose
/// children are the prefab's entities. Returns that entity.
pub fn instantiate(ctx: &mut Context, key: &str, parent: Option<Entity>) -> Result<Entity, SceneError> {
    with_parts(ctx, |world, gfx, assets, registry| {
        let path = assets.io.canonical(key);
        let name = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".json").trim_end_matches(".prefab").to_string();
        let root = world.spawn((Name(name), Transform::default(), GlobalTransform::default(), PrefabInstance::new(key)));
        if let Some(p) = parent {
            let _ = world.insert_one(root, Parent(p));
        }
        spawn_prefab(world, gfx, assets, registry, key, &[], root, 1)?;
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

impl Default for EntityEntry {
    fn default() -> Self {
        EntityEntry { id: 0, name: None, active: true, tag: None, parent: None, parent_target: Vec::new(), model: None, prefab: None, overrides: Vec::new(), components: Map::new(), scripts: Vec::new() }
    }
}

fn yes() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}
