//! Which components / behaviours scenes can store, and how ([`SceneRegistry`]).
//!
//! Subsystem crates register their components (`goat-sprites` → `SpriteRenderer`), games register
//! their behaviours: `registry.behaviour::<Spinner>("Spinner")` for any `Behaviour + Serialize +
//! Deserialize`.

use std::any::TypeId;
use std::collections::HashMap;

use goat_assets::AssetServer;
use goat_gfx::GraphicsBackend;
use goat_runtime::{Behaviour, Entity, World};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

/// What loaders get: the world being filled, the GPU and the asset server (asset keys → handles).
pub struct LoadContext<'a> {
    pub world: &'a mut World,
    pub gfx: &'a mut dyn GraphicsBackend,
    pub assets: &'a mut AssetServer,
    /// Key of the scene / prefab being loaded (error messages).
    pub source: &'a str,
}

/// What savers get.
pub struct SaveContext<'a> {
    pub world: &'a World,
    pub assets: &'a AssetServer,
}

pub type SaveFn = fn(&SaveContext, Entity) -> Option<Value>;
pub type LoadFn = fn(&mut LoadContext, Entity, &Value) -> Result<(), String>;
pub type RemoveFn = fn(&mut World, Entity);

/// Removes a component of type `T` (codecs' `remove`).
pub fn remove_typed<T: hecs::Component>(world: &mut World, e: Entity) {
    let _ = world.remove_one::<T>(e);
}

/// Saves a component through its serde implementation.
pub fn save_serde<T: hecs::Component + Serialize>(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let c = ctx.world.get::<&T>(e).ok()?;
    serde_json::to_value(&*c).ok()
}

/// Loads (inserts / replaces) a component through its serde implementation.
pub fn load_serde<T: hecs::Component + DeserializeOwned>(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let c: T = match serde_json::from_value(v.clone()) {
        Ok(c) => c,
        // unit structs (markers like AudioListener) are `null`; the editor's "defaults" is `{}`
        Err(_) if v.as_object().is_some_and(|o| o.is_empty()) => serde_json::from_value(Value::Null).map_err(|e| e.to_string())?,
        Err(e) => return Err(e.to_string()),
    };
    ctx.world.insert_one(e, c).map_err(|_| "entity vanished".to_string())
}

/// A codec for any serde component: `registry.component(goat_scene::serde_codec::<RigidBody>("RigidBody"))`.
pub fn serde_codec<T: hecs::Component + Serialize + DeserializeOwned>(name: &'static str) -> ComponentCodec {
    ComponentCodec { name, save: save_serde::<T>, load: load_serde::<T>, remove: remove_typed::<T> }
}

#[derive(Clone, Copy)]
pub struct ComponentCodec {
    pub name: &'static str,
    pub save: SaveFn,
    pub load: LoadFn,
    /// Takes the component off an entity (the editor's Remove Component).
    pub remove: RemoveFn,
}

/// How the editor shows one field of a component's scene JSON (dotted paths for nested
/// fields: `projection.perspective.fov`).
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct FieldHint {
    /// "enum" (pick from `options`), "variant" (externally tagged object: one key out of
    /// `variants`, each with its default payload), "asset" (`asset` = texture / material / mesh /
    /// model / prefab / audio / font / script / controller / scene / any), "color" (a number
    /// array as a color), "entity" (a name / id of another entity), "mask" (32-bit layer mask),
    /// "layer" (0..31), "hidden".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    pub variants: serde_json::Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<&'static str>,
}

/// Editor metadata of a component: where it sits in Add Component (Unity's menu paths), what a
/// freshly added one holds, how its fields are edited.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct ComponentInfo {
    pub menu: &'static str,
    /// JSON a new component is created from (`{}` when absent: serde defaults).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defaults: Option<Value>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub fields: std::collections::BTreeMap<&'static str, FieldHint>,
    /// Allowed more than once per entity? (ECS: never — kept for the editor's checks.)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<&'static str>,
}

impl ComponentInfo {
    pub fn new(menu: &'static str) -> Self {
        ComponentInfo { menu, ..Default::default() }
    }
    pub fn defaults(mut self, v: Value) -> Self {
        self.defaults = Some(v);
        self
    }
    fn hint(&mut self, field: &'static str) -> &mut FieldHint {
        self.fields.entry(field).or_default()
    }
    pub fn options(mut self, field: &'static str, options: &[&str]) -> Self {
        let h = self.hint(field);
        h.kind = Some("enum");
        h.options = options.iter().map(|s| s.to_string()).collect();
        self
    }
    /// `variants` = `{ "box": [0.5, 0.5, 0.5], "sphere": 0.5, ... }`.
    pub fn variants(mut self, field: &'static str, variants: Value) -> Self {
        let h = self.hint(field);
        h.kind = Some("variant");
        if let Value::Object(m) = variants {
            h.variants = m;
        }
        self
    }
    pub fn asset(mut self, field: &'static str, kind: &'static str) -> Self {
        let h = self.hint(field);
        h.kind = Some("asset");
        h.asset = Some(kind);
        self
    }
    pub fn range(mut self, field: &'static str, min: f64, max: f64) -> Self {
        let h = self.hint(field);
        h.min = Some(min);
        h.max = Some(max);
        self
    }
    pub fn kind(mut self, field: &'static str, kind: &'static str) -> Self {
        self.hint(field).kind = Some(kind);
        self
    }
    pub fn tip(mut self, field: &'static str, text: &'static str) -> Self {
        self.hint(field).tooltip = Some(text);
        self
    }
    /// Components added with this one (Unity's RequireComponent).
    pub fn requires(mut self, names: &[&'static str]) -> Self {
        self.requires = names.to_vec();
        self
    }
}

struct BehaviourCodec {
    name: String,
    save: Box<dyn Fn(&dyn Behaviour) -> Option<Value>>,
    load: Box<dyn Fn(&Value) -> Result<Box<dyn Behaviour>, String>>,
}

pub struct SceneRegistry {
    components: Vec<ComponentCodec>,
    behaviours: HashMap<TypeId, BehaviourCodec>,
    behaviour_names: HashMap<String, TypeId>,
    info: HashMap<String, ComponentInfo>,
}

impl Default for SceneRegistry {
    /// The engine's built-in components already registered.
    fn default() -> Self {
        let mut r = SceneRegistry { components: Vec::new(), behaviours: HashMap::new(), behaviour_names: HashMap::new(), info: HashMap::new() };
        crate::builtins::register(&mut r);
        crate::component_info::describe_engine_components(&mut r);
        r
    }
}

impl SceneRegistry {
    /// Editor metadata of a component (by codec name); replaces earlier descriptions.
    pub fn describe(&mut self, name: &str, info: ComponentInfo) -> &mut Self {
        self.info.insert(name.to_string(), info);
        self
    }

    pub fn info(&self, name: &str) -> Option<&ComponentInfo> {
        self.info.get(name)
    }

    /// Names of the registered behaviours (Add Component ▸ Scripts).
    pub fn behaviour_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.behaviour_names.keys().cloned().collect();
        v.sort();
        v
    }

    /// Adds (or replaces, by name) a component codec. Codecs run in registration order.
    pub fn component(&mut self, codec: ComponentCodec) -> &mut Self {
        self.components.retain(|c| c.name != codec.name);
        self.components.push(codec);
        self
    }

    pub fn components(&self) -> &[ComponentCodec] {
        &self.components
    }

    pub fn find_component(&self, name: &str) -> Option<&ComponentCodec> {
        self.components.iter().find(|c| c.name == name)
    }

    /// Makes a behaviour type storable as `{ "type": name, ...its fields }`.
    pub fn behaviour<B: Behaviour + Serialize + DeserializeOwned>(&mut self, name: &str) -> &mut Self {
        let codec = BehaviourCodec {
            name: name.to_string(),
            save: Box::new(|b| b.as_any().downcast_ref::<B>().and_then(|b| serde_json::to_value(b).ok())),
            load: Box::new(|v| serde_json::from_value::<B>(v.clone()).map(|b| Box::new(b) as Box<dyn Behaviour>).map_err(|e| e.to_string())),
        };
        self.behaviour_names.insert(name.to_string(), TypeId::of::<B>());
        self.behaviours.insert(TypeId::of::<B>(), codec);
        self
    }

    pub(crate) fn save_behaviour(&self, b: &dyn Behaviour) -> Option<Value> {
        let codec = self.behaviours.get(&b.as_any().type_id())?;
        let mut v = (codec.save)(b)?;
        let mut obj = serde_json::Map::new();
        obj.insert("type".into(), Value::String(codec.name.clone()));
        match v.take() {
            Value::Object(fields) => obj.extend(fields),
            Value::Null => {}
            other => {
                obj.insert("value".into(), other);
            }
        }
        Some(Value::Object(obj))
    }

    pub(crate) fn load_behaviour(&self, v: &Value) -> Result<Box<dyn Behaviour>, String> {
        let name = v.get("type").and_then(Value::as_str).ok_or("behaviour without \"type\"")?;
        let id = self.behaviour_names.get(name).ok_or_else(|| format!("unknown behaviour {name} (register it with SceneRegistry::behaviour)"))?;
        let mut fields = v.clone();
        if let Value::Object(o) = &mut fields {
            o.remove("type");
            if let Some(inner) = o.remove("value") {
                fields = inner;
            }
        }
        (self.behaviours[id].load)(&fields)
    }
}
