//! The Animation window's engine side (Unity's): previewing a clip on the edit world at a time
//! (the scene is put back as it was when the preview ends: transforms and the animated
//! components are saved first), and the objects under an animated root with their clip paths.

use std::collections::HashSet;

use goat_animation::clip::{sample_clip, AnimClip};
use goat_gfx::GraphicsBackend;
use goat_runtime::*;
use serde_json::{json, Value};

use crate::core::{id_of, EditorCore};

#[derive(Default)]
pub struct AnimPreview {
    root: Option<Entity>,
    transforms: Vec<(Entity, Transform)>,
    /// (entity, component) → its scene JSON before the preview.
    components: Vec<(Entity, String, Value)>,
}

impl AnimPreview {
    /// The transforms the preview started from (Play serializes the scene without the preview).
    pub(crate) fn transforms_snapshot(&self) -> Vec<(Entity, Transform)> {
        self.transforms.clone()
    }
}

impl EditorCore {
    fn subtree(&self, root: Entity) -> Vec<Entity> {
        let w = &self.edit.world;
        let children: Vec<(Entity, Entity)> = w.query::<&Parent>().iter().map(|(c, p)| (p.0, c)).collect();
        let mut out = vec![root];
        let mut i = 0;
        while i < out.len() {
            let x = out[i];
            out.extend(children.iter().filter(|(p, _)| *p == x).map(|(_, c)| *c));
            i += 1;
        }
        out
    }

    /// `[{ id, name, path }]`: the root ("" path) and every object under it.
    pub fn anim_paths(&self, root: Entity) -> Value {
        let w = &self.edit.world;
        let name = |e: Entity| w.get::<&Name>(e).map(|n| n.0.clone()).unwrap_or_default();
        let mut out = Vec::new();
        for e in self.subtree(root) {
            // path = names from below the root down to e
            let mut parts = Vec::new();
            let mut cur = e;
            while cur != root {
                parts.push(name(cur));
                match w.get::<&Parent>(cur) {
                    Ok(p) => cur = p.0,
                    Err(_) => break,
                }
            }
            parts.reverse();
            out.push(json!({ "id": id_of(e), "name": name(e), "path": parts.join("/") }));
        }
        Value::Array(out)
    }

    /// Shows `clip` at time `t` on `root`'s objects (the first call saves what it changes).
    pub fn anim_preview(&mut self, gfx: &mut dyn GraphicsBackend, root: Entity, clip_json: &str, t: f32) -> Result<(), String> {
        let clip = AnimClip::parse(clip_json)?;
        if self.anim.root != Some(root) {
            self.anim_preview_end(gfx);
            self.anim.root = Some(root);
            self.anim.transforms = self.subtree(root).into_iter().filter_map(|e| self.edit.world.get::<&Transform>(e).ok().map(|t| (e, *t))).collect();
        }
        // components animated by this clip: saved the first time they're touched
        let mut seen: HashSet<(Entity, String)> = self.anim.components.iter().map(|(e, c, _)| (*e, c.clone())).collect();
        for c in &clip.curves {
            let Some((comp, _)) = c.property.split_once('.') else { continue };
            if matches!(comp, "position" | "rotation" | "scale") {
                continue;
            }
            let Some(e) = goat_animation::clip::find_path(&self.edit.world, root, &c.path) else { continue };
            if seen.insert((e, comp.to_string())) {
                let value = self.components(e).as_array().and_then(|l| l.iter().find(|x| x["name"] == comp).map(|x| x["value"].clone()));
                if let Some(v) = value {
                    self.anim.components.push((e, comp.to_string(), v));
                }
            }
        }
        let samples = sample_clip(&self.edit.world, root, &clip, t);
        self.edit.with_context(gfx, |ctx| goat_animation::apply_component_samples(ctx, &samples));
        Ok(())
    }

    /// Puts back what the preview changed.
    pub fn anim_preview_end(&mut self, gfx: &mut dyn GraphicsBackend) {
        let prev = std::mem::take(&mut self.anim);
        for (e, t) in prev.transforms {
            if let Ok(mut cur) = self.edit.world.get::<&mut Transform>(e) {
                *cur = t;
            }
        }
        for (e, comp, v) in prev.components {
            let _ = self.set_component(gfx, e, &comp, &v);
        }
    }

    /// Recording: what the user changed during the preview stays (the scene keeps the values the
    /// preview started from; the clip gets the keys) — the saved state is the new baseline.
    pub fn anim_preview_root(&self) -> Option<Entity> {
        self.anim.root
    }
}

#[cfg(test)]
mod tests {
    use crate::core::EditorCore;
    use goat_assets::AssetIo;
    use goat_gfx::NullBackend;
    use goat_runtime::*;

    #[test]
    fn preview_samples_and_restores() {
        let io = AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets"));
        let mut gfx = NullBackend::new();
        let mut c = EditorCore::new(io);
        c.first_frame(&mut gfx);
        c.open_scene(&mut gfx, "scenes/showcase.scene.json").unwrap();
        let root = c.create_entity("Door", None);
        let arm = c.create_entity("Arm", Some(root));
        c.edit.world.get::<&mut Transform>(arm).unwrap().position.x = 5.0;
        let paths = c.anim_paths(root);
        assert_eq!(paths[1]["path"], "Arm");
        let clip = r#"{ "length": 1, "curves": [
            { "path": "", "property": "position.y", "interp": "linear", "keys": [ { "t": 0, "v": 0 }, { "t": 1, "v": 2 } ] },
            { "path": "Arm", "property": "rotation.z", "interp": "linear", "keys": [ { "t": 0, "v": 0 }, { "t": 1, "v": 90 } ] } ] }"#;
        c.anim_preview(&mut gfx, root, clip, 0.5).unwrap();
        assert!((c.edit.world.get::<&Transform>(root).unwrap().position.y - 1.0).abs() < 1e-4);
        let (z, _, _) = c.edit.world.get::<&Transform>(arm).unwrap().rotation.to_euler(goat_core::math::EulerRot::ZXY);
        assert!((z.to_degrees() - 45.0).abs() < 0.1, "{}", z.to_degrees());
        // saving / playing while previewing keeps the scene's own values
        let saved = c.save_scene().unwrap();
        let doc: serde_json::Value = serde_json::from_str(&saved).unwrap();
        let door = doc["entities"].as_array().unwrap().iter().find(|e| e["name"] == "Door").unwrap();
        let y = door["components"]["Transform"]["position"].get(1).and_then(|v| v.as_f64()).unwrap_or(0.0);
        assert_eq!(y, 0.0, "saved without the preview: {door}");
        assert!((c.edit.world.get::<&Transform>(root).unwrap().position.y - 1.0).abs() < 1e-4, "the preview stays on screen");
        c.anim_preview_end(&mut gfx);
        assert_eq!(c.edit.world.get::<&Transform>(root).unwrap().position.y, 0.0, "restored");
        assert_eq!(c.edit.world.get::<&Transform>(arm).unwrap().position.x, 5.0);
    }
}
