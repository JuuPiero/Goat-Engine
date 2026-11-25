//! TypeScript / JavaScript scripts on Boa (a JavaScript engine in pure Rust: the same host
//! runs on desktop, in the editor (WebAssembly) and on the 3DS).
//!
//! - Scripts are ES modules: `export default class Player extends Behaviour { ... }` (or an
//!   export named like the file). They import the engine from `"goat"` ([`goat.js`](./goat.js))
//!   and each other with relative paths (`import { clamp } from "./util"`).
//! - `.ts` is compiled by oxc when loaded (feature `typescript`); console builds ship the `.js`
//!   produced at build time ([`compile_typescript`]), the loader prefers it when present.
//! - C# (`.cs`) runs here too: `tools/goat-cs` (Roslyn) translates each script to `X.cs.js`.
//! - Every engine call goes through [`goat_script::Api`] (shared with the other languages).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use boa_engine::builtins::promise::PromiseState;
use boa_engine::module::{ModuleLoader, Referrer};
use boa_engine::object::ObjectInitializer;
use boa_engine::property::Attribute;
use boa_engine::{js_string, Context, JsError, JsNativeError, JsObject, JsResult, JsString, JsValue, Module, NativeFunction, Source};
use goat_assets::AssetIo;
use goat_core::math::{EulerRot, Quat, Vec3};
use goat_runtime::{Context as GameContext, Entity};
use goat_script::{Api, Method, ScriptHost};
use serde_json::Value;

#[cfg(feature = "typescript")]
mod ts;
#[cfg(feature = "quickjs")]
mod qjs;
#[cfg(feature = "quickjs")]
pub use qjs::QjsHost;

const GOAT_MODULE: &str = include_str!("goat.js");

/// TypeScript → JavaScript (build tools: `goat build` precompiles scripts for consoles).
#[cfg(feature = "typescript")]
pub fn compile_typescript(key: &str, source: &str) -> Result<String, String> {
    ts::compile(key, source)
}

// ------------------------------------------------------------------ entity handles

/// Entities as small numbers for scripts (an f64 can't hold every 64-bit entity id).
#[derive(Default)]
struct Handles {
    list: Vec<Entity>,
    map: HashMap<Entity, u32>,
}

impl Handles {
    fn of(&mut self, e: Entity) -> f64 {
        if let Some(h) = self.map.get(&e) {
            return *h as f64;
        }
        let h = self.list.len() as u32;
        self.list.push(e);
        self.map.insert(e, h);
        h as f64
    }
    fn entity(&self, h: f64) -> Option<Entity> {
        if h < 0.0 {
            return None;
        }
        self.list.get(h as usize).copied()
    }
}

// ------------------------------------------------------------------ the bridge to the engine

#[derive(Clone, Copy)]
struct Bridge {
    api: *mut (),
    handles: *const RefCell<Handles>,
}

thread_local! {
    static BRIDGE: Cell<Option<Bridge>> = const { Cell::new(None) };
    static SHARED: RefCell<Option<HostShared>> = const { RefCell::new(None) };
}

/// What natives need of the host that is running (set by `JsHost::enter`): modules, script
/// classes, loaded ScriptableObject assets.
#[derive(Clone)]
struct HostShared {
    loader: Rc<Loader>,
    classes: Rc<RefCell<HashMap<String, JsObject>>>,
    /// ScriptableObject assets by key: one shared instance per asset (Unity).
    assets: Rc<RefCell<HashMap<String, JsObject>>>,
    /// goat.js `__applyProps`.
    apply: Rc<RefCell<Option<JsObject>>>,
}

fn shared() -> JsResult<HostShared> {
    SHARED.with(|s| s.borrow().clone()).ok_or_else(|| err("no script host is running"))
}

/// Loads + evaluates a module and returns its namespace.
fn evaluate_in(loader: &Loader, ctx: &mut Context, key: &str) -> JsResult<JsObject> {
    let module = loader.module(key, ctx)?;
    let promise = module.load_link_evaluate(ctx);
    ctx.run_jobs()?;
    match promise.state() {
        PromiseState::Fulfilled(_) => Ok(module.namespace(ctx)),
        PromiseState::Rejected(v) => Err(JsError::from_opaque(v)),
        PromiseState::Pending => Err(err(format!("{key}: the module didn't finish evaluating (top-level await?)"))),
    }
}

/// The class a script exports (default export, or the export named like the file).
fn class_in(sh: &HostShared, ctx: &mut Context, src: &str) -> JsResult<JsObject> {
    if let Some(c) = sh.classes.borrow().get(src) {
        return Ok(c.clone());
    }
    let ns = evaluate_in(&sh.loader, ctx, src)?;
    let stem = src.rsplit('/').next().unwrap_or(src).split('.').next().unwrap_or(src).to_string();
    let mut class = None;
    for name in ["default", stem.as_str()] {
        let v = ns.get(JsString::from(name), ctx)?;
        if let Some(o) = v.as_object().filter(|o| o.is_constructor()) {
            class = Some(o);
            break;
        }
    }
    let class = class.ok_or_else(|| err(format!("{src}: export a class (`export default class {stem} extends Behaviour`)")))?;
    if sh.apply.borrow().is_none() {
        let goat = evaluate_in(&sh.loader, ctx, "goat")?;
        *sh.apply.borrow_mut() = goat.get(js_string!("__applyProps"), ctx).ok().and_then(|v| v.as_object());
    }
    sh.classes.borrow_mut().insert(src.to_string(), class.clone());
    Ok(class)
}

/// A ScriptableObject asset (`{ "version": 1, "script": "scripts/EnemyData.ts", "data": {...} }`):
/// the script's class constructed, `data` applied over its defaults, `onEnable` called. Cached
/// per key: every reference to the asset gets the same object.
fn load_asset(ctx: &mut Context, key: &str) -> JsResult<JsValue> {
    let sh = shared()?;
    if let Some(o) = sh.assets.borrow().get(key) {
        return Ok(o.clone().into());
    }
    let io = sh.loader.io.borrow().clone().ok_or_else(|| err("no asset root yet"))?;
    let (script, data) = asset_file(&io, key).map_err(err)?;
    let class = class_in(&sh, ctx, &script)?;
    let object = class.construct(&[], None, ctx)?;
    object.set(js_string!("_key"), JsValue::from(JsString::from(key)), true, ctx)?;
    // cached before its data: assets referencing each other resolve to the same objects
    sh.assets.borrow_mut().insert(key.to_string(), object.clone());
    if let (Some(data), Some(apply)) = (data, sh.apply.borrow().clone()) {
        let d = JsValue::from_json(&data, ctx)?;
        apply.call(&JsValue::undefined(), &[object.clone().into(), d], ctx)?;
    }
    let on_enable = object.get(js_string!("onEnable"), ctx)?;
    if let Some(f) = on_enable.as_callable() {
        f.call(&object.clone().into(), &[], ctx)?;
    }
    Ok(object.into())
}

fn err(msg: impl Into<String>) -> JsError {
    JsNativeError::error().with_message(msg.into()).into()
}

/// Runs `f` with the engine API (engine-neutral natives: errors as strings).
pub(crate) fn with_bridge<R>(f: impl FnOnce(&mut Api<'_, '_>, &RefCell<Handles>) -> Result<R, String>) -> Result<R, String> {
    let b = BRIDGE.with(|c| c.get()).ok_or("the engine API is only available while a script runs")?;
    // SAFETY: as in `with_api`.
    let api = unsafe { &mut *(b.api as *mut Api<'static, 'static>) };
    let handles = unsafe { &*b.handles };
    f(api, handles)
}

fn num(a: &[Value], i: usize) -> f64 {
    match a.get(i) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::Bool(b)) => *b as u8 as f64,
        Some(Value::String(s)) => s.trim().parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}
fn f(a: &[Value], i: usize) -> f32 {
    num(a, i) as f32
}
fn string(a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => match n.as_f64() {
            Some(x) if x.fract() == 0.0 && x.abs() < 1e15 => format!("{}", x as i64),
            _ => n.to_string(),
        },
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Null) | None => "undefined".into(),
        Some(other) => other.to_string(),
    }
}
fn entity(a: &[Value], i: usize) -> Result<Entity, String> {
    let h = num(a, i);
    with_bridge(|_, hs| hs.borrow().entity(h).ok_or_else(|| "not a GameObject (destroyed?)".to_string()))
}
fn array(v: &[f32]) -> Value {
    Value::from(v.iter().map(|x| *x as f64).collect::<Vec<_>>())
}
fn unit<T>(_: T) -> Value {
    Value::Null
}

/// An engine-neutral native: JSON arguments in, JSON out (Boa and QuickJS wrap these).
pub(crate) type Native = fn(&[Value]) -> Result<Value, String>;

/// Replaces JSON objects for which `f` returns a value (depth-first, objects and arrays).
fn map_objects(v: &mut Value, f: &mut dyn FnMut(&serde_json::Map<String, Value>) -> Option<Value>) {
    match v {
        Value::Object(o) => {
            if let Some(new) = f(o) {
                *v = new;
                return;
            }
            for x in o.values_mut() {
                map_objects(x, f);
            }
        }
        Value::Array(a) => {
            for x in a.iter_mut() {
                map_objects(x, f);
            }
        }
        _ => {}
    }
}

/// The natives behind goat.js (`globalThis.__goat`), except `loadAsset` (it builds script
/// objects: each host has its own).
pub(crate) fn natives() -> Vec<(&'static str, usize, Native)> {
    vec![
        ("find", 1, |a| {
            let name = string(a, 0);
            with_bridge(|api, hs| Ok(api.find(&name).map(|e| hs.borrow_mut().of(e)).unwrap_or(-1.0))).map(Value::from)
        }),
        ("exists", 1, |a| {
            let h = num(a, 0);
            with_bridge(|api, hs| Ok(hs.borrow().entity(h).is_some_and(|e| api.exists(e)))).map(Value::from)
        }),
        ("name", 1, |a| {
            let e = entity(a, 0)?;
            with_bridge(|api, _| Ok(api.name(e).unwrap_or_default())).map(Value::from)
        }),
        ("setName", 2, |a| {
            let (e, n) = (entity(a, 0)?, string(a, 1));
            with_bridge(|api, _| Ok(api.set_name(e, &n))).map(unit)
        }),
        ("spawn", 1, |a| {
            let n = string(a, 0);
            with_bridge(|api, hs| Ok(hs.borrow_mut().of(api.spawn(&n)))).map(Value::from)
        }),
        ("destroy", 1, |a| {
            let e = entity(a, 0)?;
            with_bridge(|api, _| Ok(api.destroy(e))).map(unit)
        }),
        ("parent", 1, |a| {
            let e = entity(a, 0)?;
            with_bridge(|api, hs| Ok(api.parent(e).map(|p| hs.borrow_mut().of(p)).unwrap_or(-1.0))).map(Value::from)
        }),
        ("setParent", 2, |a| {
            let e = entity(a, 0)?;
            let p = num(a, 1);
            with_bridge(|api, hs| {
                let p = hs.borrow().entity(p);
                api.set_parent(e, p);
                Ok(())
            })
            .map(unit)
        }),
        ("instantiate", 2, |a| {
            let key = string(a, 0);
            let p = num(a, 1);
            with_bridge(|api, hs| {
                let parent = hs.borrow().entity(p);
                let e = api.instantiate(&key, parent)?;
                Ok(hs.borrow_mut().of(e))
            })
            .map(Value::from)
        }),
        ("getPos", 1, |a| {
            let e = entity(a, 0)?;
            with_bridge(|api, _| Ok(array(&api.position(e).to_array())))
        }),
        ("setPos", 4, |a| {
            let e = entity(a, 0)?;
            let v = Vec3::new(f(a, 1), f(a, 2), f(a, 3));
            with_bridge(|api, _| Ok(api.set_position(e, v))).map(unit)
        }),
        ("worldPos", 1, |a| {
            let e = entity(a, 0)?;
            with_bridge(|api, _| Ok(array(&api.world_position(e).to_array())))
        }),
        ("getEuler", 1, |a| {
            let e = entity(a, 0)?;
            with_bridge(|api, _| Ok(array(&api.euler(e).to_array())))
        }),
        ("setEuler", 4, |a| {
            let e = entity(a, 0)?;
            let v = Vec3::new(f(a, 1), f(a, 2), f(a, 3));
            with_bridge(|api, _| Ok(api.set_euler(e, v))).map(unit)
        }),
        ("getRot", 1, |a| {
            let e = entity(a, 0)?;
            with_bridge(|api, _| Ok(array(&api.rotation(e).to_array())))
        }),
        ("setRot", 5, |a| {
            let e = entity(a, 0)?;
            let q = Quat::from_xyzw(f(a, 1), f(a, 2), f(a, 3), f(a, 4));
            with_bridge(|api, _| Ok(api.set_rotation(e, q))).map(unit)
        }),
        ("rotate", 4, |a| {
            let e = entity(a, 0)?;
            let d = Vec3::new(f(a, 1), f(a, 2), f(a, 3));
            with_bridge(|api, _| {
                let q = api.rotation(e) * Quat::from_euler(EulerRot::YXZ, d.y.to_radians(), d.x.to_radians(), d.z.to_radians());
                api.set_rotation(e, q);
                Ok(())
            })
            .map(unit)
        }),
        ("lookAt", 4, |a| {
            let e = entity(a, 0)?;
            let target = Vec3::new(f(a, 1), f(a, 2), f(a, 3));
            with_bridge(|api, _| {
                if let Some(mut t) = api.transform(e) {
                    t.look_at(target, Vec3::Y);
                    api.set_transform(e, t);
                }
                Ok(())
            })
            .map(unit)
        }),
        ("eulerToQuat", 3, |a| {
            let d = Vec3::new(f(a, 0), f(a, 1), f(a, 2));
            Ok(array(&Quat::from_euler(EulerRot::YXZ, d.y.to_radians(), d.x.to_radians(), d.z.to_radians()).to_array()))
        }),
        ("quatToEuler", 4, |a| {
            let q = Quat::from_xyzw(f(a, 0), f(a, 1), f(a, 2), f(a, 3)).normalize();
            let (y, x, z) = q.to_euler(EulerRot::YXZ);
            Ok(array(&[x.to_degrees(), y.to_degrees(), z.to_degrees()]))
        }),
        ("getScale", 1, |a| {
            let e = entity(a, 0)?;
            with_bridge(|api, _| Ok(array(&api.scale(e).to_array())))
        }),
        ("setScale", 4, |a| {
            let e = entity(a, 0)?;
            let v = Vec3::new(f(a, 1), f(a, 2), f(a, 3));
            with_bridge(|api, _| Ok(api.set_scale(e, v))).map(unit)
        }),
        ("dt", 0, |_| with_bridge(|api, _| Ok(api.delta_time() as f64)).map(Value::from)),
        ("time", 0, |_| with_bridge(|api, _| Ok(api.time() as f64)).map(Value::from)),
        ("fixedDt", 0, |_| with_bridge(|api, _| Ok(api.fixed_delta_time() as f64)).map(Value::from)),
        ("unscaledDt", 0, |_| with_bridge(|api, _| Ok(api.unscaled_delta_time() as f64)).map(Value::from)),
        ("unscaledTime", 0, |_| with_bridge(|api, _| Ok(api.unscaled_time() as f64)).map(Value::from)),
        ("frameCount", 0, |_| with_bridge(|api, _| Ok(api.frame_count() as f64)).map(Value::from)),
        ("timeScale", 0, |_| with_bridge(|api, _| Ok(api.time_scale() as f64)).map(Value::from)),
        ("setTimeScale", 1, |a| {
            let s = f(a, 0);
            with_bridge(|api, _| Ok(api.set_time_scale(s))).map(unit)
        }),
        ("button", 1, |a| {
            let n = string(a, 0);
            with_bridge(|api, _| {
                let (h, p, r) = api.button(&n);
                Ok(h as i32 | (p as i32) << 1 | (r as i32) << 2)
            })
            .map(Value::from)
        }),
        ("stick", 0, |_| with_bridge(|api, _| Ok(array(&api.stick().to_array())))),
        ("pointer", 0, |_| {
            with_bridge(|api, _| {
                Ok(match api.pointer() {
                    Some((p, down)) => array(&[p.x, p.y, down as u8 as f32]),
                    None => Value::Null,
                })
            })
        }),
        ("action", 1, |a| {
            let n = string(a, 0);
            with_bridge(|api, _| Ok(array(&api.action_value(&n).to_array())))
        }),
        ("actionTriggered", 1, |a| {
            let n = string(a, 0);
            with_bridge(|api, _| Ok(api.action_triggered(&n))).map(Value::from)
        }),
        ("actionPressed", 1, |a| {
            let n = string(a, 0);
            with_bridge(|api, _| Ok(api.action_pressed(&n))).map(Value::from)
        }),
        ("log", 2, |a| {
            use goat_core::log::Level;
            let level = match num(a, 0) as i32 {
                3 => Level::Warn,
                4 => Level::Error,
                _ => Level::Info,
            };
            let msg = string(a, 1);
            with_bridge(|api, _| Ok(api.log(level, &msg))).map(unit)
        }),
        ("getComponent", 2, |a| {
            let e = entity(a, 0)?;
            let n = string(a, 1);
            with_bridge(|api, _| Ok(api.get_component(e, &n).unwrap_or(Value::Null)))
        }),
        ("setComponent", 3, |a| {
            let e = entity(a, 0)?;
            let n = string(a, 1);
            let v = a.get(2).cloned().unwrap_or(Value::Null);
            let v = if v.is_null() { Value::Object(Default::default()) } else { v };
            with_bridge(|api, _| api.set_component(e, &n, &v)).map(unit)
        }),
        ("removeComponent", 2, |a| {
            let e = entity(a, 0)?;
            let n = string(a, 1);
            with_bridge(|api, _| Ok(api.remove_component(e, &n))).map(unit)
        }),
        ("call", 3, |a| {
            let n = string(a, 0);
            // -1: a call without an object (Tween.value, Tween.events...)
            let e = if num(a, 1) < 0.0 { Entity::DANGLING } else { entity(a, 1)? };
            let args = match a.get(2) {
                Some(Value::Array(v)) => v.clone(),
                _ => Vec::new(),
            };
            with_bridge(|api, hs| {
                // GameObjects / Transforms ({ _h }) → {"$entity"}; results the other way ({"$go": handle})
                let mut args = args;
                for a in args.iter_mut() {
                    map_objects(a, &mut |o| {
                        let h = o.get("_h")?.as_f64()?;
                        hs.borrow().entity(h).map(goat_script::entity_json)
                    });
                }
                let mut r = api.call(&n, e, &args)?;
                map_objects(&mut r, &mut |o| {
                    let e = goat_script::entity_arg(&Value::Object(o.clone()))?;
                    Some(serde_json::json!({ "$go": hs.borrow_mut().of(e) }))
                });
                Ok(r)
            })
        }),
        ("quit", 0, |_| with_bridge(|api, _| Ok(api.quit())).map(unit)),
        ("debugBreak", 1, |a| {
            let msg = string(a, 0);
            with_bridge(|api, _| {
                api.ctx.resources.get_or_default::<goat_script::ScriptDebug>().break_requested = Some(msg);
                Ok(Value::Null)
            })
        }),
    ]
}

/// The script source of a ScriptableObject asset file and its data.
pub(crate) fn asset_file(io: &AssetIo, key: &str) -> Result<(String, Option<Value>), String> {
    let text = io.read_string(key).map_err(|e| format!("{key}: {e}"))?;
    let json: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("{key}: {e}"))?;
    let script = json.get("script").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| format!("{key}: the asset has no script"))?.to_string();
    Ok((script, json.get("data").filter(|d| d.is_object()).cloned()))
}

/// Entities in a message argument → GameObject handles ({"$go"}).
pub(crate) fn message_arg(arg: &Value, handles: &RefCell<Handles>) -> Value {
    let mut arg = arg.clone();
    map_objects(&mut arg, &mut |o| {
        let e = goat_script::entity_arg(&Value::Object(o.clone()))?;
        Some(serde_json::json!({ "$go": handles.borrow_mut().of(e) }))
    });
    arg
}
// ------------------------------------------------------------------ modules

/// Resolves `"goat"` and relative imports between scripts (asset keys).
struct Loader {
    /// The game's asset root (given, or taken from the AssetServer on first use).
    io: RefCell<Option<AssetIo>>,
    goat: RefCell<Option<Module>>,
    modules: RefCell<HashMap<String, Module>>,
}

/// The JavaScript of a script key: a built `.js` next to a `.ts` wins, else the `.ts` is compiled.
fn script_source(io: &AssetIo, key: &str) -> Result<String, String> {
    if key.ends_with(".cs") {
        // C#: goat-cs (tools/goat-cs) writes the module next to the source
        let built = format!("{key}.js");
        return io.read_string(&built).map_err(|_| format!("{key} isn't translated yet: run goat-cs (the editor does it when C# files change; `goat build` before packaging)"));
    }
    if let Some(stem) = key.strip_suffix(".ts") {
        let built = format!("{stem}.js");
        if io.exists(&built) {
            return io.read_string(&built).map_err(|e| e.to_string());
        }
        let ts = io.read_string(key).map_err(|e| e.to_string())?;
        #[cfg(feature = "typescript")]
        return ts::compile(key, &ts);
        #[cfg(not(feature = "typescript"))]
        return Err(format!("{key}: TypeScript isn't compiled in this build ({} bytes; ship the built .js)", ts.len()));
    }
    io.read_string(key).map_err(|e| e.to_string())
}

impl Loader {
    fn module(&self, key: &str, ctx: &mut Context) -> JsResult<Module> {
        if key == "goat" {
            if let Some(m) = self.goat.borrow().clone() {
                return Ok(m);
            }
            let m = Module::parse(Source::from_bytes(GOAT_MODULE).with_path(Path::new("goat")), None, ctx)?;
            *self.goat.borrow_mut() = Some(m.clone());
            return Ok(m);
        }
        if let Some(m) = self.modules.borrow().get(key).cloned() {
            return Ok(m);
        }
        let io = self.io.borrow().clone().ok_or_else(|| err("no asset root yet"))?;
        let code = script_source(&io, key).map_err(|e| err(format!("{key}: {e}")))?;
        let m = Module::parse(Source::from_bytes(&code).with_path(Path::new(key)), None, ctx)?;
        self.modules.borrow_mut().insert(key.to_string(), m.clone());
        Ok(m)
    }

    /// `./util` from `scripts/player.ts` → `scripts/util.ts` (or `.js`).
    fn resolve(&self, referrer: Option<&Path>, spec: &str) -> String {
        if spec == "goat" {
            return spec.into();
        }
        let base = referrer.map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        let key = if spec.starts_with('.') { goat_assets::resolve_relative(&base, spec) } else { spec.to_string() };
        let Some(io) = self.io.borrow().clone() else { return key };
        if io.exists(&key) {
            return key;
        }
        for ext in [".ts", ".js"] {
            let k = format!("{key}{ext}");
            if io.exists(&k) {
                return k;
            }
        }
        key
    }
}

impl ModuleLoader for Loader {
    fn load_imported_module(
        self: Rc<Self>,
        referrer: Referrer,
        request: boa_engine::module::ModuleRequest,
        context: &RefCell<&mut Context>,
    ) -> impl std::future::Future<Output = JsResult<Module>> {
        let spec = request.specifier().to_std_string_escaped();
        let path = referrer.path().map(Path::to_path_buf);
        let key = self.resolve(path.as_deref(), &spec);
        let result = self.module(&key, &mut context.borrow_mut());
        async { result }
    }
}

// ------------------------------------------------------------------ the host

struct Instance {
    object: JsObject,
    src: String,
}

/// TypeScript / JavaScript scripts (`.ts`, `.js`).
pub struct JsHost {
    ctx: Context,
    loader: Rc<Loader>,
    handles: Rc<RefCell<Handles>>,
    shared: HostShared,
    instances: HashMap<u32, Instance>,
    next: u32,
    apply_props: Option<JsObject>,
    /// goat.js `__tick` (coroutines, awaitables, tween callbacks), once the module is loaded.
    tick: Option<JsObject>,
    /// goat.js `__message` (SendMessage-like callbacks: collisions, triggers, pointer events).
    message: Option<JsObject>,
}

impl JsHost {
    /// A host reading scripts from the game's asset root (the AssetServer's).
    pub fn new() -> Self {
        Self::build(None)
    }
    /// A host reading scripts through `io`.
    pub fn with_io(io: AssetIo) -> Self {
        Self::build(Some(io))
    }

    fn build(io: Option<AssetIo>) -> Self {
        let loader = Rc::new(Loader { io: RefCell::new(io), goat: RefCell::new(None), modules: RefCell::new(HashMap::new()) });
        let mut ctx = Context::builder().module_loader(loader.clone()).build().expect("a JavaScript context");
        let mut natives = ObjectInitializer::new(&mut ctx);
        for (name, len, f) in crate::natives() {
            let call = move |_: &JsValue, args: &[JsValue], ctx: &mut Context| -> JsResult<JsValue> {
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(js_to_value(a, ctx)?);
                }
                let r = f(&vals).map_err(err)?;
                JsValue::from_json(&r, ctx)
            };
            natives.function(NativeFunction::from_copy_closure(call), js_string!(name), len);
        }
        natives.function(
            NativeFunction::from_fn_ptr(|_, a, c| {
                let key = a.first().cloned().unwrap_or_default().to_string(c)?.to_std_string_escaped();
                load_asset(c, &key)
            }),
            js_string!("loadAsset"),
            1,
        );
        let natives = natives.build();
        ctx.register_global_property(js_string!("__goat"), natives, Attribute::all()).expect("__goat");
        let shared = HostShared { loader: loader.clone(), classes: Default::default(), assets: Default::default(), apply: Default::default() };
        JsHost { ctx, loader, handles: Rc::new(RefCell::new(Handles::default())), shared, instances: HashMap::new(), next: 0, apply_props: None, tick: None, message: None }
    }

    /// Runs `f` with the bridge to `api` installed.
    fn enter<R>(&mut self, api: &mut Api, f: impl FnOnce(&mut Self) -> R) -> R {
        let bridge = Bridge { api: api as *mut Api as *mut (), handles: Rc::as_ptr(&self.handles) };
        let prev = BRIDGE.with(|c| c.replace(Some(bridge)));
        let prev_shared = SHARED.with(|s| s.replace(Some(self.shared.clone())));
        let r = f(self);
        SHARED.with(|s| *s.borrow_mut() = prev_shared);
        BRIDGE.with(|c| c.set(prev));
        r
    }

    fn describe(&mut self, e: JsError) -> String {
        match e.try_native(&mut self.ctx) {
            Ok(n) => n.to_string(),
            Err(_) => e.to_string(),
        }
    }

    /// Loads + evaluates a module and returns its namespace.
    fn evaluate(&mut self, key: &str) -> Result<JsObject, String> {
        evaluate_in(&self.loader.clone(), &mut self.ctx, key).map_err(|e| self.describe(e))
    }

    fn class_of(&mut self, src: &str) -> Result<JsObject, String> {
        let sh = self.shared.clone();
        let class = class_in(&sh, &mut self.ctx, src).map_err(|e| self.describe(e))?;
        if self.apply_props.is_none() {
            let goat = self.evaluate("goat")?;
            self.apply_props = goat.get(js_string!("__applyProps"), &mut self.ctx).ok().and_then(|v| v.as_object());
            self.tick = goat.get(js_string!("__tick"), &mut self.ctx).ok().and_then(|v| v.as_object());
            self.message = goat.get(js_string!("__message"), &mut self.ctx).ok().and_then(|v| v.as_object());
        }
        Ok(class)
    }

    fn ensure_io(&mut self, api: &mut Api) -> Result<(), String> {
        if self.loader.io.borrow().is_none() {
            let io = api.ctx.resources.get::<goat_assets::AssetServer>().map(|a| a.io.clone()).ok_or("no AssetServer: add AssetsPlugin before ScriptPlugin")?;
            *self.loader.io.borrow_mut() = Some(io);
        }
        Ok(())
    }

    /// The serialized fields of a script class as a fresh instance has them (the editor's
    /// Inspector: Behaviours and ScriptableObjects), in declaration order: own enumerable fields
    /// not starting with `_`, vectors as arrays, ScriptableObjects as their asset key.
    pub fn defaults(&mut self, api: &mut Api, src: &str) -> Result<Vec<(String, Value)>, String> {
        self.ensure_io(api)?;
        self.enter(api, |h| {
            let class = h.class_of(src)?;
            let object = class.construct(&[], None, &mut h.ctx).map_err(|e| h.describe(e))?;
            let goat = h.evaluate("goat")?;
            let fields = goat.get(js_string!("__fields"), &mut h.ctx).map_err(|e| h.describe(e))?;
            let f = fields.as_callable().ok_or("goat.js has no __fields")?;
            let v = f.call(&JsValue::undefined(), &[object.into()], &mut h.ctx).map_err(|e| h.describe(e))?;
            let v = v.to_json(&mut h.ctx).map(|v| v.unwrap_or(Value::Null)).map_err(|e| h.describe(e))?;
            let pairs: Vec<(String, Value)> = serde_json::from_value(v).map_err(|e| e.to_string())?;
            Ok(pairs)
        })
    }

    /// Whether a script's class extends goat's ScriptableObject.
    pub fn is_scriptable_object(&mut self, api: &mut Api, src: &str) -> Result<bool, String> {
        self.ensure_io(api)?;
        self.enter(api, |h| {
            let class = h.class_of(src)?;
            let goat = h.evaluate("goat")?;
            let base = goat.get(js_string!("ScriptableObject"), &mut h.ctx).map_err(|e| h.describe(e))?;
            let proto = class.get(js_string!("prototype"), &mut h.ctx).map_err(|e| h.describe(e))?;
            let base_proto = base.as_object().ok_or("no ScriptableObject")?.get(js_string!("prototype"), &mut h.ctx).map_err(|e| h.describe(e))?;
            let (Some(mut p), Some(bp)) = (proto.as_object(), base_proto.as_object()) else { return Ok(false) };
            for _ in 0..32 {
                if JsObject::equals(&p, &bp) {
                    return Ok(true);
                }
                match p.prototype() {
                    Some(n) => p = n,
                    None => break,
                }
            }
            Ok(false)
        })
    }

    /// Evaluates a snippet (tests, the editor's console).
    pub fn eval(&mut self, api: &mut Api, code: &str) -> Result<Value, String> {
        self.enter(api, |h| {
            let v = h.ctx.eval(Source::from_bytes(code)).map_err(|e| h.describe(e))?;
            v.to_json(&mut h.ctx).map(|v| v.unwrap_or(Value::Null)).map_err(|e| h.describe(e))
        })
    }
}

impl Default for JsHost {
    fn default() -> Self {
        Self::new()
    }
}

/// A JavaScript value as JSON (numbers / strings directly; undefined = null).
fn js_to_value(v: &JsValue, ctx: &mut Context) -> JsResult<Value> {
    if let Some(n) = v.as_number() {
        return Ok(serde_json::Number::from_f64(n).map(Value::Number).unwrap_or(Value::Null));
    }
    if let Some(s) = v.as_string() {
        return Ok(Value::String(s.to_std_string_escaped()));
    }
    if v.is_undefined() || v.is_null() {
        return Ok(Value::Null);
    }
    Ok(v.to_json(ctx)?.unwrap_or(Value::Null))
}

impl ScriptHost for JsHost {
    fn name(&self) -> &str {
        "TypeScript / JavaScript / C#"
    }
    fn handles(&self, src: &str) -> bool {
        src.ends_with(".ts") || src.ends_with(".js") || src.ends_with(".mjs") || src.ends_with(".cs")
    }

    fn instantiate(&mut self, api: &mut Api, entity: Entity, src: &str, props: &Value) -> Result<u32, String> {
        self.ensure_io(api)?;
        self.enter(api, |h| {
            let class = h.class_of(src)?;
            let object = class.construct(&[], None, &mut h.ctx).map_err(|e| h.describe(e))?;
            let handle = h.handles.borrow_mut().of(entity);
            object.set(js_string!("_h"), JsValue::from(handle), true, &mut h.ctx).map_err(|e| h.describe(e))?;
            if !props.is_null() {
                let p = JsValue::from_json(props, &mut h.ctx).map_err(|e| h.describe(e))?;
                if let Some(apply) = h.apply_props.clone() {
                    apply.call(&JsValue::undefined(), &[object.clone().into(), p], &mut h.ctx).map_err(|e| h.describe(e))?;
                }
            }
            // the console's $scripts
            let goat = h.evaluate("goat")?;
            if let Some(track) = goat.get(js_string!("__track"), &mut h.ctx).ok().and_then(|v| v.as_callable()) {
                track.call(&JsValue::undefined(), &[object.clone().into()], &mut h.ctx).map_err(|e| h.describe(e))?;
            }
            h.next += 1;
            h.instances.insert(h.next, Instance { object, src: src.to_string() });
            Ok(h.next)
        })
    }

    fn call(&mut self, api: &mut Api, instance: u32, _entity: Entity, method: Method) -> Result<(), String> {
        let Some(obj) = self.instances.get(&instance).map(|i| i.object.clone()) else { return Ok(()) };
        self.enter(api, |h| {
            let f = obj.get(js_string!(method.js_name()), &mut h.ctx).map_err(|e| h.describe(e))?;
            if let Some(f) = f.as_callable() {
                f.call(&obj.clone().into(), &[], &mut h.ctx).map_err(|e| h.describe(e))?;
                // promises / async continuations started by the call
                h.ctx.run_jobs().map_err(|e| h.describe(e))?;
            }
            Ok(())
        })
    }

    fn destroy(&mut self, instance: u32) {
        self.instances.remove(&instance);
    }

    fn message(&mut self, api: &mut Api, instance: u32, _entity: Entity, name: &str, arg: &Value) -> Result<(), String> {
        let (Some(obj), Some(dispatch)) = (self.instances.get(&instance).map(|i| i.object.clone()), self.message.clone()) else { return Ok(()) };
        let name = name.to_string();
        self.enter(api, |h| {
            // entities → GameObject handles
            let arg = message_arg(arg, &h.handles);
            let a = JsValue::from_json(&arg, &mut h.ctx).map_err(|e| h.describe(e))?;
            dispatch.call(&JsValue::undefined(), &[obj.into(), JsValue::from(js_string!(name.as_str())), a], &mut h.ctx).map_err(|e| h.describe(e))?;
            h.ctx.run_jobs().map_err(|e| h.describe(e))?;
            Ok(())
        })
    }

    fn tick(&mut self, api: &mut Api) -> Result<(), String> {
        let Some(tick) = self.tick.clone() else { return Ok(()) };
        self.enter(api, |h| {
            tick.call(&JsValue::undefined(), &[], &mut h.ctx).map_err(|e| h.describe(e))?;
            h.ctx.run_jobs().map_err(|e| h.describe(e))?;
            Ok(())
        })
    }

    fn invalidate(&mut self, src: &str) {
        self.shared.classes.borrow_mut().remove(src);
        self.loader.modules.borrow_mut().remove(src);
        // ScriptableObject assets are re-read (their file or their script changed)
        self.shared.assets.borrow_mut().clear();
    }

    fn reload(&mut self, api: &mut Api, src: &str) -> Result<usize, String> {
        self.invalidate(src);
        let targets: Vec<JsObject> = self.instances.values().filter(|i| i.src == src).map(|i| i.object.clone()).collect();
        if targets.is_empty() {
            return Ok(0);
        }
        self.ensure_io(api)?;
        self.enter(api, |h| {
            let class = h.class_of(src)?;
            let proto = class.get(js_string!("prototype"), &mut h.ctx).map_err(|e| h.describe(e))?;
            let proto = proto.as_object().ok_or("the class has no prototype")?;
            // the running objects keep their state and get the new methods
            for o in &targets {
                o.set_prototype(Some(proto.clone()));
            }
            Ok(targets.len())
        })
    }

    fn eval(&mut self, api: &mut Api, code: &str, selected: Option<Entity>) -> Result<Value, String> {
        self.ensure_io(api)?;
        let code = code.to_string();
        self.enter(api, |h| {
            let goat = h.evaluate("goat")?;
            let f = goat.get(js_string!("__console"), &mut h.ctx).map_err(|e| h.describe(e))?;
            let f = f.as_callable().ok_or("goat.js has no __console")?;
            let handle = selected.map(|e| h.handles.borrow_mut().of(e)).unwrap_or(-1.0);
            let r = f.call(&JsValue::undefined(), &[JsValue::from(handle), JsValue::from(JsString::from(code.as_str()))], &mut h.ctx).map_err(|e| h.describe(e))?;
            h.ctx.run_jobs().map_err(|e| h.describe(e))?;
            Ok(Value::String(r.to_string(&mut h.ctx).map(|s| s.to_std_string_escaped()).unwrap_or_default()))
        })
    }
}

/// The context a host call gets in Rust tests (no other way to build an `Api`).
pub fn api_of<'a, 'c>(ctx: &'a mut GameContext<'c>) -> Api<'a, 'c> {
    Api::new(ctx)
}

#[cfg(test)]
mod tests;

/// The same suite on QuickJS.
#[cfg(all(test, feature = "quickjs"))]
mod qjs_tests {
    #[allow(unused_imports)]
    use crate::QjsHost as JsHost;
    include!("tests.rs");
}
