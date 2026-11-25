//! Script natives of the SceneManager with several scenes (Unity / PiaEngine): Additive loads,
//! loadSceneAsync / unloadSceneAsync (operation ids), the loaded scene list, the active scene,
//! DontDestroyOnLoad, an object's scene, sceneLoaded / sceneUnloaded events.

use goat_runtime::Entity;
use goat_scene::scenes::{self, SceneOp, SceneOps};
use goat_scene::{LoadMode, LoadedScenes, DONT_DESTROY_ON_LOAD};
use serde_json::{json, Value};

use crate::{entity_json, Api, Extensions};

fn build_index(api: &Api, key: &str) -> i64 {
    api.ctx.resources.get::<goat_runtime::ProjectSettings>().and_then(|s| s.build_scenes().iter().position(|p| *p == key)).map(|i| i as i64).unwrap_or(-1)
}

fn scene_json(api: &Api, handle: u32, key: &str) -> Value {
    json!({ "handle": handle, "key": key, "buildIndex": build_index(api, key) })
}

/// `[nameOrIndex, mode]` → `{ key, op }` (loaded at the end of the frame).
fn load(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let which = match a.first() {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.clone(),
        _ => return Err("SceneManager.loadScene(nameOrIndex, mode)".into()),
    };
    let mode = if a.get(1).and_then(Value::as_str) == Some("additive") { LoadMode::Additive } else { LoadMode::Single };
    let (key, op) = goat_scene::request_scene(api.ctx, &which, mode).map_err(|e| e.to_string())?;
    Ok(json!({ "key": key, "op": op }))
}

/// `[handle | name]` → op id (unloaded at the end of the frame), or null when not loaded.
fn unload(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let scenes = api.ctx.resources.get_or_default::<LoadedScenes>();
    let handle = match a.first() {
        Some(Value::Number(n)) => n.as_u64().map(|h| h as u32),
        Some(Value::String(s)) => scenes.by_name(s).map(|x| x.handle),
        _ => None,
    };
    let Some(h) = handle.filter(|h| scenes.get(*h).is_some()) else { return Ok(Value::Null) };
    Ok(json!(api.ctx.resources.get_or_default::<SceneOps>().push(SceneOp::Unload(h))))
}

/// `[op]` → finished?
fn is_done(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let id = a.first().and_then(Value::as_u64).unwrap_or(0);
    Ok(Value::Bool(api.ctx.resources.get::<SceneOps>().is_some_and(|o| o.done.contains(&id))))
}

/// `{ scenes: [{ handle, key, buildIndex }], active, dontDestroyOnLoad: bool }`
fn list(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    let s = api.ctx.resources.get_or_default::<LoadedScenes>();
    let (list, active): (Vec<(u32, String)>, u32) = (s.list.iter().map(|x| (x.handle, x.key.clone())).collect(), s.active);
    let scenes: Vec<Value> = list.iter().map(|(h, k)| scene_json(api, *h, k)).collect();
    let ddol = api.ctx.world.query::<&goat_scene::SceneMember>().iter().any(|(_, m)| m.0 == DONT_DESTROY_ON_LOAD);
    Ok(json!({ "scenes": scenes, "active": active, "dontDestroyOnLoad": ddol }))
}

/// `[handle]` → applied?
fn set_active(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let h = a.first().and_then(Value::as_u64).unwrap_or(0) as u32;
    Ok(Value::Bool(scenes::set_active(api.ctx, h)))
}

/// `[handle]` → the scene's root objects.
fn roots(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let h = a.first().and_then(Value::as_u64).unwrap_or(0) as u32;
    let s = api.ctx.resources.get_or_default::<LoadedScenes>();
    let active = s.active;
    let w = &*api.ctx.world;
    let mut out: Vec<Entity> = w
        .query::<Option<&goat_scene::SceneMember>>()
        .without::<&goat_runtime::Parent>()
        .without::<&goat_runtime::DontSave>()
        .iter()
        .filter(|(_, m)| m.map(|m| m.0).unwrap_or(active) == h)
        .map(|(e, _)| e)
        .collect();
    out.sort_by_key(|e| e.id());
    Ok(Value::Array(out.into_iter().map(entity_json).collect()))
}

/// (entity) → the handle of its scene (`gameObject.scene`).
fn scene_of(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    let s = std::mem::take(api.ctx.resources.get_or_default::<LoadedScenes>());
    let h = scenes::scene_of(api.ctx.world, &s, e);
    let key = if h == DONT_DESTROY_ON_LOAD { "DontDestroyOnLoad".to_string() } else { s.get(h).map(|x| x.key.clone()).unwrap_or_default() };
    *api.ctx.resources.get_or_default::<LoadedScenes>() = s;
    Ok(scene_json(api, h, &key))
}

/// (entity): Unity's DontDestroyOnLoad (its root + children).
fn dont_destroy(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    if !api.ctx.world.contains(e) {
        return Err("dontDestroyOnLoad: no such object".into());
    }
    goat_scene::dont_destroy_on_load(api.ctx.world, e);
    Ok(Value::Null)
}

/// `{ loaded: [scene], unloaded: [scene], activeChanged }` since the last call (events).
fn events(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    let s = api.ctx.resources.get_or_default::<LoadedScenes>();
    let loaded: Vec<(u32, String)> = s.loaded_events.drain(..).filter_map(|h| s.list.iter().find(|x| x.handle == h).map(|x| (h, x.key.clone()))).collect();
    let unloaded: Vec<(u32, String)> = s.unloaded_events.drain(..).collect();
    let changed = std::mem::take(&mut s.active_changed);
    Ok(json!({
        "loaded": loaded.iter().map(|(h, k)| scene_json(api, *h, k)).collect::<Vec<_>>(),
        "unloaded": unloaded.iter().map(|(h, k)| scene_json(api, *h, k)).collect::<Vec<_>>(),
        "activeChanged": changed,
    }))
}

pub fn register(x: &mut Extensions) {
    x.add("SceneManager.loadScene", load)
        .add("SceneManager.unloadScene", unload)
        .add("SceneManager.isDone", is_done)
        .add("SceneManager.scenes", list)
        .add("SceneManager.setActiveScene", set_active)
        .add("SceneManager.roots", roots)
        .add("SceneManager.sceneOf", scene_of)
        .add("SceneManager.dontDestroyOnLoad", dont_destroy)
        .add("SceneManager.events", events);
}
