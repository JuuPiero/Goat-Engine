//! What the editor knows about scripts (Unity's serialized fields in the Inspector): a
//! TypeScript / JavaScript / C# host of its own builds a fresh instance of a script's class and
//! reports its fields and whether it is a Behaviour or a ScriptableObject. Results are cached
//! per script until a script or asset file changes.

use std::collections::HashMap;

use goat_gfx::GraphicsBackend;
use goat_script_js::JsHost;
use serde_json::{json, Value};

use crate::core::EditorCore;

#[derive(Default)]
pub struct ScriptInfo {
    host: Option<JsHost>,
    cache: HashMap<String, Value>,
}

/// Sources the JavaScript host runs.
pub fn is_script(key: &str) -> bool {
    [".ts", ".js", ".mjs", ".cs"].iter().any(|e| key.ends_with(e)) && !key.ends_with(".d.ts")
}

impl EditorCore {
    /// `{ kind: "behaviour" | "scriptableObject", fields: { name: default }, order: [names in
    /// declaration order] }`, or `{ error }`.
    pub fn script_info(&mut self, gfx: &mut dyn GraphicsBackend, src: &str) -> Value {
        if let Some(v) = self.scripts.cache.get(src) {
            return v.clone();
        }
        let v = if !is_script(src) {
            json!({ "error": format!("{src}: not a TypeScript / JavaScript / C# script") })
        } else if !self.io.exists(src) {
            json!({ "error": format!("{src}: missing") })
        } else {
            let io = self.io.clone();
            let (scripts, edit) = (&mut self.scripts, &mut self.edit);
            let host = scripts.host.get_or_insert_with(|| JsHost::with_io(io));
            edit.with_context(gfx, |ctx| {
                let mut api = goat_script_js::api_of(ctx);
                match host.is_scriptable_object(&mut api, src).and_then(|so| host.defaults(&mut api, src).map(|f| (so, f))) {
                    Ok((so, fields)) => {
                        let order: Vec<&String> = fields.iter().map(|(k, _)| k).collect();
                        json!({ "kind": if so { "scriptableObject" } else { "behaviour" }, "order": order, "fields": fields.iter().cloned().collect::<serde_json::Map<_, _>>() })
                    }
                    Err(e) => json!({ "error": e }),
                }
            })
        };
        self.scripts.cache.insert(src.to_string(), v.clone());
        v
    }

    /// A `.shader` file as the Inspector shows it: `{ name, kind, properties, state, fallback,
    /// customVertex, warnings, standard }`, or `{ errors: [{ line, message }] }`.
    pub fn shader_info(&self, key: &str) -> Value {
        let text = match self.io.read_string(key) {
            Ok(t) => t,
            Err(e) => return json!({ "errors": [{ "line": 0, "message": e.to_string() }] }),
        };
        match goat_shader::compile(&text) {
            Ok(s) => {
                let mut v = serde_json::to_value(&s).unwrap_or(Value::Null);
                if let Some(o) = v.as_object_mut() {
                    o.remove("wgsl");
                    o.remove("params");
                    // later passes: their state and 3DS status only
                    if let Some(Value::Array(list)) = o.get_mut("passes") {
                        for p in list.iter_mut() {
                            let keep = ["state", "kind", "picaError", "picaNotes"];
                            if let Some(po) = p.as_object_mut() {
                                po.retain(|k, _| keep.contains(&k.as_str()));
                            }
                        }
                    }
                }
                v
            }
            Err(errs) => json!({ "errors": errs }),
        }
    }

    /// A project file changed: script answers are recomputed (new code, new fields); materials
    /// and shaders of the edit world are updated in place.
    pub fn file_changed(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) {
        if is_script(key) || key.ends_with(".asset.json") {
            self.scripts.cache.clear();
            self.scripts.host = None;
        }
        if key.ends_with(".mat.json") || key.ends_with(".shader") {
            if let Some(mut assets) = self.edit.resources.take::<goat_assets::AssetServer>() {
                if key.ends_with(".shader") {
                    assets.reload_shader(gfx, key);
                } else if let Err(e) = assets.reload_material(gfx, key) {
                    goat_core::error!("{e}");
                }
                self.edit.resources.put_back(assets);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::EditorCore;
    use goat_assets::AssetIo;
    use goat_gfx::NullBackend;
    use serde_json::json;

    #[test]
    fn script_fields_and_kinds() {
        let io = AssetIo::new("/nonexistent-goat-editor-scripts");
        io.put("scripts/Data.ts", b"import { ScriptableObject, Vector3 } from 'goat';\nexport default class Data extends ScriptableObject { hp: number = 3; dir = new Vector3(0, 1, 0); _hidden = 1; }\n".to_vec());
        io.put("scripts/Mover.js", b"import { Behaviour } from 'goat';\nexport default class Mover extends Behaviour { speed = 2; target = null; label = 'go'; }\n".to_vec());
        let mut gfx = NullBackend::new();
        let mut c = EditorCore::new(io.clone());
        c.first_frame(&mut gfx);
        assert_eq!(c.script_info(&mut gfx, "scripts/Data.ts"), json!({ "kind": "scriptableObject", "order": ["hp", "dir"], "fields": { "hp": 3, "dir": [0, 1, 0] } }));
        assert_eq!(c.script_info(&mut gfx, "scripts/Mover.js"), json!({ "kind": "behaviour", "order": ["speed", "target", "label"], "fields": { "speed": 2, "target": null, "label": "go" } }));
        assert!(c.script_info(&mut gfx, "scripts/Nope.ts")["error"].is_string());
        // edits are seen once the file changes
        io.put("scripts/Mover.js", b"import { Behaviour } from 'goat';\nexport default class Mover extends Behaviour { speed = 5; }\n".to_vec());
        assert_eq!(c.script_info(&mut gfx, "scripts/Mover.js")["fields"], json!({ "speed": 2, "target": null, "label": "go" }), "cached");
        c.file_changed(&mut gfx, "scripts/Mover.js");
        assert_eq!(c.script_info(&mut gfx, "scripts/Mover.js")["fields"], json!({ "speed": 5 }));
    }
}
