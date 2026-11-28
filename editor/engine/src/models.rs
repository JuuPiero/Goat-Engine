//! Model Import Settings in the editor (Unity's ModelImporter inspector): what the inspector shows
//! about a model, re-importing instances when the settings or the file change, and Extract
//! Textures / Materials.

use goat_assets::{AssetServer, ModelInstance, ModelNodes, ModelSkeleton};
use goat_gfx::GraphicsBackend;
use goat_runtime::{Entity, Parent};
use serde_json::{json, Value};

use crate::core::EditorCore;

/// The model a changed file belongs to: the model itself, or its `.import.json`.
pub fn model_of_changed_file(key: &str) -> Option<&str> {
    let model = key.strip_suffix(".import.json").unwrap_or(key);
    goat_assets::is_model_key(model).then_some(model)
}

impl EditorCore {
    /// `model_info` of the AssetServer (nodes, meshes, materials, takes, clips, file unit, size).
    pub fn model_info(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Value {
        match self.edit.resources.get_mut::<AssetServer>() {
            Some(a) => a.model_info(gfx, key).unwrap_or_else(|e| json!({ "error": e.to_string() })),
            None => json!({ "error": "no asset server" }),
        }
    }

    /// Re-imports a model: forgets the loaded data and rebuilds every instance of it in the edited
    /// scene (their root keeps its own components, name and transform). Returns how many.
    pub fn reload_model(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> usize {
        let Some(mut assets) = self.edit.resources.take::<AssetServer>() else { return 0 };
        let file = assets.io.canonical(key);
        assets.forget_model(&file);
        let world = &mut self.edit.world;
        let roots: Vec<Entity> = world.query::<&ModelInstance>().iter().filter(|(_, m)| assets.io.canonical(&m.key) == file).map(|(e, _)| e).collect();
        for &root in &roots {
            let generated: Vec<Entity> = world.get::<&ModelNodes>(root).map(|n| n.0.iter().flatten().copied().collect()).unwrap_or_default();
            let set: std::collections::HashSet<Entity> = generated.iter().copied().collect();
            // things the user parented under a model node move up to the instance root
            let orphans: Vec<Entity> = world.query::<&Parent>().iter().filter(|(e, p)| set.contains(&p.0) && !set.contains(e)).map(|(e, _)| e).collect();
            for o in orphans {
                let _ = world.insert_one(o, Parent(root));
            }
            for e in generated {
                let _ = world.despawn(e);
            }
            let _ = world.remove_one::<ModelNodes>(root);
            let _ = world.remove_one::<ModelSkeleton>(root);
            let _ = world.remove_one::<ModelInstance>(root);
            if let Err(e) = assets.spawn_model_into(world, gfx, &file, root) {
                goat_core::error!("re-import {file}: {e}");
            }
        }
        self.edit.resources.put_back(assets);
        roots.len()
    }

    /// Extract Textures / Materials: the files to write (`{ key, data: base64 }`) and the remap.
    pub fn extract_model_materials(&mut self, key: &str, textures_only: bool) -> Value {
        use base64::Engine;
        let Some(a) = self.edit.resources.get::<AssetServer>() else { return json!({ "error": "no asset server" }) };
        match a.extract_model_materials(key, textures_only) {
            Ok((files, remap)) => json!({
                "files": files.iter().map(|(k, b)| json!({ "key": k, "data": base64::engine::general_purpose::STANDARD.encode(b) })).collect::<Vec<_>>(),
                "remap": remap,
            }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::EditorCore;
    use goat_assets::AssetIo;
    use goat_gfx::NullBackend;

    /// Changing a model's import settings rebuilds its instances (Optimize Game Objects drops the
    /// bone entities; the root keeps its components).
    #[test]
    fn import_settings_reimport_instances() {
        let io = AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets"));
        let mut gfx = NullBackend::new();
        let mut c = EditorCore::new(io.clone());
        c.first_frame(&mut gfx);
        let root = c.instantiate(&mut gfx, "models/tentacle.glb", goat_core::math::Vec3::ZERO).unwrap();
        let names = |c: &EditorCore| c.edit.world.query::<&goat_runtime::Name>().iter().map(|(_, n)| n.0.clone()).collect::<Vec<_>>();
        assert!(names(&c).iter().any(|n| n == "Bone1"));
        let info = c.model_info(&mut gfx, "models/tentacle.glb");
        assert_eq!(info["bones"], 4);
        assert!(info["takes"].as_array().unwrap().iter().any(|t| t["name"] == "Sway"), "{info}");
        io.put("models/tentacle.glb.import.json", br#"{ "optimize_game_objects": true }"#.to_vec());
        c.file_changed(&mut gfx, "models/tentacle.glb.import.json");
        assert!(!names(&c).iter().any(|n| n == "Bone1"), "re-imported without bone entities");
        assert!(c.edit.world.get::<&goat_assets::ModelSkeleton>(root).is_ok());
        let ex = c.extract_model_materials("models/tentacle.glb", false);
        assert!(ex["files"].as_array().is_some_and(|f| f.iter().any(|f| f["key"].as_str().unwrap().ends_with(".mat.json"))), "{ex}");
    }
}
