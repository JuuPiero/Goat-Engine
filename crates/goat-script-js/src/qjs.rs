//! The same scripts on **QuickJS** (feature `quickjs`): a bytecode interpreter in C, much faster
//! than Boa — what the 3DS and desktop players run. Same `goat.js`, same natives (the
//! engine-neutral list in lib.rs), same [`ScriptHost`] behaviour; the editor (WebAssembly) keeps
//! Boa.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goat_assets::AssetIo;
use goat_runtime::Entity;
use goat_script::{Api, Method, ScriptHost};
use rquickjs::function::{Constructor, Rest};
use rquickjs::loader::{Loader as QjsLoader, Resolver};
use rquickjs::module::Declared;
use rquickjs::{CatchResultExt, Context, Ctx, Function, Module, Object, Persistent, Runtime, Value as JsValue};
use serde_json::Value;

use crate::{asset_file, message_arg, natives, script_source, Bridge, Handles, BRIDGE, GOAT_MODULE};

/// Resolves `"goat"` and relative imports between scripts (asset keys), loads their source.
#[derive(Clone)]
struct ModuleFiles {
    io: Rc<RefCell<Option<AssetIo>>>,
}

impl Resolver for ModuleFiles {
    fn resolve<'js>(&mut self, _ctx: &Ctx<'js>, base: &str, name: &str) -> rquickjs::Result<String> {
        // a reloaded script's fresh module name ("key?v=2")
        if name == "goat" || name.contains("?v=") {
            return Ok(name.into());
        }
        let base = base.split("?v=").next().unwrap_or(base);
        let key = if name.starts_with('.') { goat_assets::resolve_relative(base, name) } else { name.to_string() };
        let Some(io) = self.io.borrow().clone() else { return Ok(key) };
        if io.exists(&key) {
            return Ok(key);
        }
        for ext in [".ts", ".js"] {
            let k = format!("{key}{ext}");
            if io.exists(&k) {
                return Ok(k);
            }
        }
        Ok(key)
    }
}

impl QjsLoader for ModuleFiles {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> rquickjs::Result<Module<'js, Declared>> {
        if name == "goat" {
            return Module::declare(ctx.clone(), "goat", GOAT_MODULE);
        }
        let io = self.io.borrow().clone().ok_or_else(|| rquickjs::Error::new_loading_message(name, "no asset root yet"))?;
        let file = name.split("?v=").next().unwrap_or(name);
        let code = script_source(&io, file).map_err(|e| rquickjs::Error::new_loading_message(name, e))?;
        Module::declare(ctx.clone(), name, code)
    }
}

/// TypeScript / JavaScript / C# scripts on QuickJS.
pub struct QjsHost {
    rt: Runtime,
    ctx: Context,
    files: ModuleFiles,
    handles: Rc<RefCell<Handles>>,
    instances: HashMap<u32, (Persistent<Object<'static>>, String)>,
    classes: Rc<RefCell<HashMap<String, Persistent<Constructor<'static>>>>>,
    assets: Rc<RefCell<HashMap<String, Persistent<Object<'static>>>>>,
    goat: Option<Persistent<Object<'static>>>,
    next: u32,
    /// Module keys evaluated so far: a reloaded script gets a fresh name (QuickJS caches
    /// modules by name for the runtime's life).
    generation: Rc<RefCell<HashMap<String, u32>>>,
}

thread_local! {
    /// What `loadAsset` needs while a QuickJS host runs.
    static QJS: RefCell<Option<QjsShared>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct QjsShared {
    io: Rc<RefCell<Option<AssetIo>>>,
    classes: Rc<RefCell<HashMap<String, Persistent<Constructor<'static>>>>>,
    assets: Rc<RefCell<HashMap<String, Persistent<Object<'static>>>>>,
    generation: Rc<RefCell<HashMap<String, u32>>>,
}

/// JSON → a QuickJS value (numbers / strings / booleans directly, the rest through JSON.parse).
fn to_js<'js>(ctx: &Ctx<'js>, v: &Value) -> rquickjs::Result<JsValue<'js>> {
    Ok(match v {
        Value::Null => JsValue::new_null(ctx.clone()),
        Value::Bool(b) => JsValue::new_bool(ctx.clone(), *b),
        Value::Number(n) => JsValue::new_number(ctx.clone(), n.as_f64().unwrap_or(0.0)),
        Value::String(s) => rquickjs::String::from_str(ctx.clone(), s)?.into_value(),
        _ => ctx.json_parse(serde_json::to_string(v).unwrap_or_default())?,
    })
}

/// A QuickJS value → JSON (undefined = null).
fn from_js<'js>(ctx: &Ctx<'js>, v: &JsValue<'js>) -> rquickjs::Result<Value> {
    if let Some(n) = v.as_number() {
        return Ok(goat_script::json_number(n));
    }
    if let Some(i) = v.as_int() {
        return Ok(Value::from(i));
    }
    if let Some(b) = v.as_bool() {
        return Ok(Value::Bool(b));
    }
    if let Some(s) = v.as_string() {
        return Ok(Value::String(s.to_string()?));
    }
    if v.is_undefined() || v.is_null() || v.is_function() {
        return Ok(Value::Null);
    }
    match ctx.json_stringify(v.clone())? {
        Some(s) => Ok(serde_json::from_str(&s.to_string()?).unwrap_or(Value::Null)),
        None => Ok(Value::Null),
    }
}

fn throw(ctx: &Ctx<'_>, msg: impl AsRef<str>) -> rquickjs::Error {
    rquickjs::Exception::throw_message(ctx, msg.as_ref())
}

/// Runs the pending jobs (promises, async continuations).
fn run_jobs(rt: &Runtime) -> Result<(), String> {
    loop {
        match rt.execute_pending_job() {
            Ok(true) => continue,
            Ok(false) => return Ok(()),
            Err(e) => return Err(format!("{e:?}")),
        }
    }
}

/// The module key of a script (with its reload generation).
fn module_key(generation: &RefCell<HashMap<String, u32>>, src: &str) -> String {
    match generation.borrow().get(src) {
        Some(g) if *g > 0 => format!("{src}?v={g}"),
        _ => src.to_string(),
    }
}

/// The exported class of a script module.
fn class_of<'js>(ctx: &Ctx<'js>, sh: &QjsShared, src: &str) -> rquickjs::Result<Constructor<'js>> {
    if let Some(c) = sh.classes.borrow().get(src).cloned() {
        return c.restore(ctx);
    }
    let key = module_key(&sh.generation, src);
    let promise = Module::import(ctx, key.as_str())?;
    let ns: Object = promise.finish()?;
    let stem = src.rsplit('/').next().unwrap_or(src).split('.').next().unwrap_or(src).to_string();
    let mut class = None;
    for name in ["default", stem.as_str()] {
        if let Ok(f) = ns.get::<_, Constructor>(name) {
            class = Some(f);
            break;
        }
    }
    let class = class.ok_or_else(|| throw(ctx, format!("{src}: export a class (`export default class {stem} extends Behaviour`)")))?;
    sh.classes.borrow_mut().insert(src.to_string(), Persistent::save(ctx, class.clone()));
    Ok(class)
}

fn goat_ns<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Object<'js>> {
    Module::import(ctx, "goat")?.finish()
}

/// `__goat.loadAsset(key)`: a ScriptableObject asset, one shared object per key.
fn load_asset<'js>(ctx: Ctx<'js>, key: String) -> rquickjs::Result<JsValue<'js>> {
    let sh = QJS.with(|s| s.borrow().clone()).ok_or_else(|| throw(&ctx, "no script host is running"))?;
    if let Some(o) = sh.assets.borrow().get(&key).cloned() {
        return Ok(o.restore(&ctx)?.into_value());
    }
    let io = sh.io.borrow().clone().ok_or_else(|| throw(&ctx, "no asset root yet"))?;
    let (script, data) = asset_file(&io, &key).map_err(|e| throw(&ctx, e))?;
    let class = class_of(&ctx, &sh, &script)?;
    let object: Object = class.construct(())?;
    object.set("_key", key.as_str())?;
    sh.assets.borrow_mut().insert(key.clone(), Persistent::save(&ctx, object.clone()));
    if let Some(data) = data {
        let apply: Function = goat_ns(&ctx)?.get("__applyProps")?;
        let d = to_js(&ctx, &data)?;
        apply.call::<_, ()>((object.clone(), d))?;
    }
    if let Ok(f) = object.get::<_, Function>("onEnable") {
        f.call::<_, ()>((rquickjs::function::This(object.clone()),))?;
    }
    Ok(object.into_value())
}

/// `globalThis.__goat`: the engine-neutral natives + `loadAsset`.
fn install_natives<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<()> {
    let natives_obj = Object::new(ctx.clone())?;
    for (name, _len, f) in natives() {
        let func = Function::new(ctx.clone(), move |ctx: Ctx<'js>, args: Rest<JsValue<'js>>| -> rquickjs::Result<JsValue<'js>> {
            let mut vals = Vec::with_capacity(args.0.len());
            for a in &args.0 {
                vals.push(from_js(&ctx, a)?);
            }
            let r = f(&vals).map_err(|e| throw(&ctx, e))?;
            to_js(&ctx, &r)
        })?;
        natives_obj.set(name, func)?;
    }
    let load = Function::new(ctx.clone(), |ctx: Ctx<'js>, key: String| -> rquickjs::Result<JsValue<'js>> { load_asset(ctx, key) })?;
    natives_obj.set("loadAsset", load)?;
    ctx.globals().set("__goat", natives_obj)
}

impl QjsHost {
    /// A host reading scripts from the game's asset root (the AssetServer's).
    pub fn new() -> Self {
        Self::build(None)
    }
    /// A host reading scripts through `io`.
    pub fn with_io(io: AssetIo) -> Self {
        Self::build(Some(io))
    }

    fn build(io: Option<AssetIo>) -> Self {
        let rt = Runtime::new().expect("a QuickJS runtime");
        // the 3DS has little RAM: keep the heap in check, scripts' deep recursion fails cleanly
        rt.set_max_stack_size(256 * 1024);
        let files = ModuleFiles { io: Rc::new(RefCell::new(io)) };
        rt.set_loader(files.clone(), files.clone());
        let ctx = Context::full(&rt).expect("a QuickJS context");
        ctx.with(|ctx| install_natives(&ctx).expect("__goat"));
        QjsHost {
            rt,
            ctx,
            files,
            handles: Rc::new(RefCell::new(Handles::default())),
            instances: HashMap::new(),
            classes: Default::default(),
            assets: Default::default(),
            goat: None,
            next: 0,
            generation: Default::default(),
        }
    }

    fn shared(&self) -> QjsShared {
        QjsShared { io: self.files.io.clone(), classes: self.classes.clone(), assets: self.assets.clone(), generation: self.generation.clone() }
    }

    /// Runs `f` inside the context with the bridge to `api` installed.
    fn enter<R>(&mut self, api: &mut Api, f: impl FnOnce(&Ctx<'_>, &mut Self) -> Result<R, String>) -> Result<R, String> {
        let bridge = Bridge { api: api as *mut Api as *mut (), handles: Rc::as_ptr(&self.handles) };
        let prev = BRIDGE.with(|c| c.replace(Some(bridge)));
        let prev_shared = QJS.with(|s| s.replace(Some(self.shared())));
        let ctx = self.ctx.clone();
        let r = ctx.with(|c| f(&c, self));
        let jobs = run_jobs(&self.rt);
        QJS.with(|s| *s.borrow_mut() = prev_shared);
        BRIDGE.with(|c| c.set(prev));
        let r = r?;
        jobs?;
        Ok(r)
    }

    fn ensure_io(&mut self, api: &mut Api) -> Result<(), String> {
        if self.files.io.borrow().is_none() {
            let io = api.ctx.resources.get::<goat_assets::AssetServer>().map(|a| a.io.clone()).ok_or("no AssetServer: add AssetsPlugin before ScriptPlugin")?;
            *self.files.io.borrow_mut() = Some(io);
        }
        Ok(())
    }

    /// The serialized fields of a script class as a fresh instance has them (see
    /// [`JsHost::defaults`](crate::JsHost::defaults)).
    pub fn defaults(&mut self, api: &mut Api, src: &str) -> Result<Vec<(String, Value)>, String> {
        self.ensure_io(api)?;
        let sh = self.shared();
        self.enter(api, |ctx, h| {
            let class = class_of(ctx, &sh, src).catch(ctx).map_err(|e| e.to_string())?;
            let object: Object = class.construct(()).catch(ctx).map_err(|e| e.to_string())?;
            let fields: Function = h.goat(ctx)?.get("__fields").map_err(|e| e.to_string())?;
            let v: JsValue = fields.call((object,)).catch(ctx).map_err(|e| e.to_string())?;
            let v = from_js(ctx, &v).map_err(|e| e.to_string())?;
            serde_json::from_value(v).map_err(|e| e.to_string())
        })
    }

    /// Whether a script's class extends goat's ScriptableObject.
    pub fn is_scriptable_object(&mut self, api: &mut Api, src: &str) -> Result<bool, String> {
        self.ensure_io(api)?;
        let sh = self.shared();
        self.enter(api, |ctx, h| {
            let class = class_of(ctx, &sh, src).catch(ctx).map_err(|e| e.to_string())?;
            let f: Function = h.goat(ctx)?.get("__isScriptableObject").map_err(|e| e.to_string())?;
            f.call((class,)).catch(ctx).map_err(|e| e.to_string())
        })
    }

    /// Evaluates a snippet (tests, the editor's console).
    pub fn eval(&mut self, api: &mut Api, code: &str) -> Result<Value, String> {
        self.enter(api, |ctx, _| {
            let v: JsValue = ctx.eval(code).catch(ctx).map_err(|e| e.to_string())?;
            from_js(ctx, &v).map_err(|e| e.to_string())
        })
    }

    fn goat<'js>(&mut self, ctx: &Ctx<'js>) -> Result<Object<'js>, String> {
        if let Some(g) = self.goat.clone() {
            return g.restore(ctx).map_err(|e| e.to_string());
        }
        let g = goat_ns(ctx).catch(ctx).map_err(|e| e.to_string())?;
        self.goat = Some(Persistent::save(ctx, g.clone()));
        Ok(g)
    }
}

impl Drop for QjsHost {
    fn drop(&mut self) {
        // every JavaScript value goes before the runtime (QuickJS asserts on leaked objects)
        self.instances.clear();
        self.classes.borrow_mut().clear();
        self.assets.borrow_mut().clear();
        self.goat = None;
        self.ctx.with(|ctx| {
            let _ = ctx.globals().remove("$sel");
            let _ = ctx.globals().remove("$scripts");
        });
        self.rt.run_gc();
    }
}

impl Default for QjsHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptHost for QjsHost {
    fn name(&self) -> &str {
        "TypeScript / JavaScript / C# (QuickJS)"
    }
    fn handles(&self, src: &str) -> bool {
        src.ends_with(".ts") || src.ends_with(".js") || src.ends_with(".mjs") || src.ends_with(".cs")
    }

    fn instantiate(&mut self, api: &mut Api, entity: Entity, src: &str, props: &Value) -> Result<u32, String> {
        self.ensure_io(api)?;
        let sh = self.shared();
        self.enter(api, |ctx, h| {
            let class = class_of(ctx, &sh, src).catch(ctx).map_err(|e| e.to_string())?;
            let object: Object = class.construct(()).catch(ctx).map_err(|e| e.to_string())?;
            let handle = h.handles.borrow_mut().of(entity);
            object.set("_h", handle).map_err(|e| e.to_string())?;
            if !props.is_null() {
                let apply: Function = h.goat(ctx)?.get("__applyProps").map_err(|e| e.to_string())?;
                let p = to_js(ctx, props).map_err(|e| e.to_string())?;
                apply.call::<_, ()>((object.clone(), p)).catch(ctx).map_err(|e| e.to_string())?;
            }
            let track: Function = h.goat(ctx)?.get("__track").map_err(|e| e.to_string())?;
            track.call::<_, ()>((object.clone(),)).catch(ctx).map_err(|e| e.to_string())?;
            h.next += 1;
            h.instances.insert(h.next, (Persistent::save(ctx, object), src.to_string()));
            Ok(h.next)
        })
    }

    fn call(&mut self, api: &mut Api, instance: u32, _entity: Entity, method: Method) -> Result<(), String> {
        let Some((obj, _)) = self.instances.get(&instance).cloned() else { return Ok(()) };
        self.enter(api, |ctx, _| {
            let obj = obj.restore(ctx).map_err(|e| e.to_string())?;
            if let Ok(f) = obj.get::<_, Function>(method.js_name()) {
                f.call::<_, ()>((rquickjs::function::This(obj.clone()),)).catch(ctx).map_err(|e| e.to_string())?;
            }
            Ok(())
        })
    }

    fn destroy(&mut self, instance: u32) {
        self.instances.remove(&instance);
    }

    fn message(&mut self, api: &mut Api, instance: u32, _entity: Entity, name: &str, arg: &Value) -> Result<(), String> {
        let Some((obj, _)) = self.instances.get(&instance).cloned() else { return Ok(()) };
        let name = name.to_string();
        let arg = arg.clone();
        self.enter(api, |ctx, h| {
            let dispatch: Function = h.goat(ctx)?.get("__message").map_err(|e| e.to_string())?;
            let arg = message_arg(&arg, &h.handles);
            let a = to_js(ctx, &arg).map_err(|e| e.to_string())?;
            let o = obj.restore(ctx).map_err(|e| e.to_string())?;
            dispatch.call::<_, ()>((o, name.as_str(), a)).catch(ctx).map_err(|e| e.to_string())?;
            Ok(())
        })
    }

    fn tick(&mut self, api: &mut Api) -> Result<(), String> {
        if self.goat.is_none() {
            return Ok(()); // no script yet
        }
        self.enter(api, |ctx, h| {
            let tick: Function = h.goat(ctx)?.get("__tick").map_err(|e| e.to_string())?;
            tick.call::<_, ()>(()).catch(ctx).map_err(|e| e.to_string())?;
            Ok(())
        })
    }

    fn invalidate(&mut self, src: &str) {
        self.classes.borrow_mut().remove(src);
        *self.generation.borrow_mut().entry(src.to_string()).or_default() += 1;
        self.assets.borrow_mut().clear();
    }

    fn eval(&mut self, api: &mut Api, code: &str, selected: Option<Entity>) -> Result<Value, String> {
        self.ensure_io(api)?;
        self.enter(api, |ctx, h| {
            let f: Function = h.goat(ctx)?.get("__console").map_err(|e| e.to_string())?;
            let handle = selected.map(|e| h.handles.borrow_mut().of(e)).unwrap_or(-1.0);
            let r: JsValue = f.call((handle, code)).catch(ctx).map_err(|e| e.to_string())?;
            Ok(Value::String(r.as_string().and_then(|s| s.to_string().ok()).unwrap_or_default()))
        })
    }

    fn reload(&mut self, api: &mut Api, src: &str) -> Result<usize, String> {
        self.invalidate(src);
        let sh = self.shared();
        let targets: Vec<Persistent<Object<'static>>> = self.instances.values().filter(|(_, s)| s == src).map(|(o, _)| o.clone()).collect();
        if targets.is_empty() {
            return Ok(0);
        }
        self.enter(api, |ctx, _| {
            let class = class_of(ctx, &sh, src).catch(ctx).map_err(|e| e.to_string())?;
            let proto: Object = class.get("prototype").map_err(|e| e.to_string())?;
            for o in &targets {
                // the running objects keep their state and get the new methods
                let o = o.clone().restore(ctx).map_err(|e| e.to_string())?;
                o.set_prototype(Some(&proto)).map_err(|e| e.to_string())?;
            }
            Ok(targets.len())
        })
    }
}

/// QuickJS's `pthread_mutex_init(m, NULL)` (renamed by the 3DS CFLAGS in tools/goat n3ds.rs):
/// pthread-3ds reads `*attr` without a NULL check (a read of address 0: a data abort on
/// hardware), so NULL becomes an explicit default (normal) attribute.
#[cfg(target_os = "horizon")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn goat_pthread_mutex_init(lock: *mut core::ffi::c_void, attr: *const core::ffi::c_int) -> core::ffi::c_int {
    unsafe extern "C" {
        fn pthread_mutex_init(lock: *mut core::ffi::c_void, attr: *const core::ffi::c_int) -> core::ffi::c_int;
    }
    // PTHREAD_MUTEX_NORMAL (pthread-3ds reads the first int of the attribute: the kind)
    let normal: [core::ffi::c_int; 2] = [0, 0];
    // SAFETY: forwards to pthread-3ds with a valid attribute.
    unsafe { pthread_mutex_init(lock, if attr.is_null() { normal.as_ptr() } else { attr }) }
}

/// `pthread_once` for QuickJS's one-time initialisations on the 3DS: the pthread-3ds crate
/// (Rust's std threads) doesn't provide it, and newlib's would pull a second, clashing pthread
/// implementation in. Scripts run on one thread: run once, mark done (status 2, newlib's layout).
#[cfg(target_os = "horizon")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_once(once: *mut i32, init: Option<unsafe extern "C" fn()>) -> i32 {
    // SAFETY: `once` points at a pthread_once_t ({ int status }) owned by QuickJS.
    unsafe {
        if once.is_null() {
            return 22; // EINVAL
        }
        if *once == 0 {
            *once = 1;
            if let Some(f) = init {
                f();
            }
            *once = 2;
        }
    }
    0
}