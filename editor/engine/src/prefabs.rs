//! Prefab instances in the editor (Unity's Overrides dropdown): which prefab an entity belongs
//! to and what differs from it, Revert (back to the prefab) and Apply (the instance's changes
//! written into the prefab file, then every instance reloaded).

use goat_assets::AssetServer;
use goat_gfx::GraphicsBackend;
use goat_runtime::{Entity, Parent};
use goat_scene::{PrefabInstance, PrefabOverride, SceneRegistry};
use serde_json::{json, Value};

use crate::core::{id_of, EditorCore};

impl EditorCore {
    /// The prefab instance root of `e` (itself, or the nearest ancestor that is one).
    pub fn prefab_root(&self, e: Entity) -> Option<Entity> {
        let w = &self.edit.world;
        let mut cur = Some(e);
        for _ in 0..256 {
            let x = cur?;
            if w.get::<&PrefabInstance>(x).is_ok() {
                return Some(x);
            }
            cur = w.get::<&Parent>(x).ok().map(|p| p.0);
        }
        None
    }

    /// The overrides of an instance as saving would write them (loaded ones + edits since).
    fn instance_overrides(&mut self, root: Entity) -> Option<(String, Vec<PrefabOverride>)> {
        let registry = self.edit.resources.take::<SceneRegistry>().unwrap_or_default();
        let file = self.edit.resources.get::<AssetServer>().map(|a| goat_scene::save_entities(&self.edit.world, a, &registry, &[root]));
        self.edit.resources.put_back(registry);
        let file = file?;
        let entry = file.entities.iter().find(|en| en.prefab.is_some() && en.parent.is_none())?;
        Some((entry.prefab.clone()?, entry.overrides.clone()))
    }

    /// `{ root, key, overrides: [{ target, name?, active?, components: [names], scripts }] }` for
    /// the prefab instance `e` belongs to, or null.
    pub fn prefab_info(&mut self, e: Entity) -> Value {
        let Some(root) = self.prefab_root(e) else { return Value::Null };
        let Some((key, overrides)) = self.instance_overrides(root) else { return Value::Null };
        let label = |path: &[u64]| goat_scene::find_in_instance(&self.edit.world, root, path).and_then(|x| self.edit.world.get::<&goat_runtime::Name>(x).ok().map(|n| n.0.clone())).unwrap_or_else(|| format!("{path:?}"));
        json!({
            "root": id_of(root),
            "key": key,
            "overrides": overrides.iter().map(|o| json!({
                "target": o.target,
                "object": label(&o.target),
                "name": o.name,
                "active": o.active,
                "components": o.components.keys().collect::<Vec<_>>(),
                "scripts": o.scripts.is_some(),
            })).collect::<Vec<_>>(),
        })
    }

    /// Revert All: the instance rebuilt from its prefab (its own name, transform and parent kept;
    /// overrides and objects added inside it dropped). Returns the new root.
    pub fn revert_prefab(&mut self, gfx: &mut dyn GraphicsBackend, e: Entity) -> Result<Entity, String> {
        let root = self.prefab_root(e).ok_or("not part of a prefab instance")?;
        let registry = self.edit.resources.take::<SceneRegistry>().unwrap_or_default();
        let file = self.edit.resources.get::<AssetServer>().map(|a| goat_scene::save_entities(&self.edit.world, a, &registry, &[root]));
        self.edit.resources.put_back(registry);
        let mut file = file.ok_or("no asset server")?;
        // keep the root entry only, without its overrides
        file.entities.retain(|en| en.prefab.is_some() && en.parent.is_none());
        for en in file.entities.iter_mut() {
            en.overrides.clear();
        }
        let parent = self.edit.world.get::<&Parent>(root).ok().map(|p| p.0);
        self.delete_entity(root);
        let registry = self.edit.resources.take::<SceneRegistry>().unwrap_or_default();
        let result = match self.edit.resources.get_mut::<AssetServer>() {
            Some(assets) => goat_scene::spawn_file(&mut self.edit.world, gfx, assets, &registry, &file, "revert", parent).map_err(|e| e.to_string()),
            None => Err("no asset server".into()),
        };
        self.edit.resources.put_back(registry);
        let roots = result?;
        let new_root = *roots.first().ok_or("nothing spawned")?;
        self.select(Some(new_root));
        Ok(new_root)
    }

    /// Apply All: the instance's overrides written into its prefab (variants keep them as their
    /// own overrides). Returns `{ key, text }`: the prefab file to save; the instance is then
    /// rebuilt from it and the scene reloaded, so every instance shows the new prefab.
    pub fn apply_prefab(&mut self, gfx: &mut dyn GraphicsBackend, e: Entity) -> Result<Value, String> {
        let root = self.prefab_root(e).ok_or("not part of a prefab instance")?;
        let (key, overrides) = self.instance_overrides(root).ok_or("no prefab")?;
        let text = self.io.read_string(&key).map_err(|e| e.to_string())?;
        let mut prefab = goat_scene::parse(&key, &text).map_err(|e| e.to_string())?;
        if prefab.base.is_some() {
            prefab.overrides = goat_scene::merge_overrides(&prefab.overrides, overrides);
        } else {
            goat_scene::apply_overrides(&mut prefab, &overrides, &key);
        }
        let out = serde_json::to_string_pretty(&prefab).map_err(|e| e.to_string())? + "\n";
        self.io.put(&key, out.clone().into_bytes());
        // the instance from the new prefab, then everything reloaded (other instances update too)
        self.revert_prefab(gfx, root)?;
        if let Some(scene) = self.scene.clone() {
            let saved = self.save_scene()?;
            self.io.put(&scene, saved.into_bytes());
            let keep_undo = std::mem::take(&mut self.undo);
            self.open_scene(gfx, &scene)?;
            self.undo = keep_undo;
        }
        Ok(json!({ "key": key, "text": out }))
    }
}
