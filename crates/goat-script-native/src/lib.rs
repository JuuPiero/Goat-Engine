//! Native scripts through a C ABI ([`include/goat.h`](../include/goat.h)).
//!
//! C / C++ code registers behaviour classes (tables of callbacks) with
//! `goat_register_behaviour`; scenes attach them as `"native:<Name>"`. The code is linked into
//! the game statically (a `build.rs` with the `cc` crate — the 3DS can't load libraries at run
//! time), the engine calls it through [`NativeHost`], and it calls the engine back through the
//! `goat_*` functions below, which forward to the shared [`goat_script::Api`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::Mutex;

use goat_core::math::{EulerRot, Quat, Vec3};
use goat_runtime::Entity;
use goat_script::{Api, Method, ScriptHost};
use serde_json::Value;

/// `GoatEntity`
pub type GoatEntity = u32;
pub const GOAT_NONE: GoatEntity = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GoatVec2 {
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GoatVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl From<Vec3> for GoatVec3 {
    fn from(v: Vec3) -> Self {
        GoatVec3 { x: v.x, y: v.y, z: v.z }
    }
}
impl From<GoatVec3> for Vec3 {
    fn from(v: GoatVec3) -> Self {
        Vec3::new(v.x, v.y, v.z)
    }
}

type Create = unsafe extern "C" fn(*mut GoatApi, GoatEntity, *const c_char) -> *mut c_void;
type Callback = unsafe extern "C" fn(*mut c_void, *mut GoatApi, GoatEntity);
type Free = unsafe extern "C" fn(*mut c_void);

/// `GoatBehaviourClass`
#[repr(C)]
pub struct GoatBehaviourClass {
    pub name: *const c_char,
    pub create: Option<Create>,
    pub start: Option<Callback>,
    pub update: Option<Callback>,
    pub fixed_update: Option<Callback>,
    pub late_update: Option<Callback>,
    pub on_destroy: Option<Callback>,
    pub free_state: Option<Free>,
}

/// A registered class (the pointer stays valid: classes are statics of the native code).
#[derive(Clone, Copy)]
struct ClassPtr(*const GoatBehaviourClass);
// SAFETY: the class tables are immutable statics.
unsafe impl Send for ClassPtr {}

static CLASSES: Mutex<Vec<(String, ClassPtr)>> = Mutex::new(Vec::new());

/// Registers a behaviour class (from C: `goat_register_behaviour(&MY_CLASS)`).
///
/// # Safety
/// `cls` must point to a class table that lives for the whole program, with a valid C-string name.
#[no_mangle]
pub unsafe extern "C" fn goat_register_behaviour(cls: *const GoatBehaviourClass) {
    if cls.is_null() || (*cls).name.is_null() {
        return;
    }
    let name = CStr::from_ptr((*cls).name).to_string_lossy().into_owned();
    if let Ok(mut c) = CLASSES.lock() {
        c.retain(|(n, _)| *n != name);
        c.push((name, ClassPtr(cls)));
    }
}

/// Names of the registered native behaviours.
pub fn registered() -> Vec<String> {
    CLASSES.lock().map(|c| c.iter().map(|(n, _)| n.clone()).collect()).unwrap_or_default()
}

fn class(name: &str) -> Option<ClassPtr> {
    CLASSES.lock().ok()?.iter().find(|(n, _)| n == name).map(|(_, c)| *c)
}

// ------------------------------------------------------------------ GoatApi

/// What a `GoatApi*` points to during a callback.
pub struct GoatApi {
    api: *mut (),
    handles: *const RefCell<Handles>,
}

#[derive(Default)]
struct Handles {
    list: Vec<Entity>,
    map: HashMap<Entity, u32>,
}

impl Handles {
    fn of(&mut self, e: Entity) -> GoatEntity {
        if let Some(h) = self.map.get(&e) {
            return *h;
        }
        let h = self.list.len() as u32;
        self.list.push(e);
        self.map.insert(e, h);
        h
    }
    fn entity(&self, h: GoatEntity) -> Option<Entity> {
        if h == GOAT_NONE {
            return None;
        }
        self.list.get(h as usize).copied()
    }
}

/// The engine behind a `GoatApi*` (None for a null / stale pointer).
unsafe fn parts<'x>(g: *mut GoatApi) -> Option<(&'x mut Api<'static, 'static>, &'x RefCell<Handles>)> {
    if g.is_null() {
        return None;
    }
    let g = &*g;
    Some((&mut *(g.api as *mut Api<'static, 'static>), &*g.handles))
}

unsafe fn text<'x>(s: *const c_char) -> &'x str {
    if s.is_null() {
        ""
    } else {
        CStr::from_ptr(s).to_str().unwrap_or("")
    }
}

/// Copies `s` into a C buffer (truncated, always terminated); returns the full length.
unsafe fn write(s: &str, buf: *mut c_char, cap: c_int) -> c_int {
    if !buf.is_null() && cap > 0 {
        let n = s.len().min(cap as usize - 1);
        std::ptr::copy_nonoverlapping(s.as_ptr() as *const c_char, buf, n);
        *buf.add(n) = 0;
    }
    s.len() as c_int
}

macro_rules! with {
    ($g:expr, $default:expr, |$api:ident, $hs:ident| $body:expr) => {{
        match parts($g) {
            Some(($api, $hs)) => {
                #[allow(unused_variables)]
                let $hs = $hs;
                $body
            }
            None => $default,
        }
    }};
}
macro_rules! entity {
    ($hs:expr, $e:expr, $default:expr) => {
        match $hs.borrow().entity($e) {
            Some(e) => e,
            None => return $default,
        }
    };
}

/// # Safety
/// Called by native scripts with the `GoatApi*` of the running callback.
#[no_mangle]
pub unsafe extern "C" fn goat_find(g: *mut GoatApi, name: *const c_char) -> GoatEntity {
    with!(g, GOAT_NONE, |api, hs| api.find(text(name)).map(|e| hs.borrow_mut().of(e)).unwrap_or(GOAT_NONE))
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_spawn(g: *mut GoatApi, name: *const c_char) -> GoatEntity {
    with!(g, GOAT_NONE, |api, hs| {
        let e = api.spawn(text(name));
        hs.borrow_mut().of(e)
    })
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_instantiate(g: *mut GoatApi, key: *const c_char, parent: GoatEntity) -> GoatEntity {
    with!(g, GOAT_NONE, |api, hs| {
        let p = hs.borrow().entity(parent);
        match api.instantiate(text(key), p) {
            Ok(e) => hs.borrow_mut().of(e),
            Err(err) => {
                goat_core::error!("goat_instantiate {}: {err}", text(key));
                GOAT_NONE
            }
        }
    })
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_destroy(g: *mut GoatApi, e: GoatEntity) {
    with!(g, (), |api, hs| {
        let e = entity!(hs, e, ());
        api.destroy(e)
    })
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_exists(g: *mut GoatApi, e: GoatEntity) -> c_int {
    with!(g, 0, |api, hs| hs.borrow().entity(e).is_some_and(|e| api.exists(e)) as c_int)
}
/// # Safety
/// See [`goat_find`]; `buf` holds `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn goat_get_name(g: *mut GoatApi, e: GoatEntity, buf: *mut c_char, cap: c_int) -> c_int {
    with!(g, -1, |api, hs| {
        let e = entity!(hs, e, -1);
        write(&api.name(e).unwrap_or_default(), buf, cap)
    })
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_set_name(g: *mut GoatApi, e: GoatEntity, name: *const c_char) {
    with!(g, (), |api, hs| {
        let e = entity!(hs, e, ());
        api.set_name(e, text(name))
    })
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_get_parent(g: *mut GoatApi, e: GoatEntity) -> GoatEntity {
    with!(g, GOAT_NONE, |api, hs| {
        let e = entity!(hs, e, GOAT_NONE);
        api.parent(e).map(|p| hs.borrow_mut().of(p)).unwrap_or(GOAT_NONE)
    })
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_set_parent(g: *mut GoatApi, e: GoatEntity, parent: GoatEntity) {
    with!(g, (), |api, hs| {
        let e = entity!(hs, e, ());
        let p = hs.borrow().entity(parent);
        api.set_parent(e, p)
    })
}

macro_rules! vec3_get {
    ($name:ident, $method:ident) => {
        /// # Safety
        /// See [`goat_find`].
        #[no_mangle]
        pub unsafe extern "C" fn $name(g: *mut GoatApi, e: GoatEntity) -> GoatVec3 {
            with!(g, GoatVec3::default(), |api, hs| {
                let e = entity!(hs, e, GoatVec3::default());
                api.$method(e).into()
            })
        }
    };
}
macro_rules! vec3_set {
    ($name:ident, $method:ident) => {
        /// # Safety
        /// See [`goat_find`].
        #[no_mangle]
        pub unsafe extern "C" fn $name(g: *mut GoatApi, e: GoatEntity, v: GoatVec3) {
            with!(g, (), |api, hs| {
                let e = entity!(hs, e, ());
                api.$method(e, v.into())
            })
        }
    };
}
vec3_get!(goat_get_position, position);
vec3_set!(goat_set_position, set_position);
vec3_get!(goat_get_euler, euler);
vec3_set!(goat_set_euler, set_euler);
vec3_get!(goat_get_scale, scale);
vec3_set!(goat_set_scale, set_scale);
vec3_get!(goat_get_world_position, world_position);

/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_rotate(g: *mut GoatApi, e: GoatEntity, d: GoatVec3) {
    with!(g, (), |api, hs| {
        let e = entity!(hs, e, ());
        let q = api.rotation(e) * Quat::from_euler(EulerRot::YXZ, d.y.to_radians(), d.x.to_radians(), d.z.to_radians());
        api.set_rotation(e, q)
    })
}

/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_delta_time(g: *mut GoatApi) -> f32 {
    with!(g, 0.0, |api, hs| api.delta_time())
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_time(g: *mut GoatApi) -> f32 {
    with!(g, 0.0, |api, hs| api.time())
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_unscaled_delta_time(g: *mut GoatApi) -> f32 {
    with!(g, 0.0, |api, hs| api.unscaled_delta_time())
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_time_scale(g: *mut GoatApi) -> f32 {
    with!(g, 1.0, |api, hs| api.time_scale())
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_set_time_scale(g: *mut GoatApi, scale: f32) {
    with!(g, (), |api, hs| api.set_time_scale(scale))
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_button(g: *mut GoatApi, name: *const c_char) -> c_int {
    with!(g, 0, |api, hs| {
        let (h, p, r) = api.button(text(name));
        h as c_int | (p as c_int) << 1 | (r as c_int) << 2
    })
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_stick(g: *mut GoatApi) -> GoatVec2 {
    with!(g, GoatVec2::default(), |api, hs| {
        let s = api.stick();
        GoatVec2 { x: s.x, y: s.y }
    })
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_action_value(g: *mut GoatApi, action: *const c_char) -> GoatVec2 {
    with!(g, GoatVec2::default(), |api, hs| {
        let v = api.action_value(text(action));
        GoatVec2 { x: v.x, y: v.y }
    })
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_action_triggered(g: *mut GoatApi, action: *const c_char) -> c_int {
    with!(g, 0, |api, hs| api.action_triggered(text(action)) as c_int)
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_log(g: *mut GoatApi, level: c_int, msg: *const c_char) {
    use goat_core::log::Level;
    let level = match level {
        3 => Level::Warn,
        4 => Level::Error,
        _ => Level::Info,
    };
    with!(g, (), |api, hs| api.log(level, text(msg)))
}
/// # Safety
/// See [`goat_find`]; `buf` holds `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn goat_get_component(g: *mut GoatApi, e: GoatEntity, name: *const c_char, buf: *mut c_char, cap: c_int) -> c_int {
    with!(g, -1, |api, hs| {
        let e = entity!(hs, e, -1);
        match api.get_component(e, text(name)) {
            Some(v) => write(&v.to_string(), buf, cap),
            None => -1,
        }
    })
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_set_component(g: *mut GoatApi, e: GoatEntity, name: *const c_char, json: *const c_char) -> c_int {
    with!(g, 1, |api, hs| {
        let e = entity!(hs, e, 1);
        let v: Value = match serde_json::from_str(text(json)) {
            Ok(v) => v,
            Err(err) => {
                goat_core::error!("goat_set_component {}: {err}", text(name));
                return 1;
            }
        };
        match api.set_component(e, text(name), &v) {
            Ok(()) => 0,
            Err(err) => {
                goat_core::error!("goat_set_component {}: {err}", text(name));
                1
            }
        }
    })
}
/// # Safety
/// See [`goat_find`].
#[no_mangle]
pub unsafe extern "C" fn goat_remove_component(g: *mut GoatApi, e: GoatEntity, name: *const c_char) {
    with!(g, (), |api, hs| {
        let e = entity!(hs, e, ());
        api.remove_component(e, text(name))
    })
}
/// # Safety
/// See [`goat_find`]; `out` holds `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn goat_call(g: *mut GoatApi, name: *const c_char, e: GoatEntity, args: *const c_char, out: *mut c_char, cap: c_int) -> c_int {
    with!(g, -1, |api, hs| {
        let e = entity!(hs, e, -1);
        let args: Vec<Value> = serde_json::from_str(text(args)).unwrap_or_default();
        match api.call(text(name), e, &args) {
            Ok(v) => write(&v.to_string(), out, cap),
            Err(err) => {
                goat_core::error!("goat_call {}: {err}", text(name));
                -1
            }
        }
    })
}

// ------------------------------------------------------------------ the host

struct Instance {
    class: ClassPtr,
    state: *mut c_void,
}

/// Runs `native:<Name>` scripts.
#[derive(Default)]
pub struct NativeHost {
    handles: RefCell<Handles>,
    instances: HashMap<u32, Instance>,
    next: u32,
}

impl NativeHost {
    pub fn new() -> Self {
        Self::default()
    }
    fn goat_api(&self, api: &mut Api) -> GoatApi {
        GoatApi { api: api as *mut Api as *mut (), handles: &self.handles }
    }
}

impl ScriptHost for NativeHost {
    fn name(&self) -> &str {
        "native (C ABI)"
    }
    fn handles(&self, src: &str) -> bool {
        src.starts_with("native:")
    }
    fn instantiate(&mut self, api: &mut Api, entity: Entity, src: &str, props: &Value) -> Result<u32, String> {
        let name = &src["native:".len()..];
        let cls = class(name).ok_or_else(|| format!("no native behaviour {name} registered (have: {})", registered().join(", ")))?;
        let handle = self.handles.borrow_mut().of(entity);
        let props = CString::new(props.to_string()).unwrap_or_default();
        let mut g = self.goat_api(api);
        // SAFETY: the class table is a static of the native code; `g` lives for the call.
        let state = unsafe { (*cls.0).create.map(|f| f(&mut g, handle, props.as_ptr())).unwrap_or(std::ptr::null_mut()) };
        self.next += 1;
        self.instances.insert(self.next, Instance { class: cls, state });
        Ok(self.next)
    }
    fn call(&mut self, api: &mut Api, instance: u32, entity: Entity, method: Method) -> Result<(), String> {
        let Some(inst) = self.instances.get(&instance) else { return Ok(()) };
        let (cls, state) = (inst.class, inst.state);
        let handle = self.handles.borrow_mut().of(entity);
        let mut g = self.goat_api(api);
        // SAFETY: as in `instantiate`; `state` is what this class's `create` returned.
        unsafe {
            let c = &*cls.0;
            let f = match method {
                Method::Start => c.start,
                Method::Update => c.update,
                Method::FixedUpdate => c.fixed_update,
                Method::LateUpdate => c.late_update,
                Method::OnDestroy => c.on_destroy,
                Method::Awake | Method::OnEnable | Method::OnDisable => None,
            };
            if let Some(f) = f {
                f(state, &mut g, handle);
            }
        }
        Ok(())
    }
    fn destroy(&mut self, instance: u32) {
        if let Some(inst) = self.instances.remove(&instance) {
            // SAFETY: frees the state with the class's own function, once.
            unsafe {
                if let Some(f) = (*inst.class.0).free_state {
                    f(inst.state);
                }
            }
        }
    }
}

#[cfg(all(test, feature = "examples"))]
mod tests;
