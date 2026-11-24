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
    let c: T = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
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

struct BehaviourCodec {
    name: String,
    save: Box<dyn Fn(&dyn Behaviour) -> Option<Value>>,
    load: Box<dyn Fn(&Value) -> Result<Box<dyn Behaviour>, String>>,
}

pub struct SceneRegistry {
    components: Vec<ComponentCodec>,
    behaviours: HashMap<TypeId, BehaviourCodec>,
    behaviour_names: HashMap<String, TypeId>,
}

impl Default for SceneRegistry {
    /// The engine's built-in components already registered.
    fn default() -> Self {
        let mut r = SceneRegistry { components: Vec::new(), behaviours: HashMap::new(), behaviour_names: HashMap::new() };
        crate::builtins::register(&mut r);
        r
    }
}

impl SceneRegistry {
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
