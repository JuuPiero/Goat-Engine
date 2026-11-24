//! Scripting core, independent of any language.
//!
//! - [`Script`] component: script instances on an entity (Unity's MonoBehaviours written in a
//!   scripting language). In scene files:
//!   `"Script": [ { "src": "scripts/Spin.ts", "props": { "speed": 90 } }, { "src": "native:Wobble" } ]`.
//! - [`ScriptHost`]: one per language. A host says which sources it runs (`.ts` / `.js`,
//!   `native:` for C / C++ behaviours registered through the C ABI, `.cs` transpiled to TS),
//!   creates instances and calls their methods.
//! - [`Api`]: the engine operations scripts use — every host forwards to it, so all languages
//!   behave the same: entities (find / spawn / destroy / parent), Transform, time, input
//!   (buttons, pointer, input actions), logging, any registered component as JSON (through the
//!   scene codecs), and [`Extensions`] (named calls that plugins / games add, e.g.
//!   `Animator.setTrigger`).
//! - [`ScriptPlugin`] runs instances: Start before the first Update, Update then LateUpdate every
//!   frame (after the Rust behaviours), FixedUpdate at the fixed rate, OnDestroy when the entity
//!   or the script goes away. A script that throws is logged and disabled.

use std::collections::HashMap;

use goat_core::math::{EulerRot, Quat, Vec2, Vec3};
use goat_runtime::{Button, Context, Entity, GlobalTransform, Name, Parent, Plugin, Transform};
use goat_scene::{LoadContext, SaveContext, SceneRegistry};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ------------------------------------------------------------------ component

/// One script attached to an entity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptRef {
    /// `scripts/Player.ts`, `scripts/legacy.js`, `native:Wobble`, `scripts/Enemy.cs`.
    pub src: String,
    /// Initial field values (Unity's serialized fields).
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub props: Value,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}

impl ScriptRef {
    pub fn new(src: &str) -> Self {
        ScriptRef { src: src.into(), props: Value::Null, enabled: true }
    }
    pub fn with_props(mut self, props: Value) -> Self {
        self.props = props;
        self
    }
}

/// The scripts of an entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script(pub Vec<ScriptRef>);

impl Script {
    pub fn one(src: &str) -> Self {
        Script(vec![ScriptRef::new(src)])
    }
}

// ------------------------------------------------------------------ hosts

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Start,
    Update,
    FixedUpdate,
    LateUpdate,
    OnDestroy,
}

impl Method {
    /// The method name scripts define (camelCase, like Unity's in TS).
    pub fn js_name(self) -> &'static str {
        match self {
            Method::Start => "start",
            Method::Update => "update",
            Method::FixedUpdate => "fixedUpdate",
            Method::LateUpdate => "lateUpdate",
            Method::OnDestroy => "onDestroy",
        }
    }
}

/// A scripting language.
pub trait ScriptHost: 'static {
    fn name(&self) -> &str;
    /// Whether this host runs `src`.
    fn handles(&self, src: &str) -> bool;
    /// Creates an instance of the script for `entity` (props applied). Returns its id.
    fn instantiate(&mut self, api: &mut Api, entity: Entity, src: &str, props: &Value) -> Result<u32, String>;
    /// Calls a method if the script defines it.
    fn call(&mut self, api: &mut Api, instance: u32, entity: Entity, method: Method) -> Result<(), String>;
    /// Forgets an instance (after OnDestroy).
    fn destroy(&mut self, instance: u32);
    /// A source changed on disk: the next instances use the new code.
    fn invalidate(&mut self, _src: &str) {}
    /// Once per frame after every Update (Unity runs coroutines there): language-side
    /// schedulers (coroutines, awaitables, tween callbacks).
    fn tick(&mut self, _api: &mut Api) -> Result<(), String> {
        Ok(())
    }
}

// ------------------------------------------------------------------ extensions

pub type ExtensionFn = fn(&mut Api, Entity, &[Value]) -> Result<Value, String>;

/// Named engine calls that subsystems / games add for scripts (a resource):
/// `extensions.add("Animator.setTrigger", f)`; scripts call `goat.call(name, entity, ...args)`.
#[derive(Default)]
pub struct Extensions(HashMap<String, ExtensionFn>);

impl Extensions {
    pub fn add(&mut self, name: &str, f: ExtensionFn) -> &mut Self {
        self.0.insert(name.into(), f);
        self
    }
    pub fn get(&self, name: &str) -> Option<ExtensionFn> {
        self.0.get(name).copied()
    }
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.0.keys().cloned().collect();
        v.sort();
        v
    }
}

/// An entity inside the JSON exchanged with scripts: `{"$entity": <bits>}`. Hosts turn it into
/// their object (a GameObject in TS / C#) and their objects passed as arguments back into it.
pub fn entity_json(e: Entity) -> Value {
    serde_json::json!({ "$entity": e.to_bits().get() as f64 })
}

/// The entity of an `{"$entity": ...}` argument.
pub fn entity_arg(v: &Value) -> Option<Entity> {
    let bits = v.get("$entity")?.as_f64()?;
    Entity::from_bits(bits as u64)
}

/// Core GameObject calls every host gets: active state and tags (Unity's SetActive,
/// activeSelf, activeInHierarchy, tag, CompareTag, FindWithTag, FindGameObjectsWithTag).
fn register_core(x: &mut Extensions) {
    fn set_active(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
        goat_runtime::set_active(api.ctx.world, e, a.first().and_then(|v| v.as_bool()).unwrap_or(true));
        Ok(Value::Null)
    }
    fn active_self(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
        Ok(Value::Bool(goat_runtime::active_self(api.ctx.world, e)))
    }
    fn active_in_hierarchy(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
        Ok(Value::Bool(goat_runtime::active_in_hierarchy(api.ctx.world, e)))
    }
    fn tag(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
        Ok(Value::String(api.ctx.world.get::<&goat_runtime::Tag>(e).map(|t| t.0.clone()).unwrap_or_else(|_| "Untagged".into())))
    }
    fn set_tag(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
        let t = a.first().and_then(|v| v.as_str()).unwrap_or("Untagged");
        if t.is_empty() || t == "Untagged" {
            let _ = api.ctx.world.remove_one::<goat_runtime::Tag>(e);
        } else {
            let _ = api.ctx.world.insert_one(e, goat_runtime::Tag(t.into()));
        }
        Ok(Value::Null)
    }
    fn find_all(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
        let t = a.first().and_then(|v| v.as_str()).unwrap_or("");
        Ok(Value::Array(goat_runtime::find_with_tag(api.ctx.world, t).into_iter().map(entity_json).collect()))
    }
    fn find_one(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
        Ok(find_all(api, e, a)?.as_array().and_then(|l| l.first().cloned()).unwrap_or(Value::Null))
    }
    fn load_scene(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
        let which = match a.first() {
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::String(s)) => s.clone(),
            _ => return Err("SceneManager.loadScene(nameOrIndex)".into()),
        };
        let mode = if a.get(1).and_then(|v| v.as_str()) == Some("additive") { goat_scene::LoadMode::Additive } else { goat_scene::LoadMode::Single };
        goat_scene::request_scene(api.ctx, &which, mode).map(Value::String).map_err(|e| e.to_string())
    }
    fn active_scene(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
        let key = api.ctx.resources.get::<goat_scene::ActiveScene>().map(|s| s.0.clone()).unwrap_or_default();
        let index = api.ctx.resources.get::<goat_runtime::ProjectSettings>().and_then(|s| s.build_scenes().iter().position(|p| *p == key));
        Ok(serde_json::json!({ "key": key, "buildIndex": index.map(|i| i as i64).unwrap_or(-1) }))
    }
    fn scene_count(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
        Ok(Value::from(api.ctx.resources.get::<goat_runtime::ProjectSettings>().map(|s| s.build_scenes().len()).unwrap_or(0)))
    }
    x.add("SceneManager.loadScene", load_scene).add("SceneManager.activeScene", active_scene).add("SceneManager.sceneCountInBuildSettings", scene_count);
    x.add("GameObject.setActive", set_active)
        .add("GameObject.activeSelf", active_self)
        .add("GameObject.activeInHierarchy", active_in_hierarchy)
        .add("GameObject.tag", tag)
        .add("GameObject.setTag", set_tag)
        .add("GameObject.findWithTag", find_one)
        .add("GameObject.findGameObjectsWithTag", find_all);
}

/// `goat_script::extensions(ctx).add(...)`.
pub fn extensions<'a>(ctx: &'a mut Context) -> &'a mut Extensions {
    ctx.resources.get_or_default::<Extensions>()
}

// ------------------------------------------------------------------ the API

/// What scripts can do — a thin, safe layer over [`Context`].
pub struct Api<'a, 'c> {
    pub ctx: &'a mut Context<'c>,
}

/// A button by its name (`"A"`, `"dpad/up"`, `"start"`...).
pub fn button_named(name: &str) -> Option<Button> {
    match goat_input::Control::parse(name)? {
        goat_input::Control::Button(b) => Some(b),
        _ => None,
    }
}

impl<'a, 'c> Api<'a, 'c> {
    pub fn new(ctx: &'a mut Context<'c>) -> Self {
        Api { ctx }
    }

    // entities
    pub fn find(&self, name: &str) -> Option<Entity> {
        self.ctx.find(name)
    }
    pub fn exists(&self, e: Entity) -> bool {
        self.ctx.world.contains(e)
    }
    pub fn name(&self, e: Entity) -> Option<String> {
        self.ctx.world.get::<&Name>(e).ok().map(|n| n.0.clone())
    }
    pub fn set_name(&mut self, e: Entity, name: &str) {
        if let Ok(mut n) = self.ctx.world.get::<&mut Name>(e) {
            n.0 = name.into();
        }
    }
    pub fn spawn(&mut self, name: &str) -> Entity {
        self.ctx.world.spawn((Name(name.into()), Transform::default(), GlobalTransform::default()))
    }
    /// Destroys an entity and its children.
    pub fn destroy(&mut self, e: Entity) {
        let pairs: Vec<(Entity, Entity)> = self.ctx.world.query::<&Parent>().iter().map(|(c, p)| (p.0, c)).collect();
        let mut doomed = vec![e];
        let mut i = 0;
        while i < doomed.len() {
            let x = doomed[i];
            doomed.extend(pairs.iter().filter(|(p, _)| *p == x).map(|(_, c)| *c));
            i += 1;
        }
        for d in doomed {
            let _ = self.ctx.world.despawn(d);
        }
    }
    pub fn parent(&self, e: Entity) -> Option<Entity> {
        self.ctx.world.get::<&Parent>(e).ok().map(|p| p.0)
    }
    pub fn set_parent(&mut self, e: Entity, parent: Option<Entity>) {
        match parent {
            Some(p) if p != e => {
                let _ = self.ctx.world.insert_one(e, Parent(p));
            }
            _ => {
                let _ = self.ctx.world.remove_one::<Parent>(e);
            }
        }
    }
    /// A prefab / model instance (`prefabs/Enemy.prefab.json`, `models/a.glb`).
    pub fn instantiate(&mut self, key: &str, parent: Option<Entity>) -> Result<Entity, String> {
        if key.ends_with(".glb") || key.ends_with(".gltf") {
            return goat_assets::spawn_model(self.ctx, key, parent).map_err(|e| e.to_string());
        }
        goat_scene::instantiate(self.ctx, key, parent).map_err(|e| e.to_string())
    }

    // transform (local, like Unity's localPosition — roots: world)
    pub fn transform(&self, e: Entity) -> Option<Transform> {
        self.ctx.world.get::<&Transform>(e).ok().map(|t| *t)
    }
    pub fn set_transform(&mut self, e: Entity, t: Transform) {
        if let Ok(mut x) = self.ctx.world.get::<&mut Transform>(e) {
            *x = t;
        }
    }
    pub fn position(&self, e: Entity) -> Vec3 {
        self.transform(e).map(|t| t.position).unwrap_or(Vec3::ZERO)
    }
    pub fn set_position(&mut self, e: Entity, p: Vec3) {
        if let Ok(mut t) = self.ctx.world.get::<&mut Transform>(e) {
            t.position = p;
        }
    }
    /// Euler angles in degrees (Unity's order: Z, X, then Y).
    pub fn euler(&self, e: Entity) -> Vec3 {
        let (y, x, z) = self.transform(e).map(|t| t.rotation).unwrap_or(Quat::IDENTITY).to_euler(EulerRot::YXZ);
        Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
    }
    pub fn set_euler(&mut self, e: Entity, d: Vec3) {
        if let Ok(mut t) = self.ctx.world.get::<&mut Transform>(e) {
            t.rotation = Quat::from_euler(EulerRot::YXZ, d.y.to_radians(), d.x.to_radians(), d.z.to_radians());
        }
    }
    pub fn rotation(&self, e: Entity) -> Quat {
        self.transform(e).map(|t| t.rotation).unwrap_or(Quat::IDENTITY)
    }
    pub fn set_rotation(&mut self, e: Entity, q: Quat) {
        if let Ok(mut t) = self.ctx.world.get::<&mut Transform>(e) {
            t.rotation = q.normalize();
        }
    }
    pub fn scale(&self, e: Entity) -> Vec3 {
        self.transform(e).map(|t| t.scale).unwrap_or(Vec3::ONE)
    }
    pub fn set_scale(&mut self, e: Entity, s: Vec3) {
        if let Ok(mut t) = self.ctx.world.get::<&mut Transform>(e) {
            t.scale = s;
        }
    }
    /// World position (last propagated).
    pub fn world_position(&self, e: Entity) -> Vec3 {
        self.ctx.world.get::<&GlobalTransform>(e).map(|g| g.0.w_axis.truncate()).unwrap_or(Vec3::ZERO)
    }

    // time
    pub fn delta_time(&self) -> f32 {
        self.ctx.time.delta
    }
    pub fn time(&self) -> f32 {
        self.ctx.time.elapsed as f32
    }
    pub fn fixed_delta_time(&self) -> f32 {
        self.ctx.time.fixed_delta
    }
    pub fn unscaled_delta_time(&self) -> f32 {
        self.ctx.time.unscaled_delta
    }
    pub fn unscaled_time(&self) -> f32 {
        self.ctx.time.unscaled_elapsed
    }
    pub fn frame_count(&self) -> u64 {
        self.ctx.time.frame
    }
    pub fn time_scale(&self) -> f32 {
        self.ctx.resources.get::<goat_runtime::TimeScale>().map(|t| t.0).unwrap_or(self.ctx.time.time_scale)
    }
    /// Unity's `Time.timeScale = s` (applies from the next frame).
    pub fn set_time_scale(&mut self, s: f32) {
        self.ctx.resources.insert(goat_runtime::TimeScale(s.max(0.0)));
    }

    // input
    pub fn button(&self, name: &str) -> (bool, bool, bool) {
        match button_named(name) {
            Some(b) => (self.ctx.input.held(b), self.ctx.input.pressed(b), self.ctx.input.released(b)),
            None => (false, false, false),
        }
    }
    pub fn stick(&self) -> Vec2 {
        self.ctx.input.stick()
    }
    pub fn pointer(&self) -> Option<(Vec2, bool)> {
        self.ctx.input.pointer.map(|p| (p, self.ctx.input.pointer_down))
    }
    /// An input action's value (`Player/Move`).
    pub fn action_value(&self, name: &str) -> Vec2 {
        self.ctx.resources.get::<goat_input::InputActions>().map(|a| a.read_vec2(name)).unwrap_or(Vec2::ZERO)
    }
    pub fn action_triggered(&self, name: &str) -> bool {
        self.ctx.resources.get::<goat_input::InputActions>().is_some_and(|a| a.triggered(name))
    }
    pub fn action_pressed(&self, name: &str) -> bool {
        self.ctx.resources.get::<goat_input::InputActions>().is_some_and(|a| a.is_pressed(name))
    }

    // log
    pub fn log(&self, level: goat_core::log::Level, msg: &str) {
        goat_core::log::write(level, format_args!("{msg}"));
    }

    // components as JSON (the scene format)
    pub fn component_names(&self) -> Vec<String> {
        self.ctx.resources.get::<SceneRegistry>().map(|r| r.components().iter().map(|c| c.name.to_string()).collect()).unwrap_or_default()
    }
    pub fn get_component(&self, e: Entity, name: &str) -> Option<Value> {
        let registry = self.ctx.resources.get::<SceneRegistry>()?;
        let codec = registry.find_component(name)?;
        let assets = self.ctx.resources.get::<goat_assets::AssetServer>()?;
        (codec.save)(&SaveContext { world: self.ctx.world, assets }, e)
    }
    pub fn has_component(&self, e: Entity, name: &str) -> bool {
        self.get_component(e, name).is_some()
    }
    /// Adds or replaces a component from its scene JSON.
    pub fn set_component(&mut self, e: Entity, name: &str, value: &Value) -> Result<(), String> {
        let registry = self.ctx.resources.take::<SceneRegistry>().ok_or("no scene registry")?;
        let result = match (registry.find_component(name).copied(), self.ctx.resources.get_mut::<goat_assets::AssetServer>()) {
            (Some(codec), Some(assets)) => (codec.load)(&mut LoadContext { world: self.ctx.world, gfx: self.ctx.gfx, assets, source: "script" }, e, value),
            (None, _) => Err(format!("unknown component {name}")),
            (_, None) => Err("no asset server".into()),
        };
        self.ctx.resources.put_back(registry);
        result
    }
    pub fn remove_component(&mut self, e: Entity, name: &str) {
        let codec = self.ctx.resources.get::<SceneRegistry>().and_then(|r| r.find_component(name).copied());
        if let Some(c) = codec {
            (c.remove)(self.ctx.world, e);
        }
    }

    /// Runs an extension call.
    pub fn call(&mut self, name: &str, e: Entity, args: &[Value]) -> Result<Value, String> {
        let f = self.ctx.resources.get::<Extensions>().and_then(|x| x.get(name)).ok_or_else(|| format!("no engine call {name}"))?;
        f(self, e, args)
    }

    pub fn quit(&mut self) {
        self.ctx.quit();
    }
}

// ------------------------------------------------------------------ the plugin

struct Instance {
    entity: Entity,
    slot: usize,
    src: String,
    host: usize,
    id: u32,
    started: bool,
    failed: bool,
}

/// Runs the [`Script`] components through the hosts given to it.
pub struct ScriptPlugin {
    hosts: Vec<Box<dyn ScriptHost>>,
    instances: Vec<Instance>,
}

impl ScriptPlugin {
    pub fn new() -> Self {
        ScriptPlugin { hosts: Vec::new(), instances: Vec::new() }
    }
    pub fn with_host(mut self, host: impl ScriptHost) -> Self {
        self.hosts.push(Box::new(host));
        self
    }

    fn sync(&mut self, ctx: &mut Context) {
        // gone entities / removed scripts → OnDestroy
        let mut i = 0;
        while i < self.instances.len() {
            let inst = &self.instances[i];
            let still = ctx.world.get::<&Script>(inst.entity).ok().and_then(|s| s.0.get(inst.slot).map(|r| r.src == inst.src && r.enabled)).unwrap_or(false);
            if still {
                i += 1;
                continue;
            }
            let inst = self.instances.swap_remove(i);
            // also when the entity itself was destroyed (Unity's OnDestroy); engine calls on it
            // then find nothing
            if inst.started && !inst.failed {
                let mut api = Api::new(ctx);
                let _ = self.hosts[inst.host].call(&mut api, inst.id, inst.entity, Method::OnDestroy);
            }
            self.hosts[inst.host].destroy(inst.id);
        }
        // new ones
        let wanted: Vec<(Entity, usize, ScriptRef)> = ctx
            .world
            .query::<&Script>()
            .iter()
            .flat_map(|(e, s)| s.0.iter().enumerate().filter(|(_, r)| r.enabled).map(move |(k, r)| (e, k, r.clone())))
            .filter(|(e, k, _)| !self.instances.iter().any(|i| i.entity == *e && i.slot == *k))
            .collect();
        for (e, slot, r) in wanted {
            let Some(host) = self.hosts.iter().position(|h| h.handles(&r.src)) else {
                goat_core::warn!("{}: no script host runs this (languages: {})", r.src, self.hosts.iter().map(|h| h.name().to_string()).collect::<Vec<_>>().join(", "));
                self.instances.push(Instance { entity: e, slot, src: r.src.clone(), host: 0, id: u32::MAX, started: true, failed: true });
                continue;
            };
            let mut api = Api::new(ctx);
            match self.hosts[host].instantiate(&mut api, e, &r.src, &r.props) {
                Ok(id) => self.instances.push(Instance { entity: e, slot, src: r.src, host, id, started: false, failed: false }),
                Err(err) => {
                    goat_core::error!("{}: {err}", r.src);
                    self.instances.push(Instance { entity: e, slot, src: r.src, host, id: u32::MAX, started: true, failed: true });
                }
            }
        }
    }

    fn run(&mut self, ctx: &mut Context, method: Method) {
        for i in 0..self.instances.len() {
            let inst = &self.instances[i];
            // inactive objects (self or a parent) don't run (and don't Start until activated)
            if inst.failed || !ctx.world.contains(inst.entity) || ctx.world.get::<&goat_runtime::InactiveInHierarchy>(inst.entity).is_ok() {
                continue;
            }
            let (host, id, e) = (inst.host, inst.id, inst.entity);
            let mut api = Api::new(ctx);
            if !self.instances[i].started {
                self.instances[i].started = true;
                if let Err(err) = self.hosts[host].call(&mut api, id, e, Method::Start) {
                    goat_core::error!("{} start: {err}", self.instances[i].src);
                    self.instances[i].failed = true;
                    continue;
                }
                if method == Method::FixedUpdate {
                    // Start runs before the first FixedUpdate too
                }
            }
            if let Err(err) = self.hosts[host].call(&mut api, id, e, method) {
                goat_core::error!("{} {}: {err} (script disabled)", self.instances[i].src, method.js_name());
                self.instances[i].failed = true;
            }
        }
    }

    /// Instances alive (tests, the editor's stats).
    pub fn instance_count(&self) -> usize {
        self.instances.iter().filter(|i| !i.failed).count()
    }
}

impl Default for ScriptPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for ScriptPlugin {
    fn build(&mut self, ctx: &mut Context) {
        register_core(ctx.resources.get_or_default::<Extensions>());
        goat_scene::registry(ctx).component(CODEC);
    }
    fn fixed_update(&mut self, ctx: &mut Context) {
        self.sync(ctx);
        self.run(ctx, Method::FixedUpdate);
    }
    fn update(&mut self, ctx: &mut Context) {
        self.sync(ctx);
        self.run(ctx, Method::Update);
        for h in self.hosts.iter_mut() {
            let mut api = Api::new(ctx);
            if let Err(err) = h.tick(&mut api) {
                goat_core::error!("{}: {err}", h.name());
            }
        }
        self.run(ctx, Method::LateUpdate);
    }
}

// ------------------------------------------------------------------ scene codec

fn save(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let s = ctx.world.get::<&Script>(e).ok()?;
    serde_json::to_value(&s.0).ok()
}

fn load(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    // one script `{...}` or a list of them
    let list: Vec<ScriptRef> = match v {
        Value::Array(_) => serde_json::from_value(v.clone()).map_err(|e| e.to_string())?,
        Value::Object(_) => vec![serde_json::from_value(v.clone()).map_err(|e| e.to_string())?],
        _ => return Err("Script: expected { \"src\": ... } or a list".into()),
    };
    ctx.world.insert_one(e, Script(list)).map_err(|_| "entity vanished".to_string())
}

pub const CODEC: goat_scene::ComponentCodec = goat_scene::ComponentCodec { name: "Script", save, load, remove: goat_scene::remove_typed::<Script> };

#[cfg(test)]
mod tests;
