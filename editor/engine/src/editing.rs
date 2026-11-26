//! Editing operations of the editor engine: Undo / Redo (scene snapshots, like PiaEngine's
//! `recordUndo`), multi-selection, copy / paste / duplicate of entity trees, and the component
//! metadata the Inspector draws from.

use goat_assets::AssetServer;
use goat_gfx::GraphicsBackend;
use goat_runtime::*;
use goat_scene::SceneRegistry;
use serde_json::Value;

use crate::core::EditorCore;

/// One Undo step: the scene as it was before the action, and what was selected.
pub struct UndoStep {
    pub label: String,
    scene: String,
    /// Positions of the selected entities in the saved order (`goat_scene::saved_entities`).
    selection: Vec<usize>,
}

const MAX_UNDO: usize = 100;

impl EditorCore {
    // ------------------------------------------------------------------ selection
    /// Selects several entities (the last one is active: Inspector, gizmo).
    pub fn select_many(&mut self, list: Vec<Entity>) {
        let world = &self.play.as_ref().unwrap_or(&self.edit).world;
        self.selection = list.into_iter().filter(|e| world.contains(*e)).collect();
        self.selection.dedup();
        self.selected = self.selection.last().copied();
    }

    /// The selection without entities whose ancestor is selected too (what moves / copies).
    pub fn selection_roots(&self) -> Vec<Entity> {
        let w = crate::core::view_world!(self);
        self.selection
            .iter()
            .copied()
            .filter(|&e| {
                let mut cur = w.get::<&Parent>(e).ok().map(|p| p.0);
                for _ in 0..256 {
                    let Some(p) = cur else { return true };
                    if self.selection.contains(&p) {
                        return false;
                    }
                    cur = w.get::<&Parent>(p).ok().map(|p| p.0);
                }
                true
            })
            .collect()
    }

    // ------------------------------------------------------------------ undo
    fn snapshot(&mut self) -> Option<(String, Vec<usize>)> {
        let scene = self.save_scene().ok()?;
        let order = goat_scene::saved_entities(&self.edit.world);
        let selection = self.selection.iter().filter_map(|e| order.iter().position(|x| x == e)).collect();
        Some((scene, selection))
    }

    /// Call before changing the scene: the step Undo goes back to.
    pub fn record_undo(&mut self, label: &str) {
        if let Some((scene, selection)) = self.snapshot() {
            // the same action repeated on the same state (a drag sending many edits) is one step
            if self.undo.last().is_some_and(|s| s.scene == scene && s.label == label) {
                return;
            }
            self.undo.push(UndoStep { label: label.to_string(), scene, selection });
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    /// Goes back one step; returns its label.
    pub fn undo(&mut self, gfx: &mut dyn GraphicsBackend) -> Option<String> {
        let step = self.undo.pop()?;
        let (scene, selection) = self.snapshot()?;
        self.redo.push(UndoStep { label: step.label.clone(), scene, selection });
        self.restore(gfx, &step);
        Some(step.label)
    }

    pub fn redo(&mut self, gfx: &mut dyn GraphicsBackend) -> Option<String> {
        let step = self.redo.pop()?;
        let (scene, selection) = self.snapshot()?;
        self.undo.push(UndoStep { label: step.label.clone(), scene, selection });
        self.restore(gfx, &step);
        Some(step.label)
    }

    /// Forgets the history (another scene opened).
    pub fn clear_undo(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    fn restore(&mut self, gfx: &mut dyn GraphicsBackend, step: &UndoStep) {
        let Ok(file) = goat_scene::parse("undo", &step.scene) else { return };
        self.clear_scene_entities();
        let registry = self.edit.resources.take::<SceneRegistry>().unwrap_or_default();
        if let Some(assets) = self.edit.resources.get_mut::<AssetServer>() {
            if let Err(e) = goat_scene::spawn_file(&mut self.edit.world, gfx, assets, &registry, &file, self.scene.as_deref().unwrap_or("scene"), None) {
                goat_core::error!("undo: {e}");
            }
        }
        self.edit.resources.put_back(registry);
        let order = goat_scene::saved_entities(&self.edit.world);
        let sel = step.selection.iter().filter_map(|&i| order.get(i).copied()).collect();
        self.select_many(sel);
    }

    // ------------------------------------------------------------------ copy / paste
    /// The selection's trees as scene JSON (the editor's clipboard).
    pub fn copy_selection(&mut self) -> Option<String> {
        let roots = self.selection_roots();
        if roots.is_empty() {
            return None;
        }
        let registry = self.edit.resources.take::<SceneRegistry>().unwrap_or_default();
        let out = self.edit.resources.get::<AssetServer>().map(|a| goat_scene::save_entities(&self.edit.world, a, &registry, &roots));
        self.edit.resources.put_back(registry);
        out.and_then(|f| serde_json::to_string(&f).ok())
    }

    /// Spawns copied trees under `parent`; selects and returns their roots.
    pub fn paste(&mut self, gfx: &mut dyn GraphicsBackend, json: &str, parent: Option<Entity>) -> Result<Vec<Entity>, String> {
        let file = goat_scene::parse("clipboard", json).map_err(|e| e.to_string())?;
        let registry = self.edit.resources.take::<SceneRegistry>().unwrap_or_default();
        let result = match self.edit.resources.get_mut::<AssetServer>() {
            Some(assets) => goat_scene::spawn_file(&mut self.edit.world, gfx, assets, &registry, &file, "clipboard", parent).map_err(|e| e.to_string()),
            None => Err("no asset server".into()),
        };
        self.edit.resources.put_back(registry);
        let roots = result?;
        self.select_many(roots.clone());
        Ok(roots)
    }

    /// Ctrl+D: copies of the selection next to the originals, named "Name (1)", "Name (2)"…
    pub fn duplicate_selection(&mut self, gfx: &mut dyn GraphicsBackend) -> Result<Vec<Entity>, String> {
        let roots = self.selection_roots();
        let mut out = Vec::new();
        for r in roots {
            self.select_many(vec![r]);
            let Some(json) = self.copy_selection() else { continue };
            let parent = self.edit.world.get::<&Parent>(r).ok().map(|p| p.0);
            let copies = self.paste(gfx, &json, parent)?;
            for c in &copies {
                let name = self.edit.world.get::<&Name>(*c).map(|n| n.0.clone()).unwrap_or_default();
                let unique = self.unique_sibling_name(&name, parent, *c);
                let _ = self.edit.world.insert_one(*c, Name(unique));
            }
            out.extend(copies);
        }
        self.select_many(out.clone());
        Ok(out)
    }

    /// `Cube` → `Cube (1)`, `Cube (1)` → `Cube (2)`… unused among the siblings.
    fn unique_sibling_name(&self, name: &str, parent: Option<Entity>, me: Entity) -> String {
        let w = &self.edit.world;
        let base = match name.rfind(" (") {
            Some(i) if name.ends_with(')') && name[i + 2..name.len() - 1].parse::<u32>().is_ok() => &name[..i],
            _ => name,
        };
        let taken: Vec<String> = w
            .query::<(&Name, Option<&Parent>)>()
            .without::<&DontSave>()
            .iter()
            .filter(|(e, (_, p))| *e != me && p.map(|p| p.0) == parent)
            .map(|(_, (n, _))| n.0.clone())
            .collect();
        (1..).map(|i| format!("{base} ({i})")).find(|n| !taken.contains(n)).unwrap()
    }

    // ------------------------------------------------------------------ component metadata
    /// `{ components: { name: info }, behaviours: [names] }` for the Inspector.
    pub fn component_info(&self) -> Value {
        let Some(r) = self.edit.resources.get::<SceneRegistry>() else { return Value::Null };
        let mut comps = serde_json::Map::new();
        for c in r.components() {
            let info = r.info(c.name).map(|i| serde_json::to_value(i).unwrap_or_default()).unwrap_or_else(|| serde_json::json!({ "menu": format!("Miscellaneous/{}", c.name) }));
            comps.insert(c.name.to_string(), info);
        }
        serde_json::json!({ "components": comps, "behaviours": r.behaviour_names() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goat_assets::AssetIo;
    use goat_gfx::NullBackend;

    fn core() -> (EditorCore, NullBackend) {
        let io = AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets"));
        let mut gfx = NullBackend::new();
        let mut c = EditorCore::new(io);
        c.first_frame(&mut gfx);
        c.open_scene(&mut gfx, "scenes/showcase.scene.json").unwrap();
        (c, gfx)
    }

    fn named(c: &EditorCore, n: &str) -> Option<Entity> {
        c.edit.world.query::<&Name>().iter().find(|(_, x)| x.0 == n).map(|(e, _)| e)
    }

    #[test]
    fn undo_redo_restore_scene_and_selection() {
        let (mut c, mut gfx) = core();
        let crate_e = named(&c, "Spinning Crate").unwrap();
        c.select_many(vec![crate_e]);
        c.record_undo("Rename");
        c.rename(crate_e, "Box");
        c.record_undo("Delete");
        c.delete_entity(crate_e);
        assert!(named(&c, "Box").is_none());
        assert_eq!(c.undo(&mut gfx).as_deref(), Some("Delete"));
        let b = named(&c, "Box").expect("undo brought it back");
        assert_eq!(c.selected, Some(b), "selection restored");
        assert_eq!(c.undo(&mut gfx).as_deref(), Some("Rename"));
        assert!(named(&c, "Spinning Crate").is_some());
        assert_eq!(c.redo(&mut gfx).as_deref(), Some("Rename"));
        assert!(named(&c, "Box").is_some());
        assert_eq!(c.redo_label(), Some("Delete"));
        // a new action drops the redo history
        c.record_undo("Create");
        assert_eq!(c.redo_label(), None);
    }

    #[test]
    fn copy_paste_and_duplicate_trees() {
        let (mut c, mut gfx) = core();
        let n = c.hierarchy().len();
        let shrine = named(&c, "Shrine").unwrap();
        c.select_many(vec![shrine]);
        let json = c.copy_selection().unwrap();
        let pasted = c.paste(&mut gfx, &json, None).unwrap();
        assert_eq!(pasted.len(), 1);
        assert!(c.hierarchy().len() > n + 1, "the prefab instance came with its children");
        let dup = c.duplicate_selection(&mut gfx).unwrap();
        let name = c.edit.world.get::<&Name>(dup[0]).unwrap().0.clone();
        assert_eq!(name, "Shrine (1)");
        // the metadata the Inspector needs
        let info = c.component_info();
        assert_eq!(info["components"]["MeshRenderer"]["menu"], "Mesh/Mesh Renderer");
        assert_eq!(info["components"]["Collider"]["fields"]["shape"]["kind"], "variant");
        assert!(info["behaviours"].as_array().unwrap().iter().any(|b| b == "Spinner"));
    }

    #[test]
    fn bake_button_and_navmesh_preview() {
        let (mut c, mut gfx) = core();
        c.open_scene(&mut gfx, "scenes/navigation.scene.json").unwrap();
        let s = named(&c, "NavMesh Surface").unwrap();
        let msg = c.component_action(&mut gfx, s, "NavMeshSurface", "Bake").unwrap();
        assert!(msg.contains("walkable cells"), "{msg}");
        assert!(!c.edit.world.get::<&goat_navigation::NavMeshSurface>(s).unwrap().baked_data.is_empty());
        let info = c.component_info();
        assert_eq!(info["components"]["NavMeshSurface"]["actions"], serde_json::json!(["Bake", "Clear"]));
        // selecting the surface shows its navmesh on the editor layer
        c.select_many(vec![s]);
        c.frame(&mut gfx, 0.016);
        let shown = c.edit.world.query::<(&Name, &MeshRenderer)>().iter().filter(|(_, (n, r))| n.0 == "NavMesh Preview" && r.visible).count();
        assert!(shown >= 1);
        c.select_many(vec![]);
        c.frame(&mut gfx, 0.016);
        assert_eq!(c.edit.world.query::<(&Name, &MeshRenderer)>().iter().filter(|(_, (n, r))| n.0 == "NavMesh Preview" && r.visible).count(), 0);
        // saved with the scene, Clear removes it
        assert!(c.save_scene().unwrap().contains("baked_data"));
        c.component_action(&mut gfx, s, "NavMeshSurface", "Clear").unwrap();
        assert!(c.edit.world.get::<&goat_navigation::NavMeshSurface>(s).unwrap().baked_data.is_empty());
    }

    #[test]
    fn gizmos_for_selected_components() {
        let (mut c, _gfx) = core();
        assert!(c.gizmo_lines().mesh.vertices.is_empty(), "nothing selected: no gizmo");
        for name in ["Main Camera", "Sun", "Lamp"] {
            let e = named(&c, name).unwrap();
            c.select_many(vec![e]);
            assert!(!c.gizmo_lines().mesh.vertices.is_empty(), "{name} has a gizmo");
        }
    }
    #[test]
    fn pause_step_and_profiler() {
        let (mut c, mut gfx) = core();
        c.play();
        for _ in 0..5 {
            c.frame(&mut gfx, 1.0 / 60.0);
        }
        let t = |c: &EditorCore| c.play.as_ref().unwrap().time.frame;
        let f0 = t(&c);
        c.paused = true;
        c.frame(&mut gfx, 1.0 / 60.0);
        c.frame(&mut gfx, 1.0 / 60.0);
        assert_eq!(t(&c), f0, "paused: no frame runs");
        c.step_once = true;
        c.frame(&mut gfx, 1.0 / 60.0);
        assert_eq!(t(&c), f0 + 1, "step: exactly one");
        let p = c.profiler_json();
        assert!(p["sections"].as_array().unwrap().iter().any(|s| s[0] == "Behaviours" || s[0] == "Render"));
        assert!(p["history"].as_array().unwrap().len() >= 5);
        c.stop();
        assert!(!c.paused);
    }
    /// The typed component classes of the TypeScript API (Unity's getComponent(Light),
    /// lineRenderer.setPosition, particleSystem.emit...) against the game's real plugins (Play).
    #[test]
    fn typed_component_api_in_play_mode() {
        let (mut c, mut gfx) = core();
        c.io.put("scripts/ApiTest.ts", r####"import { Behaviour, GameObject, Vector3, Vector2, Color, Light, LineRenderer, ParticleSystem, Rigidbody, BoxCollider, SphereCollider, Collider, TrailRenderer, Text, Image, Slider, Camera, MeshRenderer, MeshFilter, Component, ForceMode, SpriteRenderer, TextMesh } from "goat";

class Helper extends Behaviour {}
export { Helper };

export default class ApiTest extends Behaviour {
  log: any[] = [];
  pinged = 0;
  ping(n: number) { this.pinged = n; }
  frames = 0;
  start() {
    const L = this.log;
    const lamp = GameObject.find("Lamp")!;
    const light = lamp.getComponent(Light)!;
    L.push(light.type, light.range);
    light.intensity = 2.5;
    light.color = new Color(1, 0, 0, 1);
    L.push(lamp.getComponent(Light)!.intensity, lamp.getComponent(Light)!.color.r);

    const line = GameObject.create("Line").addComponent(LineRenderer)!;
    line.positionCount = 3;
    line.setPosition(2, new Vector3(1, 2, 3));
    line.startWidth = 0.5;
    L.push(line.positionCount, line.getPosition(2).y, line.startWidth);

    const crate = GameObject.find("Crate")!;
    L.push(!!crate.getComponent(BoxCollider), !!crate.getComponent(SphereCollider), !!crate.getComponent(Collider));
    L.push(crate.getComponent(BoxCollider)!.size.x);
    const rb = crate.getComponent(Rigidbody)!;
    rb.velocity = new Vector3(0, 3, 0);
    rb.addForce(new Vector3(0, 1, 0), ForceMode.Impulse);
    L.push(rb.isKinematic, rb.useGravity);
    L.push(crate.getComponent(MeshFilter)!.mesh, (crate.getComponent(MeshRenderer) as any).sharedMaterial);

    const parent = GameObject.find("Parent")!;
    L.push(parent.getComponentInChildren(Light) ? "childLight" : "none");
    L.push(GameObject.findObjectsByType(Light).length);

    const ps = GameObject.find("Sparks")!.getComponent(ParticleSystem)!;
    ps.stop(true, "StopEmittingAndClear");
    L.push(ps.isPlaying, ps.particleCount);
    ps.emit(7);
    L.push(ps.particleCount);
    ps.startSpeed = 4;
    L.push(ps.startSpeed);

    L.push(lamp.getComponents(Component).map((c: any) => c.constructor.name).sort().join("+"));
    const extra = lamp.addComponent(TrailRenderer)!;
    L.push(!!lamp.getComponent(TrailRenderer));
    GameObject.destroy(extra);
    L.push(!!lamp.getComponent(TrailRenderer), lamp.exists);

    const helper = this.getComponent(Helper);
    L.push(helper === null ? "noHelper" : "helper");
    L.push(this.getComponent(ApiTest) === this);
    L.push(Camera.main ? "cam" : "nocam", !!GameObject.find("Main Camera")!.getComponent(Camera));
    const label = GameObject.find("Label")!.getComponent(TextMesh)!;
    label.text = "changed";
    L.push(GameObject.find("Label")!.getComponent(TextMesh)!.text);
    const copy = GameObject.instantiate(crate);
    L.push(copy.name, copy.getComponent(BoxCollider) ? "box" : "nobox", copy.transform.position.y);
    const lightCopy = GameObject.instantiate(lamp.getComponent(Light)!) as any;
    L.push(lightCopy.type);
    const prim = GameObject.createPrimitive("Sphere");
    L.push(prim.getComponent(MeshFilter)!.mesh, !!prim.getComponent(SphereCollider));
    this.gameObject.sendMessage("ping", 5);
    L.push(this.pinged);
  }
  update() {
    if (++this.frames === 3) {
      this.log.push(GameObject.find("Sparks")!.getComponent(ParticleSystem)!.particleCount > 0);
      this.gameObject.name = "RESULT " + JSON.stringify(this.log);
    }
  }
}
"####.as_bytes().to_vec());
        c.io.put(
            "scenes/api_test.scene.json",
            br##"{ "version": 1, "entities": [
              { "id": 1, "name": "Main Camera", "components": { "Transform": { "position": [0, 2, 8] }, "Camera": {} } },
              { "id": 2, "name": "Lamp", "components": { "Transform": {}, "PointLight": { "color": "#ffffff", "intensity": 1, "range": 7 } } },
              { "id": 3, "name": "Crate", "components": { "Transform": { "position": [0, 1, 0] }, "MeshRenderer": { "mesh": "builtin:cube", "material": "builtin:default" }, "Collider": { "shape": { "box": [0.5, 0.5, 0.5] } }, "RigidBody": {} } },
              { "id": 4, "name": "Parent", "components": { "Transform": {} } },
              { "id": 5, "name": "Child", "parent": 4, "components": { "Transform": {}, "SpotLight": { "color": "#ffffff", "intensity": 1, "range": 5, "spot_angle": 30 } } },
              { "id": 6, "name": "Sparks", "components": { "Transform": {}, "ParticleSystem": { "rate": 30 } } },
              { "id": 7, "name": "Label", "components": { "Transform": {}, "Text": { "text": "hello", "font": "" } } },
              { "id": 8, "name": "Tester", "components": { "Transform": {}, "Script": { "src": "scripts/ApiTest.ts" } } }
            ] }"##
            .to_vec(),
        );
        c.open_scene(&mut gfx, "scenes/api_test.scene.json").unwrap();
        c.play();
        for _ in 0..8 {
            c.frame(&mut gfx, 1.0 / 60.0);
        }
        let names: Vec<String> = c.play.as_ref().unwrap().world.query::<&goat_runtime::Name>().iter().map(|(_, n)| n.0.clone()).collect();
        let result = names.iter().find(|n| n.starts_with("RESULT ")).unwrap_or_else(|| panic!("no result: {names:?}"));
        assert_eq!(
            &result[7..],
            r#"["Point",7,2.5,1,3,2,0.5,true,false,true,1,false,true,"builtin:cube","builtin:default","childLight",2,false,0,7,4,"Light+Transform",true,false,true,"noHelper",true,"cam",true,"changed","Crate(Clone)","box",1,"Point","builtin:sphere",true,5,true]"#
        );
    }
    #[test]
    fn every_component_can_be_added_from_its_defaults() {
        let (mut c, mut gfx) = core();
        let info = c.component_info();
        for (name, i) in info["components"].as_object().unwrap() {
            if name == "Script" {
                continue; // needs a script file
            }
            let e = c.create_entity("x", None);
            let v = i.get("defaults").cloned().unwrap_or(serde_json::json!({}));
            c.set_component(&mut gfx, e, name, &v).unwrap_or_else(|err| panic!("{name}: {err}"));
        }
    }
}
