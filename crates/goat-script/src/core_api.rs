//! Engine calls behind the core script API (Unity's names, every language): Transform in world
//! space and the hierarchy, Screen, Camera (main, ScreenPointToRay, WorldToScreenPoint) and
//! Renderer materials (`renderer.material`: the first write gives the object its own copy of the
//! material, then colors / floats / vectors by property name — a Goat shader's properties, or the
//! standard material's `_Color` / `_EmissionColor`).

use std::collections::HashMap;

use goat_core::math::{Quat, Vec2, Vec3};
use goat_runtime::{Camera, Entity, GlobalTransform, MeshRenderer, Name, Parent, Transform};
use serde_json::{json, Value};

use crate::{entity_json, Api, Extensions};

fn f(a: &[Value], i: usize) -> f32 {
    a.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32
}
fn arr(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

/// The world matrix from the local transforms up the parent chain (current even before this
/// frame's transform propagation: scripts move things in Start / Update).
fn world_matrix(api: &Api, e: Entity) -> goat_core::math::Mat4 {
    let w = &api.ctx.world;
    let mut m = goat_core::math::Mat4::IDENTITY;
    let mut cur = Some(e);
    for _ in 0..256 {
        let Some(x) = cur else { break };
        match w.get::<&Transform>(x) {
            Ok(t) => m = t.matrix() * m,
            Err(_) => {
                if let Ok(g) = w.get::<&GlobalTransform>(x) {
                    m = g.0 * m;
                }
                break;
            }
        }
        cur = w.get::<&Parent>(x).ok().map(|p| p.0);
    }
    m
}

fn parent_world(api: &Api, e: Entity) -> goat_core::math::Mat4 {
    match api.ctx.world.get::<&Parent>(e).map(|p| p.0) {
        Ok(p) => world_matrix(api, p),
        Err(_) => goat_core::math::Mat4::IDENTITY,
    }
}

/// `{ position, rotation: [x, y, z, w], lossyScale }` in world space.
fn world(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    // the parent chain may have moved this frame: compose from the local transforms
    let m = parent_world(api, e) * api.ctx.world.get::<&Transform>(e).map_err(|_| "no Transform")?.matrix();
    let (s, r, p) = m.to_scale_rotation_translation();
    Ok(json!({ "position": arr(p), "rotation": [r.x, r.y, r.z, r.w], "lossyScale": arr(s) }))
}

fn set_world_position(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let local = parent_world(api, e).inverse().transform_point3(Vec3::new(f(a, 0), f(a, 1), f(a, 2)));
    if let Ok(mut t) = api.ctx.world.get::<&mut Transform>(e) {
        t.position = local;
    }
    Ok(Value::Null)
}

fn set_world_rotation(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let (_, pr, _) = parent_world(api, e).to_scale_rotation_translation();
    let q = Quat::from_xyzw(f(a, 0), f(a, 1), f(a, 2), f(a, 3)).normalize();
    if let Ok(mut t) = api.ctx.world.get::<&mut Transform>(e) {
        t.rotation = (pr.inverse() * q).normalize();
    }
    Ok(Value::Null)
}

fn children_of(api: &Api, e: Entity) -> Vec<Entity> {
    let mut c: Vec<Entity> = api.ctx.world.query::<&Parent>().iter().filter(|(_, p)| p.0 == e).map(|(c, _)| c).collect();
    c.sort_by_key(|x| x.id());
    c
}

fn children(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    Ok(Value::Array(children_of(api, e).into_iter().map(entity_json).collect()))
}

/// Unity's Transform.Find: a child by name, or a path of names ("Arm/Hand").
fn find_child(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let path = a.first().and_then(Value::as_str).unwrap_or("");
    let mut cur = e;
    for part in path.split('/').filter(|p| !p.is_empty()) {
        let next = children_of(api, cur).into_iter().find(|c| api.ctx.world.get::<&Name>(*c).is_ok_and(|n| n.0 == part));
        match next {
            Some(n) => cur = n,
            None => return Ok(Value::Null),
        }
    }
    Ok(entity_json(cur))
}

fn root(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    let mut cur = e;
    while let Ok(p) = api.ctx.world.get::<&Parent>(cur).map(|p| p.0) {
        cur = p;
    }
    Ok(entity_json(cur))
}

/// `{ width, height, displays: [{ width, height, stereo }] }` (the main display first).
fn screen(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    let d = api.ctx.displays;
    let main = d.iter().find(|x| x.display == goat_gfx::Display::Main).or(d.first());
    Ok(json!({
        "width": main.map(|m| m.width).unwrap_or(0),
        "height": main.map(|m| m.height).unwrap_or(0),
        "displays": d.iter().map(|x| json!({ "width": x.width, "height": x.height, "stereo": x.stereo })).collect::<Vec<_>>(),
        "slider3D": api.ctx.input.slider_3d,
    }))
}

/// Unity's Camera.main: the camera tagged MainCamera, else the first camera of the main display.
fn main_camera(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    let w = &api.ctx.world;
    let mut cams: Vec<(Entity, bool, i32)> = w
        .query::<&Camera>()
        .without::<&goat_runtime::InactiveInHierarchy>()
        .iter()
        .map(|(e, c)| (e, w.get::<&goat_runtime::Tag>(e).is_ok_and(|t| t.0 == "MainCamera") || c.display == goat_gfx::Display::Main, c.depth))
        .collect();
    cams.sort_by_key(|(e, main, depth)| (!*main, *depth, e.id()));
    Ok(cams.first().map(|(e, _, _)| entity_json(*e)).unwrap_or(Value::Null))
}

fn screen_point_to_ray(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let r = goat_runtime::screen_point_to_ray(api.ctx.world, api.ctx.displays, e, Vec2::new(f(a, 0), f(a, 1))).ok_or("not a camera on a display")?;
    Ok(json!({ "origin": arr(r.origin), "direction": arr(r.direction) }))
}

/// Unity's WorldToScreenPoint: [x, y (pixels, top-left), z (distance along the view)].
fn world_to_screen_point(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let w = &api.ctx.world;
    let c = w.get::<&Camera>(e).map_err(|_| "not a camera")?;
    let g = w.get::<&GlobalTransform>(e).map_err(|_| "no transform")?.0;
    let info = api.ctx.displays.iter().find(|d| d.display == c.display).ok_or("no display")?;
    let (dw, dh) = (info.width as f32, info.height as f32);
    let vp = c.viewport;
    let (vx, vy, vw, vh) = (vp.x * dw, vp.y * dh, (vp.width * dw).max(1.0), (vp.height * dh).max(1.0));
    let view = g.inverse();
    let pv = view.transform_point3(Vec3::new(f(a, 0), f(a, 1), f(a, 2)));
    let clip = c.projection.gl_matrix(vw / vh) * pv.extend(1.0);
    if clip.w.abs() < 1e-6 {
        return Ok(json!([0.0, 0.0, -pv.z]));
    }
    let (nx, ny) = (clip.x / clip.w, clip.y / clip.w);
    Ok(json!([vx + (nx * 0.5 + 0.5) * vw, vy + (0.5 - ny * 0.5) * vh, -pv.z]))
}

/// Objects given their own copy of a material (`renderer.material`): entity → (copy, original).
#[derive(Default)]
pub struct MaterialInstances(pub HashMap<Entity, (goat_gfx::MaterialId, goat_gfx::MaterialId)>);

fn value4(v: &Value) -> Option<[f32; 4]> {
    match v {
        Value::Number(n) => Some([n.as_f64()? as f32, 0.0, 0.0, 0.0]),
        Value::Bool(b) => Some([*b as u8 as f32, 0.0, 0.0, 0.0]),
        Value::Array(l) => {
            let mut a = [0.0, 0.0, 0.0, 1.0];
            for (i, x) in l.iter().take(4).enumerate() {
                a[i] = x.as_f64()? as f32;
            }
            Some(a)
        }
        Value::String(s) => goat_core::parse_hex(s).map(|c| c.to_array()),
        _ => None,
    }
}

/// For graphics_api.rs (material textures).
pub(crate) fn own_material_pub(api: &mut Api, e: Entity) -> Result<(goat_gfx::MaterialId, goat_gfx::MaterialId), String> {
    own_material(api, e)
}

/// The object's own material (made on first use) and the material it was copied from.
fn own_material(api: &mut Api, e: Entity) -> Result<(goat_gfx::MaterialId, goat_gfx::MaterialId), String> {
    if let Some(m) = api.ctx.resources.get::<MaterialInstances>().and_then(|m| m.0.get(&e).copied()) {
        return Ok(m);
    }
    let shared = api.ctx.world.get::<&MeshRenderer>(e).map_err(|_| "no MeshRenderer on this object")?.material;
    let desc = api.ctx.gfx.material_desc(shared).ok_or("the renderer's material isn't known")?;
    let copy = api.ctx.gfx.create_material(&desc).map_err(|e| e.to_string())?;
    if let Ok(mut r) = api.ctx.world.get::<&mut MeshRenderer>(e) {
        r.material = copy;
    }
    api.ctx.resources.get_or_default::<MaterialInstances>().0.insert(e, (copy, shared));
    Ok((copy, shared))
}

/// `Renderer.setProperty(name, value)`: Material.SetColor / SetFloat / SetVector.
fn set_property(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let name = a.first().and_then(Value::as_str).ok_or("setProperty(name, value)")?.to_string();
    let v = a.get(1).and_then(value4).ok_or("a number, [x, y, z, w] or a color")?;
    let (copy, original) = own_material(api, e)?;
    let mut desc = api.ctx.gfx.material_desc(copy).ok_or("material")?;
    let shader = api.ctx.resources.get_mut::<goat_assets::AssetServer>().and_then(|a| a.material_shader(original));
    let color = goat_core::Color::rgba(v[0], v[1], v[2], v[3]);
    match shader {
        Some(sh) => {
            let p = sh.params.iter().find(|p| p.name == name).ok_or_else(|| format!("{} has no property {name}", sh.name))?;
            desc.shader_params[p.slot] = v;
            // what the 3DS draws: the standard mapping, or the combiner program planned again
            if sh.standard.color.as_deref() == Some(name.as_str()) {
                desc.base_color = color;
            }
            if sh.standard.emission.as_deref() == Some(name.as_str()) {
                desc.emissive = color;
            }
            if desc.tev.is_some() {
                let params = desc.shader_params;
                desc.tev = sh.tev(|n| sh.params.iter().find(|p| p.name == n).map(|p| params[p.slot])).ok();
            }
        }
        None => match name.as_str() {
            "_Color" | "_BaseColor" | "color" => desc.base_color = color,
            "_EmissionColor" | "emissive" => desc.emissive = color,
            "_Glossiness" | "_Shininess" | "shininess" => desc.shininess = v[0],
            "_Cutoff" => desc.alpha_cutoff = v[0].clamp(0.0, 1.0),
            other => return Err(format!("the standard material has no property {other} (_Color, _EmissionColor, _Shininess, _Cutoff)")),
        },
    }
    api.ctx.gfx.update_material(copy, &desc);
    Ok(Value::Null)
}

/// `Renderer.getProperty(name)` → [x, y, z, w].
fn get_property(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let name = a.first().and_then(Value::as_str).unwrap_or("");
    let id = api.ctx.world.get::<&MeshRenderer>(e).map_err(|_| "no MeshRenderer on this object")?.material;
    let original = api.ctx.resources.get::<MaterialInstances>().and_then(|m| m.0.get(&e).map(|x| x.1)).unwrap_or(id);
    let desc = api.ctx.gfx.material_desc(id).ok_or("material")?;
    let shader = api.ctx.resources.get_mut::<goat_assets::AssetServer>().and_then(|a| a.material_shader(original));
    let v = match shader.as_ref().and_then(|sh| sh.params.iter().find(|p| p.name == name)) {
        Some(p) => desc.shader_params[p.slot],
        None => match name {
            "_EmissionColor" | "emissive" => desc.emissive.to_array(),
            "_Glossiness" | "_Shininess" | "shininess" => [desc.shininess, 0.0, 0.0, 0.0],
            _ => desc.base_color.to_array(),
        },
    };
    Ok(json!(v))
}

fn set_visible(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let on = a.first().and_then(Value::as_bool).unwrap_or(true);
    api.ctx.world.get::<&mut MeshRenderer>(e).map_err(|_| "no MeshRenderer on this object")?.visible = on;
    Ok(Value::Null)
}

fn is_visible(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(api.ctx.world.get::<&MeshRenderer>(e).is_ok_and(|r| r.visible)))
}


/// `{ platform, editor }`.
fn application(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    let platform = if cfg!(target_os = "horizon") {
        "3DS"
    } else if cfg!(target_arch = "wasm32") {
        "Web"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    };
    let editor = api.ctx.resources.get::<EditorMarker>().is_some();
    let s = api.ctx.resources.get::<goat_runtime::ProjectSettings>();
    let (name, version) = s.map(|s| (s.name.clone(), s.version.clone())).unwrap_or_default();
    Ok(json!({ "platform": platform, "editor": editor, "productName": name, "version": version }))
}

/// Present while the game runs inside the editor (Application.isEditor).
#[derive(Default)]
pub struct EditorMarker;

/// `[names, includeInactive]` → the objects having one of these components (Unity's
/// FindObjectsByType), in creation order.
fn find_with_component(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let names: Vec<String> = a.first().and_then(Value::as_array).map(|l| l.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let inactive = a.get(1).and_then(Value::as_bool).unwrap_or(false);
    let mut list: Vec<Entity> = api
        .ctx
        .world
        .query::<()>()
        .without::<&goat_runtime::DontSave>()
        .iter()
        .map(|(e, _)| e)
        .filter(|e| inactive || api.ctx.world.get::<&goat_runtime::InactiveInHierarchy>(*e).is_err())
        .collect();
    list.sort_by_key(|e| e.id());
    let found: Vec<Value> = list.into_iter().filter(|e| names.iter().any(|n| api.has_component(*e, n))).map(crate::entity_json).collect();
    Ok(Value::Array(found))
}

/// Seconds of a high-resolution clock (ProfilerMarker / Stopwatch-like timings).
fn profiler_now(_: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    Ok(json!(goat_runtime::profiler::now()))
}
/// `[name, seconds]`: adds a script's sample to this frame's profiler section (Profiler window).
fn profiler_sample(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let name = a.first().and_then(Value::as_str).unwrap_or("Script");
    let s = a.get(1).and_then(Value::as_f64).unwrap_or(0.0);
    api.ctx.resources.get_or_default::<goat_runtime::Profiler>().add(name, s);
    Ok(Value::Null)
}
/// `{ drawCalls, triangles, sections: { name: ms of the last frame } }` (RenderStats, ProfilerRecorder).
fn profiler_stats(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    let p = api.ctx.resources.get_or_default::<goat_runtime::Profiler>();
    Ok(json!({ "drawCalls": p.draw_calls, "triangles": p.triangles, "sections": p.last }))
}

/// `[code]` → bits: 1 held, 2 went down, 4 went up (Input.GetKey / GetKeyDown / GetKeyUp).
fn input_key(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let code = a.first().and_then(Value::as_str).unwrap_or("");
    let i = api.ctx.input;
    Ok(json!(i.key(code) as u8 | (i.key_pressed(code) as u8) << 1 | (i.key_released(code) as u8) << 2))
}

/// `{ mouse: [bits per button], scroll, anyKey, anyKeyDown, text }` (Input.GetMouseButton...).
fn input_state(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    let i = api.ctx.input;
    let mouse: Vec<u8> = (0..3).map(|b| i.mouse_button(b) as u8 | (i.mouse_pressed(b) as u8) << 1 | (i.mouse_released(b) as u8) << 2).collect();
    let any_button = goat_runtime::Button::ALL.iter().any(|b| i.held(*b));
    let any_button_down = goat_runtime::Button::ALL.iter().any(|b| i.pressed(*b));
    Ok(json!({
        "mouse": mouse,
        "scroll": [i.scroll.x, i.scroll.y],
        "anyKey": !i.keys.is_empty() || any_button || i.mouse_buttons.iter().any(|b| *b),
        "anyKeyDown": i.any_key_pressed() || any_button_down || (0..3).any(|b| i.mouse_pressed(b)),
        "text": i.text,
    }))
}

/// `[src, on?]` (entity): the `enabled` of the entity's script `src` (Behaviour.enabled); with `on`,
/// sets it (OnEnable / OnDisable follow at the next update).
fn script_enabled(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let src = a.first().and_then(Value::as_str).unwrap_or("").to_string();
    let mut s = api.ctx.world.get::<&mut crate::Script>(e).map_err(|_| "no Script on this object".to_string())?;
    let r = s.0.iter_mut().find(|r| r.src == src).ok_or("no such script on this object")?;
    if let Some(on) = a.get(1).and_then(Value::as_bool) {
        r.enabled = on;
    }
    Ok(Value::Bool(r.enabled))
}

/// (entity, `[parent | null]`) → a copy of the object and its children (Unity's Instantiate of a
/// scene object), named "<name>(Clone)"; scripts start again from their saved values.
fn clone_object(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let parent = a.first().and_then(crate::entity_arg).filter(|p| api.ctx.world.contains(*p));
    let registry = api.ctx.resources.take::<goat_scene::SceneRegistry>().ok_or("no scene registry")?;
    let result = (|| {
        let assets = api.ctx.resources.get_mut::<goat_assets::AssetServer>().ok_or("no asset server")?;
        let file = goat_scene::save_entities(api.ctx.world, assets, &registry, &[e]);
        goat_scene::spawn_file(api.ctx.world, api.ctx.gfx, assets, &registry, &file, "instantiate", parent).map_err(|e| e.to_string())
    })();
    api.ctx.resources.put_back(registry);
    let roots = result?;
    let root = *roots.first().ok_or("nothing was copied")?;
    if let Ok(mut n) = api.ctx.world.get::<&mut goat_runtime::Name>(root) {
        n.0 = format!("{}(Clone)", n.0);
    }
    Ok(crate::entity_json(root))
}

/// (entity) → the names of the components it has (getComponent of components without fields,
/// getComponents).
fn component_names(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    let names: Vec<String> = match api.ctx.resources.get::<goat_scene::SceneRegistry>() {
        Some(r) => r.components().iter().map(|c| c.name.to_string()).collect(),
        None => Vec::new(),
    };
    Ok(Value::Array(names.into_iter().filter(|n| api.has_component(e, n)).map(Value::String).collect()))
}

pub fn register(x: &mut Extensions) {
    x.add("Application.info", application);
    x.add("GameObject.componentNames", component_names);
    x.add("Input.key", input_key).add("Input.state", input_state);
    x.add("Script.enabled", script_enabled);
    x.add("GameObject.clone", clone_object);
    x.add("Profiler.now", profiler_now).add("Profiler.sample", profiler_sample).add("Profiler.stats", profiler_stats);
    x.add("GameObject.findWithComponent", find_with_component);
    x.add("Transform.world", world)
        .add("Transform.setWorldPosition", set_world_position)
        .add("Transform.setWorldRotation", set_world_rotation)
        .add("Transform.children", children)
        .add("Transform.find", find_child)
        .add("Transform.root", root)
        .add("Screen.info", screen)
        .add("Camera.main", main_camera)
        .add("Camera.screenPointToRay", screen_point_to_ray)
        .add("Camera.worldToScreenPoint", world_to_screen_point)
        .add("Renderer.setProperty", set_property)
        .add("Renderer.getProperty", get_property)
        .add("Renderer.setEnabled", set_visible)
        .add("Renderer.enabled", is_visible);
}
