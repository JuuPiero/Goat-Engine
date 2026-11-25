//! Script natives of the low-level graphics API (Unity / PiaEngine names): Graphics.DrawMesh(
//! Instanced), Graphics.Blit, GL (vertices buffered in the script, one call per `GL.End`),
//! Debug.DrawLine, RenderTexture, Texture2D pixels (rows bottom-up like Unity), PNG encoding,
//! MaterialPropertyBlock colors and textures on renderer materials.
//!
//! Textures are named by asset key everywhere (materials, Camera target texture, Raw Image):
//! textures made by scripts get `runtime:` keys registered in the AssetServer.

use base64::Engine as _;
use goat_assets::AssetServer;
use goat_core::math::{Mat4, Quat, Vec3};
use goat_core::Color;
use goat_gfx::{GraphicsBackend, MaterialId, MeshId, RenderTargetDesc, TextureData, TextureId};
use goat_runtime::{GlMode, GlSpace, GraphicsQueue, MeshRenderer, PropertyBlock};
use goat_runtime::Entity;
use serde_json::{json, Value};

use crate::{Api, Extensions};

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn f(v: Option<&Value>, d: f32) -> f32 {
    v.and_then(Value::as_f64).map(|x| x as f32).unwrap_or(d)
}

fn vec3(v: Option<&Value>) -> Option<Vec3> {
    let a = v?.as_array()?;
    Some(Vec3::new(f(a.first(), 0.0), f(a.get(1), 0.0), f(a.get(2), 0.0)))
}

fn color(v: Option<&Value>) -> Color {
    match v.and_then(Value::as_array) {
        Some(a) => Color::rgba(f(a.first(), 1.0), f(a.get(1), 1.0), f(a.get(2), 1.0), f(a.get(3), 1.0)),
        None => v.and_then(Value::as_str).and_then(goat_core::parse_hex).unwrap_or(Color::WHITE),
    }
}

fn assets<'x>(api: &'x mut Api) -> Result<&'x mut AssetServer, String> {
    api.ctx.resources.get_mut::<AssetServer>().ok_or_else(|| "no asset server".to_string())
}

/// The asset server and the graphics device together (distinct parts of the context).
fn with_assets<R>(api: &mut Api, f: impl FnOnce(&mut AssetServer, &mut dyn GraphicsBackend) -> R) -> Result<R, String> {
    let srv = api.ctx.resources.get_mut::<AssetServer>().ok_or("no asset server")?;
    Ok(f(srv, &mut *api.ctx.gfx))
}

/// `'Cube'` / `'Sphere'`… = the built-in primitives (like PiaEngine), else a mesh key.
fn mesh_of(api: &mut Api, v: Option<&Value>) -> Result<MeshId, String> {
    let name = v.and_then(Value::as_str).ok_or("a mesh: 'Cube', 'Sphere', 'Quad', 'Plane', 'Capsule', 'Cylinder' or a mesh key")?;
    let key = match name {
        "Cube" | "Sphere" | "Quad" | "Plane" | "Capsule" | "Cylinder" => format!("builtin:{}", name.to_lowercase()),
        k => k.to_string(),
    };
    with_assets(api, |s, g| s.mesh(g, &key))?.map_err(|e| e.to_string())
}

fn material_of(api: &mut Api, v: Option<&Value>) -> Result<MaterialId, String> {
    let key = match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => "builtin:default".into(),
        _ => return Err("a material key".into()),
    };
    with_assets(api, |s, g| s.material(g, &key))?.map_err(|e| e.to_string())
}

fn texture_of(api: &mut Api, v: Option<&Value>) -> Result<Option<TextureId>, String> {
    let Some(key) = v.and_then(Value::as_str).filter(|k| !k.is_empty()) else { return Ok(None) };
    with_assets(api, |s, g| s.texture(g, key))?.map(Some).map_err(|e| e.to_string())
}

fn queue<'x>(api: &'x mut Api) -> &'x mut GraphicsQueue {
    api.ctx.resources.get_or_default::<GraphicsQueue>()
}

/// A counter for `runtime:` keys.
#[derive(Default)]
struct RuntimeKeys(u32);

fn new_key(api: &mut Api, kind: &str) -> String {
    let n = api.ctx.resources.get_or_default::<RuntimeKeys>();
    n.0 += 1;
    format!("runtime:{kind}/{}", n.0)
}

/// Rows top-down ↔ bottom-up (the engine's TextureData vs Unity's Texture2D).
fn flip_rows(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let row = (w * 4) as usize;
    let mut out = Vec::with_capacity(rgba.len());
    for y in (0..h as usize).rev() {
        out.extend_from_slice(&rgba[y * row..(y + 1) * row]);
    }
    out
}

// ------------------------------------------------------------------ Graphics

/// `[mesh, material, position, rotation [x,y,z,w], scale, color, layer]`
fn draw_mesh(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let mesh = mesh_of(api, a.first())?;
    let material = material_of(api, a.get(1))?;
    let p = vec3(a.get(2)).unwrap_or(Vec3::ZERO);
    let r = a.get(3).and_then(Value::as_array).map(|q| Quat::from_xyzw(f(q.first(), 0.0), f(q.get(1), 0.0), f(q.get(2), 0.0), f(q.get(3), 1.0))).unwrap_or(Quat::IDENTITY);
    let s = vec3(a.get(4)).unwrap_or(Vec3::ONE);
    let c = color(a.get(5));
    let layer = f(a.get(6), 0.0) as u8;
    queue(api).draw_mesh(mesh, material, Mat4::from_scale_rotation_translation(s, r.normalize(), p), c, layer, None);
    Ok(Value::Null)
}

/// `[mesh, material, matrices (16 numbers per instance, column-major), colors (4 per instance) | null, layer]`
fn draw_mesh_instanced(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let mesh = mesh_of(api, a.first())?;
    let material = material_of(api, a.get(1))?;
    let m: Vec<f32> = a.get(2).and_then(Value::as_array).map(|l| l.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default();
    let matrices: Vec<Mat4> = m.chunks_exact(16).map(|c| Mat4::from_cols_slice(c)).collect();
    let colors: Option<Vec<Color>> = a.get(3).and_then(Value::as_array).map(|l| {
        let v: Vec<f32> = l.iter().map(|x| x.as_f64().unwrap_or(1.0) as f32).collect();
        v.chunks_exact(4).map(|c| Color::rgba(c[0], c[1], c[2], c[3])).collect()
    });
    let layer = f(a.get(4), 0.0) as u8;
    queue(api).draw_mesh_instanced(mesh, material, &matrices, colors.as_deref(), layer);
    Ok(json!(matrices.len()))
}

/// `[sourceKey, destKey | null, materialKey | null]`
fn blit(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let src = texture_of(api, a.first())?.ok_or("Graphics.blit(source, dest, material): no source")?;
    let dst = texture_of(api, a.get(1))?;
    let mat = match a.get(2).and_then(Value::as_str) {
        Some(k) => Some(material_of(api, Some(&Value::String(k.into())))?),
        None => None,
    };
    queue(api).blit(src, dst, mat);
    Ok(Value::Null)
}

/// `GL.end`: `[mode, space, [x, y, z, r, g, b, a, …]]` (vertices already through the script's matrix).
fn gl_draw(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let mode = match a.first().and_then(Value::as_str).unwrap_or("") {
        "LINES" => GlMode::Lines,
        "LINE_STRIP" => GlMode::LineStrip,
        "TRIANGLES" => GlMode::Triangles,
        "TRIANGLE_STRIP" => GlMode::TriangleStrip,
        "QUADS" => GlMode::Quads,
        m => return Err(format!("GL.begin: unknown mode {m}")),
    };
    let space = match a.get(1).and_then(Value::as_str).unwrap_or("world") {
        "ortho" => GlSpace::Screen01,
        "pixels" => GlSpace::Pixels,
        _ => GlSpace::World,
    };
    let v: Vec<f32> = a.get(2).and_then(Value::as_array).map(|l| l.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default();
    let q = queue(api);
    q.gl_push_matrix();
    q.gl_load_space(space);
    q.gl_begin(mode);
    for c in v.chunks_exact(7) {
        q.gl_color(Color::rgba(c[3], c[4], c[5], c[6]));
        q.gl_vertex(Vec3::new(c[0], c[1], c[2]));
    }
    q.gl_end();
    q.gl_pop_matrix();
    Ok(Value::Null)
}

/// `[from, to, color, duration, depthTest]`
fn draw_line(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let (p, q) = (vec3(a.first()).unwrap_or(Vec3::ZERO), vec3(a.get(1)).unwrap_or(Vec3::ZERO));
    let c = color(a.get(2));
    let duration = f(a.get(3), 0.0);
    let depth = a.get(4).and_then(Value::as_bool).unwrap_or(true);
    queue(api).debug_line(p, q, c, duration, depth);
    Ok(Value::Null)
}

// ------------------------------------------------------------------ RenderTexture / Texture2D

/// `[width, height, depth]` → key
fn rt_create(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let (w, h) = (f(a.first(), 256.0).max(1.0) as u32, f(a.get(1), 256.0).max(1.0) as u32);
    let depth = f(a.get(2), 24.0) > 0.0;
    let id = api.ctx.gfx.create_render_target(&RenderTargetDesc { width: w, height: h, depth }).map_err(|e| e.to_string())?;
    let key = new_key(api, "rendertexture");
    let srv = assets(api)?;
    srv.register_texture(&key, id);
    srv.set_texture_size(id, (w, h));
    Ok(json!(key))
}

/// `[key]`: frees a texture made by a script (RenderTexture.release / Destroy(texture)).
fn tex_release(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let key = a.first().and_then(Value::as_str).unwrap_or("");
    if !key.starts_with("runtime:") {
        return Ok(Value::Null);
    }
    if let Some(id) = assets(api)?.forget_texture(key) {
        api.ctx.gfx.destroy_texture(id);
    }
    Ok(Value::Null)
}

/// `[key]` → `{ width, height }`
fn tex_size(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let id = texture_of(api, a.first())?.ok_or("texture")?;
    let (w, h) = api.ctx.gfx.texture_size(id).or_else(|| api.ctx.resources.get::<AssetServer>().and_then(|s| s.texture_size(id))).unwrap_or((0, 0));
    Ok(json!({ "width": w, "height": h }))
}

/// `Texture2D.apply`: `[key | null, width, height, base64 RGBA rows bottom-up, filter]` → key
fn tex_apply(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let (w, h) = (f(a.get(1), 1.0).max(1.0) as u32, f(a.get(2), 1.0).max(1.0) as u32);
    let bytes = b64().decode(a.get(3).and_then(Value::as_str).unwrap_or("")).map_err(|e| e.to_string())?;
    if bytes.len() != (w * h * 4) as usize {
        return Err(format!("Texture2D.apply: {} bytes for {w}×{h}", bytes.len()));
    }
    let mut data = TextureData::rgba8(w, h, flip_rows(w, h, &bytes));
    if a.get(4).and_then(Value::as_str) == Some("point") {
        data.filter = goat_gfx::Filter::Nearest;
    }
    if a.get(5).and_then(Value::as_str) == Some("repeat") {
        data.wrap = goat_gfx::Wrap::Repeat;
    }
    let existing = a.first().and_then(Value::as_str).filter(|k| k.starts_with("runtime:")).map(str::to_string);
    if let Some(key) = &existing {
        if let Some(id) = lookup(api, key) {
            if api.ctx.gfx.texture_size(id) == Some((w, h)) && api.ctx.gfx.update_texture(id, &data).is_ok() {
                return Ok(json!(key));
            }
            // size changed: a new GPU texture under the same key
            let srv = assets(api)?;
            srv.forget_texture(key);
            api.ctx.gfx.destroy_texture(id);
        }
    }
    let id = api.ctx.gfx.create_texture(&data).map_err(|e| e.to_string())?;
    let key = existing.unwrap_or_else(|| new_key(api, "texture2d"));
    let srv = assets(api)?;
    srv.register_texture(&key, id);
    srv.set_texture_size(id, (w, h));
    Ok(json!(key))
}

/// A texture already registered under `key` (no loading).
fn lookup(api: &mut Api, key: &str) -> Option<TextureId> {
    // runtime keys are registered: `texture` returns the cached id without touching files
    with_assets(api, |s, g| s.texture(g, key).ok()).ok().flatten()
}

fn pixels_json(w: u32, h: u32, top_down: &[u8]) -> Value {
    json!({ "width": w, "height": h, "data": b64().encode(flip_rows(w, h, top_down)) })
}

/// `[key]` → `{ width, height, data }` (rows bottom-up): a texture file's pixels (Read/Write) or
/// what a render texture holds (ReadPixels).
fn tex_read(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let key = a.first().and_then(Value::as_str).ok_or("a texture key")?.to_string();
    let is_target = key.starts_with("runtime:rendertexture/") || key.ends_with(".renderTexture.json");
    if is_target || key.starts_with("runtime:") {
        let id = texture_of(api, Some(&Value::String(key.clone())))?.ok_or("texture")?;
        let t = api.ctx.gfx.read_pixels(id).map_err(|e| e.to_string())?;
        return Ok(pixels_json(t.width, t.height, &t.rgba));
    }
    let t = assets(api)?.texture_data(&key).map_err(|e| e.to_string())?;
    Ok(pixels_json(t.width, t.height, &t.rgba))
}

/// `[base64 file bytes]` → `{ width, height, data }` (LoadImage: PNG / JPEG).
fn tex_decode(_: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let bytes = b64().decode(a.first().and_then(Value::as_str).unwrap_or("")).map_err(|e| e.to_string())?;
    let t = if bytes.starts_with(&[0xFF, 0xD8]) { goat_assets::image::decode_jpeg(&bytes)? } else { goat_assets::image::decode_png(&bytes)? };
    Ok(pixels_json(t.width, t.height, &t.rgba))
}

/// `[width, height, base64 RGBA bottom-up]` → base64 PNG (EncodeToPNG).
fn tex_encode_png(_: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let (w, h) = (f(a.first(), 1.0) as u32, f(a.get(1), 1.0) as u32);
    let bytes = b64().decode(a.get(2).and_then(Value::as_str).unwrap_or("")).map_err(|e| e.to_string())?;
    if bytes.len() != (w * h * 4) as usize {
        return Err("encodeToPNG: size".into());
    }
    let png = goat_assets::image::encode_png(w, h, &flip_rows(w, h, &bytes))?;
    Ok(json!(b64().encode(png)))
}

// ------------------------------------------------------------------ renderers

/// `Renderer.setPropertyBlock([color] | [])`: an empty block removes it.
fn set_property_block(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    if api.ctx.world.get::<&MeshRenderer>(e).is_err() {
        return Err("no MeshRenderer on this object".into());
    }
    match a.first().filter(|v| !v.is_null()) {
        Some(c) => {
            let _ = api.ctx.world.insert_one(e, PropertyBlock { color: color(Some(c)) });
        }
        None => {
            let _ = api.ctx.world.remove_one::<PropertyBlock>(e);
        }
    }
    Ok(Value::Null)
}

/// `Renderer.propertyBlock` → color array or null.
fn get_property_block(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    Ok(api.ctx.world.get::<&PropertyBlock>(e).map(|b| json!(b.color.to_array())).unwrap_or(Value::Null))
}

/// `Renderer.setTexture([name, key | null])`: material.mainTexture / SetTexture (own copy).
fn set_texture(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let name = a.first().and_then(Value::as_str).unwrap_or("_MainTex").to_string();
    let tex = texture_of(api, a.get(1))?;
    let (copy, original) = crate::core_api::own_material_pub(api, e)?;
    let mut desc = api.ctx.gfx.material_desc(copy).ok_or("material")?;
    let shader = api.ctx.resources.get_mut::<AssetServer>().and_then(|s| s.material_shader(original));
    match shader {
        Some(sh) => {
            let t = sh.textures.iter().find(|t| t.name == name).ok_or_else(|| format!("{} has no texture {name}", sh.name))?;
            let old = desc.shader_textures[t.unit];
            desc.shader_textures[t.unit] = tex;
            if desc.texture == old {
                desc.texture = tex;
            }
        }
        None => match name.as_str() {
            "_MainTex" | "_BaseMap" | "mainTexture" => desc.texture = tex,
            "_EmissionMap" => desc.emissive_texture = tex,
            other => return Err(format!("the standard material has no texture {other} (_MainTex, _EmissionMap)")),
        },
    }
    api.ctx.gfx.update_material(copy, &desc);
    Ok(Value::Null)
}

/// `Renderer.getTexture([name])` → key or null.
fn get_texture(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    let m = api.ctx.world.get::<&MeshRenderer>(e).map_err(|_| "no MeshRenderer on this object")?.material;
    let t = api.ctx.gfx.material_desc(m).and_then(|d| d.texture);
    Ok(t.and_then(|t| api.ctx.resources.get::<AssetServer>().and_then(|s| s.key_of_texture(t)).map(|k| json!(k))).unwrap_or(Value::Null))
}

pub fn register(x: &mut Extensions) {
    x.add("Graphics.drawMesh", draw_mesh)
        .add("Graphics.drawMeshInstanced", draw_mesh_instanced)
        .add("Graphics.blit", blit)
        .add("GL.draw", gl_draw)
        .add("Debug.drawLine", draw_line)
        .add("RenderTexture.create", rt_create)
        .add("Texture.release", tex_release)
        .add("Texture.size", tex_size)
        .add("Texture2D.apply", tex_apply)
        .add("Texture.read", tex_read)
        .add("Texture2D.decode", tex_decode)
        .add("Texture2D.encodePNG", tex_encode_png)
        .add("Renderer.setPropertyBlock", set_property_block)
        .add("Renderer.propertyBlock", get_property_block)
        .add("Renderer.setTexture", set_texture)
        .add("Renderer.getTexture", get_texture);
}
