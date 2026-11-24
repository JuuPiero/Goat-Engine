//! Prefab overrides and variants (Unity's property modifications / prefab variants).
//!
//! - An instance entry (`"prefab": ...`) may carry `overrides`: changes to entities *inside* the
//!   prefab, each `{ "target": [ids...], "name"?, "components"?, "scripts"? }`. `target` is the
//!   id path into the prefab: `[3]` = entity 3 of the prefab file; `[3, 2]` = entity 2 of the
//!   prefab instantiated by entity 3 (nested prefabs). `components` is a JSON merge patch
//!   (RFC 7386: objects merge, other values replace, `null` removes a field / component);
//!   `scripts` replaces the behaviour list.
//! - A scene entity whose parent is inside a prefab instance stores `parent` = the instance
//!   entity and `parent_target` = the id path of the actual parent (Unity's added GameObjects).
//! - A **variant** is a prefab file with `"base": "<prefab>"`: the base's entities with the
//!   file's `overrides` applied, plus the file's own `entities` (ids must not collide with the
//!   base's; they may use base ids as parents).
//! - Saving keeps an instance's overrides and, when [`set_track_overrides`] is on (the editor),
//!   adds what changed since the instance was loaded: every entity spawned from a prefab
//!   remembers its loaded state ([`PrefabBaseline`]).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use goat_assets::AssetServer;
use goat_runtime::{Entity, Parent, World};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{parse, PrefabInstance, SceneError, SceneFile, MAX_PREFAB_DEPTH};

/// One change to an entity inside a prefab instance.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PrefabOverride {
    pub target: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// SetActive override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub components: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<Vec<Value>>,
}

impl PrefabOverride {
    pub(crate) fn is_empty(&self) -> bool {
        self.name.is_none() && self.active.is_none() && self.components.is_empty() && self.scripts.is_none()
    }
}

/// `existing` overrides followed by `new` ones, one entry per target (new edits win).
pub fn merge_overrides(existing: &[PrefabOverride], mut new: Vec<PrefabOverride>) -> Vec<PrefabOverride> {
    let mut out: Vec<PrefabOverride> = Vec::new();
    new.sort_by(|a, b| a.target.cmp(&b.target));
    for ov in existing.iter().cloned().chain(new) {
        match out.iter_mut().find(|o| o.target == ov.target) {
            Some(o) => {
                if ov.name.is_some() {
                    o.name = ov.name;
                }
                if ov.active.is_some() {
                    o.active = ov.active;
                }
                if !ov.components.is_empty() {
                    if let Value::Object(m) = compose(&Value::Object(std::mem::take(&mut o.components)), &Value::Object(ov.components)) {
                        o.components = m;
                    }
                }
                if ov.scripts.is_some() {
                    o.scripts = ov.scripts;
                }
            }
            None => out.push(ov),
        }
    }
    out.retain(|o| !o.is_empty());
    out
}

/// The id of an entity in the prefab file it was spawned from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrefabSource(pub u64);

/// What an entity spawned from a prefab looked like once loaded (only while
/// [`set_track_overrides`] is on): saving diffs against it.
#[derive(Clone, Debug, PartialEq)]
pub struct PrefabBaseline {
    pub name: Option<String>,
    pub active: bool,
    pub components: Map<String, Value>,
    pub scripts: Vec<Value>,
}

static TRACK: AtomicBool = AtomicBool::new(false);

/// Record [`PrefabBaseline`]s so that saving turns edits of prefab children into overrides
/// (the editor turns this on; games don't pay for it).
pub fn set_track_overrides(on: bool) {
    TRACK.store(on, Ordering::Relaxed);
}

pub fn track_overrides() -> bool {
    TRACK.load(Ordering::Relaxed)
}

// ------------------------------------------------------------------------ JSON merge patches

/// Applies an RFC 7386 merge patch.
pub fn merge_patch(target: &mut Value, patch: &Value) {
    match patch {
        Value::Object(p) => {
            if !target.is_object() {
                *target = Value::Object(Map::new());
            }
            let t = target.as_object_mut().unwrap();
            for (k, v) in p {
                if v.is_null() {
                    t.remove(k);
                } else {
                    merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        _ => *target = patch.clone(),
    }
}

/// The merge patch turning `from` into `to` (None when equal).
pub fn diff(from: &Value, to: &Value) -> Option<Value> {
    match (from, to) {
        (Value::Object(a), Value::Object(b)) => {
            let mut out = Map::new();
            for (k, vb) in b {
                match a.get(k) {
                    Some(va) => {
                        if let Some(d) = diff(va, vb) {
                            out.insert(k.clone(), d);
                        }
                    }
                    None => {
                        out.insert(k.clone(), vb.clone());
                    }
                }
            }
            for k in a.keys() {
                if !b.contains_key(k) {
                    out.insert(k.clone(), Value::Null);
                }
            }
            (!out.is_empty()).then_some(Value::Object(out))
        }
        _ => (from != to).then(|| to.clone()),
    }
}

/// One patch doing `first` then `second`.
pub fn compose(first: &Value, second: &Value) -> Value {
    match (first, second) {
        (Value::Object(a), Value::Object(b)) => {
            let mut out = a.clone();
            for (k, v) in b {
                let merged = match out.get(k) {
                    Some(prev) if prev.is_object() && v.is_object() => compose(prev, v),
                    _ => v.clone(),
                };
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        _ => second.clone(),
    }
}

// ------------------------------------------------------------------------ loading

/// Applies overrides to a (prefab) file's entries; overrides for nested prefabs are handed down
/// to the instance entry they go through.
pub fn apply_overrides(file: &mut SceneFile, overrides: &[PrefabOverride], source: &str) {
    for ov in overrides {
        let Some((&first, rest)) = ov.target.split_first() else { continue };
        let Some(entry) = file.entities.iter_mut().find(|e| e.id == first) else {
            goat_core::warn!("{source}: override of entity {first}, which the prefab no longer has (ignored)");
            continue;
        };
        if rest.is_empty() {
            if let Some(n) = &ov.name {
                entry.name = Some(n.clone());
            }
            if let Some(a) = ov.active {
                entry.active = a;
            }
            if !ov.components.is_empty() {
                let mut v = Value::Object(std::mem::take(&mut entry.components));
                merge_patch(&mut v, &Value::Object(ov.components.clone()));
                entry.components = match v {
                    Value::Object(m) => m,
                    _ => Map::new(),
                };
            }
            if let Some(s) = &ov.scripts {
                entry.scripts = s.clone();
            }
        } else if entry.prefab.is_some() {
            entry.overrides.push(PrefabOverride { target: rest.to_vec(), ..ov.clone() });
        } else {
            goat_core::warn!("{source}: override path {:?} goes through entity {first}, which isn't a prefab instance (ignored)", ov.target);
        }
    }
}

/// Reads a scene / prefab file, resolving variants (`base` chains) into plain entity lists.
pub fn resolve_file(assets: &AssetServer, key: &str) -> Result<SceneFile, SceneError> {
    resolve_depth(assets, key, 0)
}

fn resolve_depth(assets: &AssetServer, key: &str, depth: u32) -> Result<SceneFile, SceneError> {
    if depth > MAX_PREFAB_DEPTH {
        return Err(SceneError::Format(key.into(), "prefab variants nested too deep (a variant of itself?)".into()));
    }
    let text = assets.io.read_string(key)?;
    let file = parse(key, &text)?;
    resolve_variant(assets, key, file, depth)
}

/// Turns a parsed file with a `base` into the base's entities + overrides + its own entities.
pub(crate) fn resolve_variant(assets: &AssetServer, key: &str, mut file: SceneFile, depth: u32) -> Result<SceneFile, SceneError> {
    let Some(base_key) = file.base.take() else {
        file.overrides.clear();
        return Ok(file);
    };
    let mut base = resolve_depth(assets, &base_key, depth + 1)?;
    apply_overrides(&mut base, &file.overrides, key);
    for e in std::mem::take(&mut file.entities) {
        if base.entities.iter().any(|b| b.id == e.id) {
            return Err(SceneError::Format(key.into(), format!("entity id {} is already used by the base prefab {base_key}", e.id)));
        }
        base.entities.push(e);
    }
    Ok(SceneFile { version: file.version.max(base.version), entities: base.entities, ..Default::default() })
}

// ------------------------------------------------------------------------ id paths

/// The id path of `e` inside the instance `root` (None if `e` isn't spawned under it).
pub fn path_in_instance(world: &World, parents: &HashMap<Entity, Entity>, e: Entity, root: Entity) -> Option<Vec<u64>> {
    let mut path = vec![world.get::<&PrefabSource>(e).ok()?.0];
    let mut cur = parents.get(&e).copied();
    let mut guard = 0;
    while let Some(c) = cur {
        if c == root {
            path.reverse();
            return Some(path);
        }
        if world.get::<&PrefabInstance>(c).is_ok() {
            path.push(world.get::<&PrefabSource>(c).ok()?.0);
        }
        cur = parents.get(&c).copied();
        guard += 1;
        if guard > 256 {
            break;
        }
    }
    None
}

/// The entity at an id path inside the instance `root`.
pub fn find_in_instance(world: &World, root: Entity, path: &[u64]) -> Option<Entity> {
    let parents: HashMap<Entity, Entity> = world.query::<&Parent>().iter().map(|(e, p)| (e, p.0)).collect();
    let mut cur_root = root;
    let mut found = None;
    for &id in path {
        // candidates with this source id whose nearest instance ancestor is cur_root
        let mut hit = None;
        for (e, src) in world.query::<&PrefabSource>().iter() {
            if src.0 != id {
                continue;
            }
            let mut up = parents.get(&e).copied();
            while let Some(u) = up {
                if u == cur_root || world.get::<&PrefabInstance>(u).is_ok() {
                    break;
                }
                up = parents.get(&u).copied();
            }
            if up == Some(cur_root) {
                hit = Some(e);
                break;
            }
        }
        let e = hit?;
        found = Some(e);
        cur_root = e;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_patch_diff_and_compose_agree() {
        let base = json!({ "Transform": { "position": [0, 1, 0], "scale": [1, 1, 1] }, "Light": { "range": 5 } });
        let edited = json!({ "Transform": { "position": [2, 1, 0], "scale": [1, 1, 1] }, "Tag": { "x": 1 } });
        let d = diff(&base, &edited).unwrap();
        assert_eq!(d, json!({ "Transform": { "position": [2, 1, 0] }, "Tag": { "x": 1 }, "Light": null }));
        let mut v = base.clone();
        merge_patch(&mut v, &d);
        assert_eq!(v, edited);
        assert_eq!(diff(&edited, &edited), None);
        // composing two patches = applying both
        let second = json!({ "Transform": { "scale": [3, 3, 3] }, "Tag": null });
        let mut twice = base.clone();
        merge_patch(&mut twice, &d);
        merge_patch(&mut twice, &second);
        let mut once = base.clone();
        merge_patch(&mut once, &compose(&d, &second));
        assert_eq!(once, twice);
    }
}
