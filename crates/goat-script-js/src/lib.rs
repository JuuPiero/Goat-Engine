//! TypeScript / JavaScript scripts on Boa (a JavaScript engine in pure Rust: the same host
//! runs on desktop, in the editor (WebAssembly) and on the 3DS).
//!
//! - Scripts are ES modules: `export default class Player extends Behaviour { ... }` (or an
//!   export named like the file). They import the engine from `"goat"` ([`goat.js`](./goat.js))
//!   and each other with relative paths (`import { clamp } from "./util"`).
//! - `.ts` is compiled by oxc when loaded (feature `typescript`); console builds ship the `.js`
//!   produced at build time ([`compile_typescript`]), the loader prefers it when present.
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
}

fn err(msg: impl Into<String>) -> JsError {
    JsNativeError::error().with_message(msg.into()).into()
}

/// Runs `f` with the engine API of the script call in progress.
fn with_api<R>(f: impl FnOnce(&mut Api<'_, '_>, &RefCell<Handles>) -> Result<R, String>) -> JsResult<R> {
    let b = BRIDGE.with(|c| c.get()).ok_or_else(|| err("the engine API is only available while a script runs"))?;
    // SAFETY: the pointers were set by `enter` from live references for the duration of the
    // host call that is running this native function (same thread, nothing else holds them).
    let api = unsafe { &mut *(b.api as *mut Api<'static, 'static>) };
    let handles = unsafe { &*b.handles };
    f(api, handles).map_err(err)
}

fn num(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<f64> {
    args.get(i).cloned().unwrap_or_default().to_number(ctx)
}
fn f(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<f32> {
    num(args, i, ctx).map(|v| v as f32)
}
fn string(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<String> {
    Ok(args.get(i).cloned().unwrap_or_default().to_string(ctx)?.to_std_string_escaped())
}
fn entity(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<Entity> {
    let h = num(args, i, ctx)?;
    with_api(|_, hs| hs.borrow().entity(h).ok_or_else(|| "not a GameObject (destroyed?)".to_string()))
}
fn array(v: &[f32], ctx: &mut Context) -> JsResult<JsValue> {
    JsValue::from_json(&Value::from(v.iter().map(|x| *x as f64).collect::<Vec<_>>()), ctx)
}
fn json(v: &Value, ctx: &mut Context) -> JsResult<JsValue> {
    JsValue::from_json(v, ctx)
}

type Native = fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>;

/// The natives behind goat.js (`globalThis.__goat`).
fn natives() -> Vec<(&'static str, usize, Native)> {
    vec![
        ("find", 1, |_, a, c| {
            let name = string(a, 0, c)?;
            with_api(|api, hs| Ok(api.find(&name).map(|e| hs.borrow_mut().of(e)).unwrap_or(-1.0))).map(JsValue::from)
        }),
        ("exists", 1, |_, a, c| {
            let h = num(a, 0, c)?;
            with_api(|api, hs| Ok(hs.borrow().entity(h).is_some_and(|e| api.exists(e)))).map(JsValue::from)
        }),
        ("name", 1, |_, a, c| {
            let e = entity(a, 0, c)?;
            with_api(|api, _| Ok(api.name(e).unwrap_or_default())).map(|s| JsValue::from(JsString::from(s)))
        }),
        ("setName", 2, |_, a, c| {
            let (e, n) = (entity(a, 0, c)?, string(a, 1, c)?);
            with_api(|api, _| Ok(api.set_name(e, &n))).map(|_| JsValue::undefined())
        }),
        ("spawn", 1, |_, a, c| {
            let n = string(a, 0, c)?;
            with_api(|api, hs| Ok(hs.borrow_mut().of(api.spawn(&n)))).map(JsValue::from)
        }),
        ("destroy", 1, |_, a, c| {
            let e = entity(a, 0, c)?;
            with_api(|api, _| Ok(api.destroy(e))).map(|_| JsValue::undefined())
        }),
        ("parent", 1, |_, a, c| {
            let e = entity(a, 0, c)?;
            with_api(|api, hs| Ok(api.parent(e).map(|p| hs.borrow_mut().of(p)).unwrap_or(-1.0))).map(JsValue::from)
        }),
        ("setParent", 2, |_, a, c| {
            let e = entity(a, 0, c)?;
            let p = num(a, 1, c)?;
            with_api(|api, hs| {
                let p = hs.borrow().entity(p);
                api.set_parent(e, p);
                Ok(())
            })
            .map(|_| JsValue::undefined())
        }),
        ("instantiate", 2, |_, a, c| {
            let key = string(a, 0, c)?;
            let p = num(a, 1, c)?;
            with_api(|api, hs| {
                let parent = hs.borrow().entity(p);
                let e = api.instantiate(&key, parent)?;
                Ok(hs.borrow_mut().of(e))
            })
            .map(JsValue::from)
        }),
        ("getPos", 1, |_, a, c| {
            let e = entity(a, 0, c)?;
            let p = with_api(|api, _| Ok(api.position(e)))?;
            array(&p.to_array(), c)
        }),
        ("setPos", 4, |_, a, c| {
            let e = entity(a, 0, c)?;
            let v = Vec3::new(f(a, 1, c)?, f(a, 2, c)?, f(a, 3, c)?);
            with_api(|api, _| Ok(api.set_position(e, v))).map(|_| JsValue::undefined())
        }),
        ("worldPos", 1, |_, a, c| {
            let e = entity(a, 0, c)?;
            let p = with_api(|api, _| Ok(api.world_position(e)))?;
            array(&p.to_array(), c)
        }),
        ("getEuler", 1, |_, a, c| {
            let e = entity(a, 0, c)?;
            let p = with_api(|api, _| Ok(api.euler(e)))?;
            array(&p.to_array(), c)
        }),
        ("setEuler", 4, |_, a, c| {
            let e = entity(a, 0, c)?;
            let v = Vec3::new(f(a, 1, c)?, f(a, 2, c)?, f(a, 3, c)?);
            with_api(|api, _| Ok(api.set_euler(e, v))).map(|_| JsValue::undefined())
        }),
        ("getRot", 1, |_, a, c| {
            let e = entity(a, 0, c)?;
            let q = with_api(|api, _| Ok(api.rotation(e)))?;
            array(&q.to_array(), c)
        }),
        ("setRot", 5, |_, a, c| {
            let e = entity(a, 0, c)?;
            let q = Quat::from_xyzw(f(a, 1, c)?, f(a, 2, c)?, f(a, 3, c)?, f(a, 4, c)?);
            with_api(|api, _| Ok(api.set_rotation(e, q))).map(|_| JsValue::undefined())
        }),
        ("rotate", 4, |_, a, c| {
            let e = entity(a, 0, c)?;
            let d = Vec3::new(f(a, 1, c)?, f(a, 2, c)?, f(a, 3, c)?);
            with_api(|api, _| {
                let q = api.rotation(e) * Quat::from_euler(EulerRot::YXZ, d.y.to_radians(), d.x.to_radians(), d.z.to_radians());
                api.set_rotation(e, q);
                Ok(())
            })
            .map(|_| JsValue::undefined())
        }),
        ("lookAt", 4, |_, a, c| {
            let e = entity(a, 0, c)?;
            let target = Vec3::new(f(a, 1, c)?, f(a, 2, c)?, f(a, 3, c)?);
            with_api(|api, _| {
                if let Some(mut t) = api.transform(e) {
                    t.look_at(target, Vec3::Y);
                    api.set_transform(e, t);
                }
                Ok(())
            })
            .map(|_| JsValue::undefined())
        }),
        ("eulerToQuat", 3, |_, a, c| {
            let d = Vec3::new(f(a, 0, c)?, f(a, 1, c)?, f(a, 2, c)?);
            array(&Quat::from_euler(EulerRot::YXZ, d.y.to_radians(), d.x.to_radians(), d.z.to_radians()).to_array(), c)
        }),
        ("quatToEuler", 4, |_, a, c| {
            let q = Quat::from_xyzw(f(a, 0, c)?, f(a, 1, c)?, f(a, 2, c)?, f(a, 3, c)?).normalize();
            let (y, x, z) = q.to_euler(EulerRot::YXZ);
            array(&[x.to_degrees(), y.to_degrees(), z.to_degrees()], c)
        }),
        ("getScale", 1, |_, a, c| {
            let e = entity(a, 0, c)?;
            let s = with_api(|api, _| Ok(api.scale(e)))?;
            array(&s.to_array(), c)
        }),
        ("setScale", 4, |_, a, c| {
            let e = entity(a, 0, c)?;
            let v = Vec3::new(f(a, 1, c)?, f(a, 2, c)?, f(a, 3, c)?);
            with_api(|api, _| Ok(api.set_scale(e, v))).map(|_| JsValue::undefined())
        }),
        ("dt", 0, |_, _, _| with_api(|api, _| Ok(api.delta_time() as f64)).map(JsValue::from)),
        ("time", 0, |_, _, _| with_api(|api, _| Ok(api.time() as f64)).map(JsValue::from)),
        ("fixedDt", 0, |_, _, _| with_api(|api, _| Ok(api.fixed_delta_time() as f64)).map(JsValue::from)),
        ("button", 1, |_, a, c| {
            let n = string(a, 0, c)?;
            with_api(|api, _| {
                let (h, p, r) = api.button(&n);
                Ok(h as i32 | (p as i32) << 1 | (r as i32) << 2)
            })
            .map(JsValue::from)
        }),
        ("stick", 0, |_, _, c| {
            let s = with_api(|api, _| Ok(api.stick()))?;
            array(&s.to_array(), c)
        }),
        ("pointer", 0, |_, _, c| match with_api(|api, _| Ok(api.pointer()))? {
            Some((p, down)) => array(&[p.x, p.y, down as u8 as f32], c),
            None => Ok(JsValue::null()),
        }),
        ("action", 1, |_, a, c| {
            let n = string(a, 0, c)?;
            let v = with_api(|api, _| Ok(api.action_value(&n)))?;
            array(&v.to_array(), c)
        }),
        ("actionTriggered", 1, |_, a, c| {
            let n = string(a, 0, c)?;
            with_api(|api, _| Ok(api.action_triggered(&n))).map(JsValue::from)
        }),
        ("actionPressed", 1, |_, a, c| {
            let n = string(a, 0, c)?;
            with_api(|api, _| Ok(api.action_pressed(&n))).map(JsValue::from)
        }),
        ("log", 2, |_, a, c| {
            use goat_core::log::Level;
            let level = match num(a, 0, c)? as i32 {
                3 => Level::Warn,
                4 => Level::Error,
                _ => Level::Info,
            };
            let msg = string(a, 1, c)?;
            with_api(|api, _| Ok(api.log(level, &msg))).map(|_| JsValue::undefined())
        }),
        ("getComponent", 2, |_, a, c| {
            let e = entity(a, 0, c)?;
            let n = string(a, 1, c)?;
            match with_api(|api, _| Ok(api.get_component(e, &n)))? {
                Some(v) => json(&v, c),
                None => Ok(JsValue::null()),
            }
        }),
        ("setComponent", 3, |_, a, c| {
            let e = entity(a, 0, c)?;
            let n = string(a, 1, c)?;
            let v = a.get(2).cloned().unwrap_or_default().to_json(c)?.unwrap_or(Value::Null);
            let v = if v.is_null() { Value::Object(Default::default()) } else { v };
            with_api(|api, _| api.set_component(e, &n, &v)).map(|_| JsValue::undefined())
        }),
        ("removeComponent", 2, |_, a, c| {
            let e = entity(a, 0, c)?;
            let n = string(a, 1, c)?;
            with_api(|api, _| Ok(api.remove_component(e, &n))).map(|_| JsValue::undefined())
        }),
        ("call", 3, |_, a, c| {
            let n = string(a, 0, c)?;
            let e = entity(a, 1, c)?;
            let args = match a.get(2).cloned().unwrap_or_default().to_json(c)? {
                Some(Value::Array(v)) => v,
                _ => Vec::new(),
            };
            let r = with_api(|api, _| api.call(&n, e, &args))?;
            json(&r, c)
        }),
        ("quit", 0, |_, _, _| with_api(|api, _| Ok(api.quit())).map(|_| JsValue::undefined())),
    ]
}

// ------------------------------------------------------------------ modules

/// Resolves `"goat"` and relative imports between scripts (asset keys).
struct Loader {
    io: AssetIo,
    goat: RefCell<Option<Module>>,
    modules: RefCell<HashMap<String, Module>>,
}

/// The JavaScript of a script key: a built `.js` next to a `.ts` wins, else the `.ts` is compiled.
fn script_source(io: &AssetIo, key: &str) -> Result<String, String> {
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
        let code = script_source(&self.io, key).map_err(|e| err(format!("{key}: {e}")))?;
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
        if self.io.exists(&key) {
            return key;
        }
        for ext in [".ts", ".js"] {
            let k = format!("{key}{ext}");
            if self.io.exists(&k) {
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
}

/// TypeScript / JavaScript scripts (`.ts`, `.js`).
pub struct JsHost {
    ctx: Context,
    loader: Rc<Loader>,
    handles: Rc<RefCell<Handles>>,
    classes: HashMap<String, JsObject>,
    instances: HashMap<u32, Instance>,
    next: u32,
    apply_props: Option<JsObject>,
}

impl JsHost {
    /// A host reading scripts through `io` (the game's asset root).
    pub fn new(io: AssetIo) -> Self {
        let loader = Rc::new(Loader { io, goat: RefCell::new(None), modules: RefCell::new(HashMap::new()) });
        let mut ctx = Context::builder().module_loader(loader.clone()).build().expect("a JavaScript context");
        let mut natives = ObjectInitializer::new(&mut ctx);
        for (name, len, f) in natives_list() {
            natives.function(NativeFunction::from_fn_ptr(f), js_string!(name), len);
        }
        let natives = natives.build();
        ctx.register_global_property(js_string!("__goat"), natives, Attribute::all()).expect("__goat");
        JsHost { ctx, loader, handles: Rc::new(RefCell::new(Handles::default())), classes: HashMap::new(), instances: HashMap::new(), next: 0, apply_props: None }
    }

    /// Runs `f` with the bridge to `api` installed.
    fn enter<R>(&mut self, api: &mut Api, f: impl FnOnce(&mut Self) -> R) -> R {
        let bridge = Bridge { api: api as *mut Api as *mut (), handles: Rc::as_ptr(&self.handles) };
        let prev = BRIDGE.with(|c| c.replace(Some(bridge)));
        let r = f(self);
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
        let module = self.loader.module(key, &mut self.ctx).map_err(|e| self.describe(e))?;
        let promise = module.load_link_evaluate(&mut self.ctx);
        self.ctx.run_jobs().map_err(|e| self.describe(e))?;
        match promise.state() {
            PromiseState::Fulfilled(_) => Ok(module.namespace(&mut self.ctx)),
            PromiseState::Rejected(v) => Err(self.describe(JsError::from_opaque(v))),
            PromiseState::Pending => Err("the module didn't finish evaluating (top-level await?)".into()),
        }
    }

    fn class_of(&mut self, src: &str) -> Result<JsObject, String> {
        if let Some(c) = self.classes.get(src) {
            return Ok(c.clone());
        }
        let ns = self.evaluate(src)?;
        let stem = src.rsplit('/').next().unwrap_or(src).split('.').next().unwrap_or(src).to_string();
        let mut class = None;
        for name in ["default", stem.as_str()] {
            let v = ns.get(JsString::from(name), &mut self.ctx).map_err(|e| self.describe(e))?;
            if let Some(o) = v.as_object().filter(|o| o.is_constructor()) {
                class = Some(o);
                break;
            }
        }
        let class = class.ok_or_else(|| format!("{src}: export a class (`export default class {stem} extends Behaviour`)"))?;
        if self.apply_props.is_none() {
            let goat = self.evaluate("goat")?;
            self.apply_props = goat.get(js_string!("__applyProps"), &mut self.ctx).ok().and_then(|v| v.as_object());
        }
        self.classes.insert(src.to_string(), class.clone());
        Ok(class)
    }

    /// Evaluates a snippet (tests, the editor's console).
    pub fn eval(&mut self, api: &mut Api, code: &str) -> Result<Value, String> {
        self.enter(api, |h| {
            let v = h.ctx.eval(Source::from_bytes(code)).map_err(|e| h.describe(e))?;
            v.to_json(&mut h.ctx).map(|v| v.unwrap_or(Value::Null)).map_err(|e| h.describe(e))
        })
    }
}

fn natives_list() -> Vec<(&'static str, usize, Native)> {
    natives()
}

impl ScriptHost for JsHost {
    fn name(&self) -> &str {
        "TypeScript / JavaScript"
    }
    fn handles(&self, src: &str) -> bool {
        src.ends_with(".ts") || src.ends_with(".js") || src.ends_with(".mjs")
    }

    fn instantiate(&mut self, api: &mut Api, entity: Entity, src: &str, props: &Value) -> Result<u32, String> {
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
            h.next += 1;
            h.instances.insert(h.next, Instance { object });
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

    fn invalidate(&mut self, src: &str) {
        self.classes.remove(src);
        self.loader.modules.borrow_mut().remove(src);
    }
}

/// The context a host call gets in Rust tests (no other way to build an `Api`).
pub fn api_of<'a, 'c>(ctx: &'a mut GameContext<'c>) -> Api<'a, 'c> {
    Api::new(ctx)
}

#[cfg(test)]
mod tests;
