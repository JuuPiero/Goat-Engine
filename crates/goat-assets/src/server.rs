//! [`AssetServer`]: loads assets by key once, keeps their GPU handles, maps handles back to keys.

use std::collections::HashMap;

use goat_gfx::{GfxError, GraphicsBackend, MaterialDesc, MaterialId, MeshData, MeshId, TextureData, TextureId};
use goat_runtime::{Context, Entity, GlobalTransform, MeshFilter, MeshRenderer, Name, Parent, Plugin, Transform, World};

use crate::image::{decode_png, to_power_of_two, TextureImport};
use crate::material::MaterialFile;
use crate::model::{load_model, ModelData};
use crate::{ext_of, file_of, resolve_relative, AssetError, AssetIo};

/// A model's GPU resources, by glTF mesh / primitive and material index.
pub struct LoadedModel {
    pub data: ModelData,
    pub meshes: Vec<Vec<MeshId>>,
    pub materials: Vec<MaterialId>,
    /// Skin data per (mesh, skin), made when an instance is spawned (shared by every instance).
    pub skins: HashMap<(usize, usize), std::sync::Arc<goat_runtime::SkinData>>,
}

/// On a model instance's root: the entity of each glTF node (animation targets, skin joints).
#[derive(Clone, Debug, Default)]
pub struct ModelNodes(pub Vec<Option<Entity>>);

/// Put on the root entity of a spawned model (scenes save the reference, not the hierarchy).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInstance {
    pub key: String,
}

/// `.renderTexture.json` (the fields of Unity's Render Texture asset that matter here).
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RenderTextureAsset {
    pub width: u32,
    pub height: u32,
    /// Depth buffer bits: 0 = none (2D), 16 / 24.
    pub depth: u32,
}

impl Default for RenderTextureAsset {
    fn default() -> Self {
        RenderTextureAsset { width: 256, height: 256, depth: 24 }
    }
}

pub struct AssetServer {
    pub io: AssetIo,
    textures: HashMap<String, TextureId>,
    materials: HashMap<String, MaterialId>,
    /// Compiled `.shader` files (errors kept: reported once) and their GPU programs (None: the
    /// backend can't run custom shaders, or it rejected this one).
    /// Compiled shaders by `path` (default variant) or `path|KW1 KW2` (a keyword variant).
    shaders: HashMap<String, Result<std::sync::Arc<goat_shader::Shader>, String>>,
    /// GPU programs by `variant key#pass`.
    gpu_shaders: HashMap<String, Option<goat_gfx::ShaderId>>,
    /// The materials of a multi-pass material's later passes (by material key).
    pass_materials: HashMap<String, Vec<MaterialId>>,
    /// The keywords a material's shader variant was made with (by material key).
    material_keywords: HashMap<String, Vec<String>>,
    /// Material file → the shader it uses (reloading a shader updates its materials).
    material_shaders: HashMap<String, String>,
    meshes: HashMap<String, MeshId>,
    models: HashMap<String, LoadedModel>,
    texture_keys: HashMap<TextureId, String>,
    material_keys: HashMap<MaterialId, String>,
    mesh_keys: HashMap<MeshId, String>,
    texture_sizes: HashMap<TextureId, (u32, u32)>,
}

impl AssetServer {
    pub fn new(io: AssetIo) -> Self {
        AssetServer {
            io,
            textures: HashMap::new(),
            materials: HashMap::new(),
            shaders: HashMap::new(),
            gpu_shaders: HashMap::new(),
            pass_materials: HashMap::new(),
            material_keywords: HashMap::new(),
            material_shaders: HashMap::new(),
            meshes: HashMap::new(),
            models: HashMap::new(),
            texture_keys: HashMap::new(),
            material_keys: HashMap::new(),
            mesh_keys: HashMap::new(),
            texture_sizes: HashMap::new(),
        }
    }

    // ------------------------------------------------------------------ keys of handles

    pub fn key_of_texture(&self, id: TextureId) -> Option<&str> {
        self.texture_keys.get(&id).map(String::as_str)
    }
    pub fn key_of_material(&self, id: MaterialId) -> Option<&str> {
        self.material_keys.get(&id).map(String::as_str)
    }
    /// Pixel size of a texture loaded through the server (as authored, before GPU resizing).
    pub fn texture_size(&self, id: TextureId) -> Option<(u32, u32)> {
        self.texture_sizes.get(&id).copied()
    }
    pub fn key_of_mesh(&self, id: MeshId) -> Option<&str> {
        self.mesh_keys.get(&id).map(String::as_str)
    }
    /// The key of a [`MeshFilter`]'s mesh: a one-submesh mesh's own key, or `<model>#mesh:<m>`
    /// when the submeshes are all the primitives of a model mesh.
    pub fn key_of_submeshes(&self, submeshes: &[MeshId]) -> Option<String> {
        match submeshes {
            [] => None,
            [one] => self.key_of_mesh(*one).map(str::to_string),
            many => self.models.iter().find_map(|(file, m)| m.meshes.iter().position(|ids| ids.as_slice() == many).map(|mi| format!("{file}#mesh:{mi}"))),
        }
    }

    /// Names resources made in code (`runtime:...`) so scenes can refer to them.
    pub fn register_texture(&mut self, key: &str, id: TextureId) {
        self.textures.insert(key.to_string(), id);
        self.texture_keys.insert(id, key.to_string());
    }
    /// Drops a key made with `register_texture` (the texture was destroyed).
    pub fn forget_texture(&mut self, key: &str) -> Option<TextureId> {
        let id = self.textures.remove(key)?;
        self.texture_keys.remove(&id);
        self.texture_sizes.remove(&id);
        Some(id)
    }
    /// Size of a texture made in code (`register_texture` doesn't know it).
    pub fn set_texture_size(&mut self, id: TextureId, size: (u32, u32)) {
        self.texture_sizes.insert(id, size);
    }

    pub fn register_material(&mut self, key: &str, id: MaterialId) {
        self.materials.insert(key.to_string(), id);
        self.material_keys.insert(id, key.to_string());
    }
    pub fn register_mesh(&mut self, key: &str, id: MeshId) {
        self.meshes.insert(key.to_string(), id);
        self.mesh_keys.insert(id, key.to_string());
    }

    // ------------------------------------------------------------------ loading

    /// CPU pixels of a texture file with its import settings applied.
    pub fn texture_data(&self, key: &str) -> Result<TextureData, AssetError> {
        let key: &str = &self.io.canonical(key);
        if key == "builtin:white" {
            // 8×8 white, 8 pixels per unit by convention: tinted sprites / UI rectangles
            return Ok(TextureData::checker(8, 1, goat_core::Color::WHITE, goat_core::Color::WHITE));
        }
        // shader property defaults (Unity's "black" / "gray" / "bump" / "red")
        let solid = match key {
            "builtin:black" => Some(goat_core::Color::rgba(0.0, 0.0, 0.0, 1.0)),
            "builtin:gray" | "builtin:grey" => Some(goat_core::Color::rgba(0.5, 0.5, 0.5, 1.0)),
            "builtin:bump" => Some(goat_core::Color::rgba(0.5, 0.5, 1.0, 1.0)),
            "builtin:red" => Some(goat_core::Color::rgba(1.0, 0.0, 0.0, 1.0)),
            _ => None,
        };
        if let Some(c) = solid {
            return Ok(TextureData::checker(8, 1, c, c));
        }
        let bytes = self.io.read(key)?;
        // the bytes decide (builds store precompressed KTX under the source name)
        let sniffed = if bytes.starts_with(&[0xAB, b'K', b'T', b'X']) {
            "ktx".to_string()
        } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
            "png".to_string()
        } else if bytes.starts_with(&[0xFF, 0xD8]) {
            "jpg".to_string()
        } else {
            ext_of(key)
        };
        let t = match sniffed.as_str() {
            "png" => decode_png(&bytes).map_err(|e| AssetError::Format(key.into(), e))?,
            "jpg" | "jpeg" => crate::image::decode_jpeg(&bytes).map_err(|e| AssetError::Format(key.into(), e))?,
            "ktx" => crate::image::decode_ktx(&bytes).map_err(|e| AssetError::Format(key.into(), e))?,
            other => return Err(AssetError::Format(key.into(), format!("unsupported image format .{other} (PNG, JPEG, KTX)"))),
        };
        let import_key = format!("{key}.import.json");
        let import: TextureImport = match self.io.read_string(&import_key) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| AssetError::Format(import_key, e.to_string()))?,
            Err(_) => TextureImport::default(),
        };
        Ok(import.apply(t))
    }

    /// Uploads a texture; sizes the GPU can't take (3DS: powers of two) are resized.
    fn upload_texture(gfx: &mut dyn GraphicsBackend, key: &str, t: &TextureData) -> Result<TextureId, AssetError> {
        match gfx.create_texture(t) {
            Ok(id) => Ok(id),
            Err(GfxError::Unsupported(_)) => gfx.create_texture(&to_power_of_two(t)).map_err(|e| AssetError::Gpu(key.into(), e)),
            Err(e) => Err(AssetError::Gpu(key.into(), e)),
        }
    }

    pub fn texture(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Result<TextureId, AssetError> {
        let key: &str = &self.io.canonical(key);
        if let Some(id) = self.textures.get(key) {
            return Ok(*id);
        }
        if let Some((file, sub)) = key.split_once('#') {
            // model sub-texture: "<model>#texture:<material>"
            let material: usize = sub.strip_prefix("texture:").and_then(|s| s.parse().ok()).ok_or_else(|| AssetError::Format(key.into(), "bad sub-asset".into()))?;
            self.model(gfx, file)?;
            let m = &self.models[file].data;
            let mat = m.materials.get(material).ok_or_else(|| AssetError::Format(key.into(), "no such material".into()))?;
            let img = mat.image.and_then(|i| m.images.get(i).cloned().flatten()).ok_or_else(|| AssetError::Format(key.into(), "material has no image".into()))?;
            let t = TextureData { filter: mat.filter, wrap: mat.wrap, ..img };
            let id = Self::upload_texture(gfx, key, &t)?;
            self.register_texture(key, id);
            self.texture_sizes.insert(id, (t.width, t.height));
            return Ok(id);
        }
        if key.ends_with(".renderTexture.json") {
            return self.render_texture(gfx, key);
        }
        let t = self.texture_data(key)?;
        let id = Self::upload_texture(gfx, key, &t)?;
        self.register_texture(key, id);
        self.texture_sizes.insert(id, (t.width, t.height));
        Ok(id)
    }

    /// A `.renderTexture.json` asset (Unity's Render Texture: `{ "width": 256, "height": 256,
    /// "depth": 16 }`): a texture cameras draw into and materials / Raw Images show. One GPU
    /// target per asset, shared by everything that names it.
    pub fn render_texture(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Result<TextureId, AssetError> {
        let key: &str = &self.io.canonical(key);
        if let Some(id) = self.textures.get(key) {
            return Ok(*id);
        }
        let text = self.io.read_string(key)?;
        let desc: RenderTextureAsset = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| AssetError::Format(key.into(), e.to_string()))?;
        let (w, h) = (desc.width.clamp(1, 4096), desc.height.clamp(1, 4096));
        let id = gfx.create_render_target(&goat_gfx::RenderTargetDesc { width: w, height: h, depth: desc.depth > 0 }).map_err(|e| AssetError::Gpu(key.into(), e))?;
        self.register_texture(key, id);
        self.texture_sizes.insert(id, (w, h));
        Ok(id)
    }

    /// A cube map: a `.cubemap.json` file (`{ "faces": ["px.png", "nx.png", "py.png", "ny.png",
    /// "pz.png", "nz.png"] }`, relative to the file) or `builtin:sky` (a soft blue sky over a
    /// darker ground, for quick shiny materials).
    pub fn cubemap(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Result<TextureId, AssetError> {
        let key: &str = &self.io.canonical(key);
        if let Some(id) = self.textures.get(key) {
            return Ok(*id);
        }
        let faces: [TextureData; 6] = if key == "builtin:sky" {
            std::array::from_fn(sky_face)
        } else {
            let text = self.io.read_string(key)?;
            let j: serde_json::Value = serde_json::from_str(&text).map_err(|e| AssetError::Format(key.into(), e.to_string()))?;
            let list: Vec<String> = j.get("faces").and_then(|f| f.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(|s| resolve_relative(key, s))).collect()).unwrap_or_default();
            if list.len() != 6 {
                return Err(AssetError::Format(key.into(), "a cube map needs 6 faces (+X, -X, +Y, -Y, +Z, -Z)".into()));
            }
            let mut out = Vec::with_capacity(6);
            for f in &list {
                out.push(to_power_of_two(&self.texture_data(f)?));
            }
            out.try_into().map_err(|_| AssetError::Format(key.into(), "faces".into()))?
        };
        let id = gfx.create_cubemap(&faces).map_err(|e| AssetError::Gpu(key.into(), e))?;
        self.register_texture(key, id);
        Ok(id)
    }

    /// `builtin:default`, a `.mat.json` file, or `<model>#material:<i>`.
    pub fn material(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Result<MaterialId, AssetError> {
        let key: &str = &self.io.canonical(key);
        if let Some(id) = self.materials.get(key) {
            return Ok(*id);
        }
        let desc = self.material_desc_of(gfx, key)?;
        let id = gfx.create_material(&desc).map_err(|e| AssetError::Gpu(key.into(), e))?;
        self.register_material(key, id);
        Ok(id)
    }

    /// Re-reads a loaded `.mat.json` and updates its GPU material in place (renderers keep their
    /// MaterialId): the editor, when the file changed. Not loaded = nothing to do.
    pub fn reload_material(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Result<(), AssetError> {
        let key: &str = &self.io.canonical(key);
        let Some(id) = self.materials.get(key).copied() else { return Ok(()) };
        let desc = self.material_desc_of(gfx, key)?;
        gfx.update_material(id, &desc);
        Ok(())
    }

    /// A `.shader` file changed: compiled again, and every loaded material using it updated.
    pub fn reload_shader(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) {
        let key = self.io.canonical(key);
        // every variant and pass of the file
        let of_file = |k: &str| k == key || k.starts_with(&format!("{key}|")) || k.starts_with(&format!("{key}#"));
        let old: Vec<goat_gfx::ShaderId> = self.gpu_shaders.iter().filter(|(k, _)| of_file(k)).filter_map(|(_, v)| *v).collect();
        self.gpu_shaders.retain(|k, _| !of_file(k));
        self.shaders.retain(|k, _| !of_file(k));
        let users: Vec<String> = self.material_shaders.iter().filter(|(_, s)| **s == key).map(|(m, _)| m.clone()).collect();
        for m in users {
            if let Err(e) = self.reload_material(gfx, &m) {
                goat_core::error!("{m}: {e}");
            }
        }
        for id in old {
            gfx.destroy_shader(id);
        }
    }

    fn material_desc_of(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Result<MaterialDesc, AssetError> {
        let desc = if key == "builtin:default" {
            MaterialDesc::default()
        } else if let Some((file, sub)) = key.split_once('#') {
            let i: usize = sub.strip_prefix("material:").and_then(|s| s.parse().ok()).ok_or_else(|| AssetError::Format(key.into(), "bad sub-asset".into()))?;
            self.model(gfx, file)?;
            let mat = self.models[file].data.materials.get(i).cloned().ok_or_else(|| AssetError::Format(key.into(), "no such material".into()))?;
            let texture = match mat.image {
                Some(_) => Some(self.texture(gfx, &format!("{file}#texture:{i}"))?),
                None => None,
            };
            MaterialDesc { texture, ..mat.desc }
        } else {
            let text = self.io.read_string(key)?;
            let file: MaterialFile = serde_json::from_str(&text).map_err(|e| AssetError::Format(key.into(), e.to_string()))?;
            let rel = |t: &str| if t.starts_with("builtin:") { t.to_string() } else { t.strip_prefix('/').map(str::to_string).unwrap_or_else(|| resolve_relative(key, t)) };
            let texture = match &file.texture {
                Some(t) => Some(self.texture(gfx, &rel(t))?),
                None => None,
            };
            let emissive_texture = match &file.emissive_texture {
                Some(t) => Some(self.texture(gfx, &rel(t))?),
                None => None,
            };
            // a reflection the backend can't do (no cube maps) just isn't drawn
            let env_map = match &file.env_map {
                Some(t) => match self.cubemap(gfx, &rel(t)) {
                    Ok(id) => Some(id),
                    Err(e) => {
                        goat_core::warn!("{key}: env map: {e}");
                        None
                    }
                },
                None => None,
            };
            match &file.shader {
                Some(s) => {
                    let sk = self.io.canonical(&rel(s));
                    self.material_shaders.insert(key.to_string(), sk.clone());
                    self.shader_material(gfx, key, &sk, &file, &rel)
                }
                None => {
                    self.material_shaders.remove(key);
                    MaterialDesc { texture, emissive_texture, env_map, ..file.desc() }
                }
            }
        };
        Ok(desc)
    }

    /// A compiled `.shader` file (cached; the error text lists the problems with their lines).
    pub fn shader(&mut self, key: &str) -> Result<std::sync::Arc<goat_shader::Shader>, AssetError> {
        self.shader_variant(key, &[])
    }

    /// The variant of a `.shader` file for these keywords (cached).
    pub fn shader_variant(&mut self, key: &str, keywords: &[String]) -> Result<std::sync::Arc<goat_shader::Shader>, AssetError> {
        let key: &str = &self.io.canonical(key);
        let vkey = variant_key(key, keywords);
        if let Some(r) = self.shaders.get(&vkey) {
            return r.clone().map_err(|e| AssetError::Format(key.into(), e));
        }
        let text = self.io.read_string(key)?;
        let r = goat_shader::compile_variant(&text, keywords).map(std::sync::Arc::new).map_err(|errs| errs.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "));
        if let (Ok(s), true) = (&r, keywords.is_empty()) {
            for w in &s.warnings {
                goat_core::warn!("{key}: {w}");
            }
        }
        self.shaders.insert(vkey, r.clone());
        r.map_err(|e| AssetError::Format(key.into(), e))
    }

    /// The Goat shader (variant) a loaded material is drawn with (None: the standard material).
    pub fn material_shader(&mut self, id: MaterialId) -> Option<std::sync::Arc<goat_shader::Shader>> {
        let key = self.material_keys.get(&id)?.clone();
        let sk = self.material_shaders.get(&key)?.clone();
        let kw = self.material_keywords.get(&key).cloned().unwrap_or_default();
        self.shader_variant(&sk, &kw).ok()
    }

    /// Forgets compiled shaders and the materials using them (the editor: a `.shader` changed).
    pub fn forget_shader(&mut self, key: &str) {
        let of_file = |k: &str| k == key || k.starts_with(&format!("{key}|")) || k.starts_with(&format!("{key}#"));
        self.shaders.retain(|k, _| !of_file(k));
        self.gpu_shaders.retain(|k, _| !of_file(k));
    }

    /// A material drawn with a Goat shader: its render state, parameters and textures; where the
    /// backend can't run it (3DS) the shader's standard mapping (color, texture, emission, cutoff)
    /// on the standard material. A shader that doesn't compile draws magenta (Unity's error shader).
    fn shader_material(&mut self, gfx: &mut dyn GraphicsBackend, key: &str, shader_key: &str, file: &MaterialFile, rel: &dyn Fn(&str) -> String) -> MaterialDesc {
        let error = |e: &dyn std::fmt::Display| {
            goat_core::error!("{key}: {e}");
            MaterialDesc { base_color: goat_core::Color::rgba(1.0, 0.0, 1.0, 1.0), lighting: goat_gfx::LightingModel::Unlit, ..MaterialDesc::default() }
        };
        let props = file.properties.clone().unwrap_or_default();
        let value = |name: &str| -> Option<[f32; 4]> {
            let v = props.get(name)?;
            let mut a = [0.0f32; 4];
            match v {
                serde_json::Value::Number(n) => a[0] = n.as_f64()? as f32,
                serde_json::Value::Bool(b) => a[0] = if *b { 1.0 } else { 0.0 },
                serde_json::Value::Array(l) => {
                    for (i, x) in l.iter().take(4).enumerate() {
                        a[i] = x.as_f64()? as f32;
                    }
                }
                serde_json::Value::String(s) => a = goat_core::parse_hex(s)?.to_array(),
                _ => return None,
            }
            Some(a)
        };
        // keywords: the material's list + those of its Toggle / KeywordEnum properties
        let base = match self.shader(shader_key) {
            Ok(s) => s,
            Err(e) => return error(&e),
        };
        let mut keywords: Vec<String> = file.keywords.clone().unwrap_or_default();
        for p in &base.properties {
            let v = value(&p.name).map(|v| v[0]).unwrap_or(p.default.first().copied().unwrap_or(0.0));
            if let Some(k) = p.keyword_for(v) {
                if !keywords.contains(&k) {
                    keywords.push(k);
                }
            }
        }
        let sh = match self.shader_variant(shader_key, &keywords) {
            Ok(s) => s,
            Err(e) => return error(&e),
        };
        self.material_keywords.insert(key.to_string(), keywords.clone());
        let params = sh.pack_params(&value);
        let mut color_slots = 0u16;
        for p in &sh.params {
            if sh.properties.iter().any(|q| q.name == p.name && q.kind == goat_shader::PropertyKind::Color) {
                color_slots |= 1 << p.slot;
            }
        }
        let mut textures = [None; goat_gfx::SHADER_TEXTURES];
        for t in &sh.textures {
            let path = props.get(&t.name).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(rel).unwrap_or_else(|| format!("builtin:{}", t.default));
            textures[t.unit] = match self.texture(gfx, &path) {
                Ok(id) => Some(id),
                Err(e) => {
                    goat_core::warn!("{key}: {}: {e}", t.name);
                    self.texture(gfx, "builtin:white").ok()
                }
            };
        }
        let vkey = variant_key(shader_key, &keywords);
        // passes after the first: their own materials, chained (built last to first)
        let mut descs = Vec::new();
        let passes: Vec<&goat_shader::Shader> = std::iter::once(&*sh).chain(sh.passes.iter()).collect();
        for (i, pass) in passes.iter().enumerate() {
            match self.pass_desc(gfx, key, &format!("{vkey}#{i}"), pass, &value, params, color_slots, textures) {
                Ok(d) => descs.push(d),
                Err(e) => return error(&e),
            }
        }
        let mut ids = self.pass_materials.remove(key).unwrap_or_default();
        let mut next = None;
        let mut chain = Vec::new();
        for (i, mut d) in descs.into_iter().enumerate().rev() {
            d.next_pass = next;
            if i == 0 {
                self.pass_materials.insert(key.to_string(), chain.into_iter().rev().collect());
                // stale pass materials (fewer passes than before)
                for id in ids {
                    gfx.destroy_material(id);
                }
                return d;
            }
            // reuse the pass material of an earlier load (renderers don't hold these ids)
            let id = match ids.pop() {
                Some(id) => {
                    gfx.update_material(id, &d);
                    id
                }
                None => match gfx.create_material(&d) {
                    Ok(id) => id,
                    Err(e) => return error(&e),
                },
            };
            chain.push(id);
            next = Some(id);
        }
        error(&"no pass")
    }

    /// One pass of a shader material: its GPU program (or texture combiners / standard mapping)
    /// and render state.
    #[allow(clippy::too_many_arguments)]
    fn pass_desc(
        &mut self,
        gfx: &mut dyn GraphicsBackend,
        key: &str,
        gpu_key: &str,
        sh: &goat_shader::Shader,
        value: &dyn Fn(&str) -> Option<[f32; 4]>,
        params: [[f32; 4]; goat_gfx::SHADER_PARAMS],
        color_slots: u16,
        textures: [Option<goat_gfx::TextureId>; goat_gfx::SHADER_TEXTURES],
    ) -> Result<MaterialDesc, String> {
        // the GPU program (once per variant and pass)
        let gpu = match self.gpu_shaders.get(gpu_key) {
            Some(g) => *g,
            None => {
                let desc = goat_gfx::ShaderDesc { name: sh.name.clone(), wgsl: sh.wgsl_module(), custom_vertex: sh.custom_vertex };
                let g = match gfx.create_shader(&desc) {
                    Ok(id) => Some(id),
                    Err(GfxError::Unsupported(_)) => None,
                    Err(e) => {
                        self.gpu_shaders.insert(gpu_key.to_string(), None);
                        return Err(e.to_string());
                    }
                };
                self.gpu_shaders.insert(gpu_key.to_string(), g);
                g
            }
        };
        let st = &sh.state;
        let blend = match st.blend {
            goat_shader::Blend::Opaque => goat_gfx::BlendMode::Opaque,
            goat_shader::Blend::Alpha => goat_gfx::BlendMode::Alpha,
            goat_shader::Blend::Additive => goat_gfx::BlendMode::Additive,
        };
        let m = &sh.standard;
        let tex_of = |name: &Option<String>| name.as_ref().and_then(|n| sh.textures.iter().find(|t| &t.name == n)).and_then(|t| textures[t.unit]);
        let color_of = |name: &Option<String>| name.as_ref().and_then(|n| value(n).or_else(|| sh.params.iter().find(|p| &p.name == n).map(|p| p.default))).map(|c| goat_core::Color::rgba(c[0], c[1], c[2], c[3]));
        // no custom programs (3DS): the texture combiners when the fragment program fits them,
        // else the standard mapping (Fallback)
        let tev = if gpu.is_none() {
            match sh.tev(value) {
                Ok(p) => Some(p),
                Err(why) => {
                    goat_core::warn!("{key}: {} draws its Fallback here ({why})", sh.name);
                    None
                }
            }
        } else {
            None
        };
        let d = MaterialDesc::default();
        Ok(MaterialDesc {
            tev,
            base_color: color_of(&m.color).unwrap_or(d.base_color),
            texture: tex_of(&m.texture),
            lighting: if m.lighting == "unlit" { goat_gfx::LightingModel::Unlit } else { goat_gfx::LightingModel::Lit },
            toon_steps: if m.lighting == "toon" { 3 } else { 0 },
            emissive: color_of(&m.emission).unwrap_or(d.emissive),
            alpha_cutoff: m.cutoff.as_ref().and_then(|c| value(c).or_else(|| sh.params.iter().find(|p| &p.name == c).map(|p| p.default))).map(|v| v[0].clamp(0.0, 1.0)).unwrap_or(0.0),
            blend,
            cull: match st.cull {
                goat_shader::Cull::Back => goat_gfx::CullMode::Back,
                goat_shader::Cull::Front => goat_gfx::CullMode::Front,
                goat_shader::Cull::Off => goat_gfx::CullMode::None,
            },
            depth_write: st.zwrite.unwrap_or(blend == goat_gfx::BlendMode::Opaque),
            depth_test: !st.ztest_always,
            fog: st.fog,
            shader: gpu,
            shader_params: params,
            shader_color_slots: color_slots,
            shader_textures: textures,
            ..d
        })
    }
    /// `builtin:cube|sphere|plane`, or `<model>#mesh:<mesh>:<primitive>`.
    pub fn mesh(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Result<MeshId, AssetError> {
        let key: &str = &self.io.canonical(key);
        if let Some(id) = self.meshes.get(key) {
            return Ok(*id);
        }
        if let Some((file, sub)) = key.split_once('#') {
            let mut parts = sub.strip_prefix("mesh:").unwrap_or("").split(':').map(|s| s.parse::<usize>().ok());
            let (Some(Some(m)), p) = (parts.next(), parts.next().flatten().unwrap_or(0)) else {
                return Err(AssetError::Format(key.into(), "bad sub-asset".into()));
            };
            self.model(gfx, file)?;
            return self.models[file].meshes.get(m).and_then(|ps| ps.get(p)).copied().ok_or_else(|| AssetError::Format(key.into(), "no such mesh".into()));
        }
        let data = match key {
            "builtin:cube" => MeshData::cube(1.0),
            "builtin:sphere" => MeshData::sphere(0.5, 16, 24),
            "builtin:capsule" => MeshData::capsule(0.5, 2.0, 12, 24),
            "builtin:cylinder" => MeshData::cylinder(0.5, 2.0, 24),
            "builtin:quad" => MeshData::quad(1.0),
            "builtin:plane" => MeshData::plane(10.0, 10.0),
            _ => return Err(AssetError::Format(key.into(), "unknown mesh (builtin:cube|sphere|plane or a model sub-asset)".into())),
        };
        let id = gfx.create_mesh(&data).map_err(|e| AssetError::Gpu(key.into(), e))?;
        self.register_mesh(key, id);
        Ok(id)
    }

    /// The submeshes of a mesh key (a [`MeshFilter`]'s mesh): `<model>#mesh:<m>` = every primitive of
    /// that model mesh (one per material), `<model>#mesh:<m>:<p>` / `builtin:*` = one.
    pub fn mesh_filter(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Result<Vec<MeshId>, AssetError> {
        let key: &str = &self.io.canonical(key);
        if let Some((file, sub)) = key.split_once('#') {
            let parts: Vec<&str> = sub.strip_prefix("mesh:").unwrap_or("").split(':').collect();
            if parts.len() == 1 {
                let m = parts[0].parse::<usize>().map_err(|_| AssetError::Format(key.into(), "bad sub-asset".into()))?;
                self.model(gfx, file)?;
                return self.models[file].meshes.get(m).cloned().ok_or_else(|| AssetError::Format(key.into(), "no such mesh".into()));
            }
        }
        Ok(vec![self.mesh(gfx, key)?])
    }

    /// Loads a glTF / GLB and uploads its meshes and materials.
    pub fn model(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Result<&LoadedModel, AssetError> {
        let key: &str = &self.io.canonical(key);
        let key = file_of(key);
        if !self.models.contains_key(key) {
            let data = load_model(&self.io, key)?;
            let mut meshes = Vec::new();
            for (mi, prims) in data.meshes.iter().enumerate() {
                let mut ids = Vec::new();
                for (pi, p) in prims.iter().enumerate() {
                    let id = gfx.create_mesh(&p.mesh).map_err(|e| AssetError::Gpu(key.into(), e))?;
                    self.register_mesh(&format!("{key}#mesh:{mi}:{pi}"), id);
                    ids.push(id);
                }
                meshes.push(ids);
            }
            // insert before materials (they look the model up)
            self.models.insert(key.to_string(), LoadedModel { data, meshes, materials: Vec::new(), skins: HashMap::new() });
            let count = self.models[key].data.materials.len();
            let mut materials = Vec::with_capacity(count);
            for i in 0..count {
                materials.push(self.material(gfx, &format!("{key}#material:{i}"))?);
            }
            self.models.get_mut(key).unwrap().materials = materials;
            // the meshes' vertex data now lives on the GPU (skinned ones keep their bind pose:
            // they are deformed on the CPU every frame)
            for prims in self.models.get_mut(key).unwrap().data.meshes.iter_mut() {
                for p in prims {
                    if p.skin_vertices.is_none() {
                        p.mesh = MeshData::default();
                    }
                }
            }
        }
        Ok(&self.models[key])
    }

    /// Full vertices of a mesh on the CPU (static batching): `builtin:*` or
    /// `<model>#mesh:<mesh>:<primitive>` (re-read from the model file).
    pub fn mesh_data(&self, key: &str) -> Result<MeshData, AssetError> {
        let key: &str = &self.io.canonical(key);
        match key {
            "builtin:cube" => return Ok(MeshData::cube(1.0)),
            "builtin:sphere" => return Ok(MeshData::sphere(0.5, 16, 24)),
            "builtin:capsule" => return Ok(MeshData::capsule(0.5, 2.0, 12, 24)),
            "builtin:cylinder" => return Ok(MeshData::cylinder(0.5, 2.0, 24)),
            "builtin:quad" => return Ok(MeshData::quad(1.0)),
            "builtin:plane" => return Ok(MeshData::plane(10.0, 10.0)),
            _ => {}
        }
        let (file, sub) = key.split_once('#').ok_or_else(|| AssetError::Format(key.into(), "not a mesh key".into()))?;
        let mut parts = sub.strip_prefix("mesh:").unwrap_or("").split(':').map(|s| s.parse::<usize>().ok());
        let (Some(Some(mi)), pi) = (parts.next(), parts.next().flatten().unwrap_or(0)) else {
            return Err(AssetError::Format(key.into(), "bad sub-asset".into()));
        };
        let data = load_model(&self.io, file)?;
        data.meshes.get(mi).and_then(|ps| ps.get(pi)).map(|p| p.mesh.clone()).ok_or_else(|| AssetError::Format(key.into(), "no such mesh".into()))
    }

    /// Triangles of a mesh on the CPU (mesh colliders, navigation): `builtin:cube|sphere|plane`,
    /// `<model>#mesh:<mesh>:<primitive>`, or a whole `<model>` (every node's meshes in the
    /// model's space). Model files are read again (their GPU meshes keep no CPU copy).
    pub fn mesh_geometry(&self, key: &str) -> Result<(Vec<goat_core::math::Vec3>, Vec<[u32; 3]>), AssetError> {
        let key: &str = &self.io.canonical(key);
        use goat_core::math::{Mat4, Vec3};
        let mut points = Vec::new();
        let mut tris = Vec::new();
        let mut add = |m: &MeshData, xf: &Mat4| {
            let base = points.len() as u32;
            points.extend(m.vertices.iter().map(|v| xf.transform_point3(Vec3::from(v.position))));
            tris.extend(m.indices.chunks_exact(3).map(|t| [base + t[0] as u32, base + t[1] as u32, base + t[2] as u32]));
        };
        match key {
            "builtin:cube" => add(&MeshData::cube(1.0), &Mat4::IDENTITY),
            "builtin:sphere" => add(&MeshData::sphere(0.5, 16, 24), &Mat4::IDENTITY),
            "builtin:capsule" => add(&MeshData::capsule(0.5, 2.0, 12, 24), &Mat4::IDENTITY),
            "builtin:cylinder" => add(&MeshData::cylinder(0.5, 2.0, 24), &Mat4::IDENTITY),
            "builtin:quad" => add(&MeshData::quad(1.0), &Mat4::IDENTITY),
            "builtin:plane" => add(&MeshData::plane(10.0, 10.0), &Mat4::IDENTITY),
            _ => {
                let data = load_model(&self.io, file_of(key))?;
                if let Some((_, sub)) = key.split_once('#') {
                    let mut parts = sub.strip_prefix("mesh:").unwrap_or("").split(':').map(|s| s.parse::<usize>().ok());
                    let (Some(Some(mi)), pi) = (parts.next(), parts.next().flatten()) else {
                        return Err(AssetError::Format(key.into(), "bad sub-asset".into()));
                    };
                    let prims = data.meshes.get(mi).ok_or_else(|| AssetError::Format(key.into(), "no such mesh".into()))?;
                    match pi {
                        // one primitive, or every submesh of the mesh
                        Some(pi) => add(&prims.get(pi).ok_or_else(|| AssetError::Format(key.into(), "no such mesh".into()))?.mesh, &Mat4::IDENTITY),
                        None => prims.iter().for_each(|p| add(&p.mesh, &Mat4::IDENTITY)),
                    }
                } else {
                    let mut stack: Vec<(usize, Mat4)> = data.roots.iter().map(|&r| (r, Mat4::IDENTITY)).collect();
                    while let Some((n, parent)) = stack.pop() {
                        let Some(node) = data.nodes.get(n) else { continue };
                        let m = parent * Mat4::from_scale_rotation_translation(node.scale, node.rotation, node.translation);
                        if let Some(mi) = node.mesh {
                            for p in data.meshes.get(mi).into_iter().flatten() {
                                add(&p.mesh, &m);
                            }
                        }
                        stack.extend(node.children.iter().map(|&c| (c, m)));
                    }
                }
            }
        }
        if tris.is_empty() {
            return Err(AssetError::Format(key.into(), "no triangles".into()));
        }
        Ok((points, tris))
    }

    /// A model already loaded (no GPU access needed).
    pub fn loaded_model(&self, key: &str) -> Option<&LoadedModel> {
        self.models.get(file_of(key))
    }

    /// Spawns a model's node hierarchy (Transform + Parent + MeshRenderer) and returns its root,
    /// tagged with [`ModelInstance`].
    pub fn spawn_model(&mut self, world: &mut World, gfx: &mut dyn GraphicsBackend, key: &str, parent: Option<Entity>) -> Result<Entity, AssetError> {
        let name = file_of(key).rsplit('/').next().unwrap_or(key).to_string();
        let root = world.spawn((Name(name), Transform::default(), GlobalTransform::default()));
        if let Some(p) = parent {
            let _ = world.insert_one(root, Parent(p));
        }
        self.spawn_model_into(world, gfx, key, root)?;
        Ok(root)
    }

    /// Spawns a model's nodes as children of an existing entity and tags it [`ModelInstance`].
    pub fn spawn_model_into(&mut self, world: &mut World, gfx: &mut dyn GraphicsBackend, key: &str, root: Entity) -> Result<(), AssetError> {
        let key: &str = &self.io.canonical(key);
        let fallback = self.material(gfx, "builtin:default")?;
        self.model(gfx, key)?;
        let file = file_of(key).to_string();
        // skin data (bindposes + weights) of the skinned meshes, shared by every instance
        {
            let m = self.models.get_mut(&file).expect("loaded above");
            let wanted: Vec<(usize, usize)> = m.data.nodes.iter().filter_map(|n| Some((n.mesh?, n.skin?))).collect();
            for (mi, si) in wanted {
                if m.skins.contains_key(&(mi, si)) {
                    continue;
                }
                let Some(prims) = m.data.meshes.get(mi) else { continue };
                if !prims.iter().any(|p| p.skin_vertices.is_some()) {
                    continue;
                }
                let bindposes = m.data.skins.get(si).map(|s| s.inverse_bind.clone()).unwrap_or_default();
                let submeshes = prims
                    .iter()
                    .map(|p| p.skin_vertices.as_ref().map(|sv| goat_runtime::SkinnedSubmesh { mesh: p.mesh.clone(), joints: sv.joints.clone(), weights: sv.weights.clone() }))
                    .collect();
                m.skins.insert((mi, si), std::sync::Arc::new(goat_runtime::SkinData { bindposes, submeshes }));
            }
        }
        let model = &self.models[&file];
        let _ = world.insert_one(root, ModelInstance { key: file.clone() });
        let mut nodes: Vec<Option<Entity>> = vec![None; model.data.nodes.len()];
        // skinned entities: their bones are set once every node exists
        let mut skinned: Vec<(Entity, usize)> = Vec::new();
        #[allow(clippy::too_many_arguments)]
        fn spawn_node(world: &mut World, model: &LoadedModel, fallback: MaterialId, node: usize, parent: Entity, nodes: &mut Vec<Option<Entity>>, skinned: &mut Vec<(Entity, usize)>, depth: u32) {
            let Some(n) = model.data.nodes.get(node) else { return };
            if depth > 64 {
                return;
            }
            let t = Transform { position: n.translation, rotation: n.rotation, scale: n.scale };
            let e = world.spawn((Name(n.name.clone()), t, GlobalTransform::default(), Parent(parent)));
            nodes[node] = Some(e);
            if let Some(mi) = n.mesh {
                let prims = &model.data.meshes[mi];
                let materials: Vec<MaterialId> = prims.iter().map(|p| p.material.and_then(|m| model.materials.get(m).copied()).unwrap_or(fallback)).collect();
                match n.skin.and_then(|si| model.skins.get(&(mi, si)).map(|d| (si, d.clone()))) {
                    // Unity: a SkinnedMeshRenderer (mesh + materials + bones), no MeshFilter
                    Some((si, data)) => {
                        let mut r = goat_runtime::SkinnedMeshRenderer::new(model.meshes[mi].clone(), materials);
                        r.skin = Some(data);
                        let _ = world.insert_one(e, r);
                        skinned.push((e, si));
                    }
                    // Unity: one MeshFilter (the mesh, a submesh per material) + one MeshRenderer (its materials)
                    None => {
                        let _ = world.insert(e, (MeshFilter::with_submeshes(model.meshes[mi].clone()), MeshRenderer::with_materials(materials)));
                    }
                }
            }
            for &c in &n.children {
                spawn_node(world, model, fallback, c, e, nodes, skinned, depth + 1);
            }
        }
        for &r in &model.data.roots {
            spawn_node(world, model, fallback, r, root, &mut nodes, &mut skinned, 0);
        }
        // bones = the skin's joints; root bone = the joint whose parent isn't a joint (the skeleton's top)
        let mut parent_of = vec![usize::MAX; model.data.nodes.len()];
        for (i, n) in model.data.nodes.iter().enumerate() {
            for &c in &n.children {
                if c < parent_of.len() {
                    parent_of[c] = i;
                }
            }
        }
        for (e, si) in skinned {
            let Some(s) = model.data.skins.get(si) else { continue };
            let bones: Vec<Entity> = s.joints.iter().map(|&j| nodes.get(j).copied().flatten().unwrap_or(root)).collect();
            let top = s.joints.iter().find(|&&j| !s.joints.contains(&parent_of.get(j).copied().unwrap_or(usize::MAX))).copied();
            if let Ok(mut r) = world.get::<&mut goat_runtime::SkinnedMeshRenderer>(e) {
                r.root_bone = top.and_then(|j| nodes.get(j).copied().flatten());
                r.bones = bones;
            }
        }
        let _ = world.insert_one(root, ModelNodes(nodes));
        Ok(())
    }
}

/// Inserts the [`AssetServer`] resource (asset root = the platform's [`default_root`](crate::default_root)).
#[derive(Default)]
pub struct AssetsPlugin {
    /// Overrides the root (tests, tools).
    pub root: Option<std::path::PathBuf>,
    /// Used when the platform's root doesn't exist (desktop runs straight from `target/`):
    /// games pass their source assets folder, `concat!(env!("CARGO_MANIFEST_DIR"), "/assets")`.
    pub dev_root: Option<std::path::PathBuf>,
}

impl AssetsPlugin {
    pub fn with_dev_root(dev_root: impl Into<std::path::PathBuf>) -> Self {
        AssetsPlugin { root: None, dev_root: Some(dev_root.into()) }
    }
}

impl Plugin for AssetsPlugin {
    fn build(&mut self, ctx: &mut Context) {
        if ctx.resources.get::<AssetServer>().is_none() {
            let mut root = self.root.clone().unwrap_or_else(crate::default_root);
            if !cfg!(target_os = "horizon") && !root.is_dir() {
                if let Some(dev) = self.dev_root.clone().filter(|d| d.is_dir()) {
                    root = dev;
                }
            }
            goat_core::info!("assets: {}", root.display());
            ctx.resources.insert(AssetServer::new(AssetIo::new(root)));
        }
        // project settings (Unity's ProjectSettings + Build Settings)
        if ctx.resources.get::<goat_runtime::ProjectSettings>().is_none() {
            // goat.project.json: at the root of a build's assets, next to a project's assets/
            // folder (players run from the sources), or the older assets/project.json
            let file = goat_runtime::settings::PROJECT_FILE;
            let text = ctx.resources.get::<AssetServer>().and_then(|a| {
                [file.to_string(), format!("../{file}"), "project.json".to_string()].into_iter().find_map(|k| a.io.read_string(&k).ok())
            });
            if let Some(text) = text {
                match serde_json::from_str::<goat_runtime::ProjectSettings>(&text) {
                    Ok(s) => ctx.resources.insert(s),
                    Err(e) => goat_core::warn!("{file}: {e}"),
                }
            }
        }
    }
}

/// Shortcuts for game code: `goat_assets::texture(ctx, "textures/a.png")`.
pub fn texture(ctx: &mut Context, key: &str) -> Result<TextureId, AssetError> {
    let server = ctx.resources.get_mut::<AssetServer>().ok_or_else(|| AssetError::Format(key.into(), "no AssetsPlugin".into()))?;
    server.texture(ctx.gfx, key)
}
pub fn material(ctx: &mut Context, key: &str) -> Result<MaterialId, AssetError> {
    let server = ctx.resources.get_mut::<AssetServer>().ok_or_else(|| AssetError::Format(key.into(), "no AssetsPlugin".into()))?;
    server.material(ctx.gfx, key)
}
pub fn mesh(ctx: &mut Context, key: &str) -> Result<MeshId, AssetError> {
    let server = ctx.resources.get_mut::<AssetServer>().ok_or_else(|| AssetError::Format(key.into(), "no AssetsPlugin".into()))?;
    server.mesh(ctx.gfx, key)
}
pub fn spawn_model(ctx: &mut Context, key: &str, parent: Option<Entity>) -> Result<Entity, AssetError> {
    let server = ctx.resources.get_mut::<AssetServer>().ok_or_else(|| AssetError::Format(key.into(), "no AssetsPlugin".into()))?;
    server.spawn_model(ctx.world, ctx.gfx, key, parent)
}

/// Face `i` (+X, -X, +Y, -Y, +Z, -Z) of the builtin sky: a gradient by direction height.
fn sky_face(i: usize) -> TextureData {
    use goat_core::math::Vec3;
    let n = 32u32;
    let mut rgba = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            // face coordinates (-1..1), v down
            let (u, v) = ((x as f32 + 0.5) / n as f32 * 2.0 - 1.0, (y as f32 + 0.5) / n as f32 * 2.0 - 1.0);
            let d = match i {
                0 => Vec3::new(1.0, -v, -u),
                1 => Vec3::new(-1.0, -v, u),
                2 => Vec3::new(u, 1.0, v),
                3 => Vec3::new(u, -1.0, -v),
                4 => Vec3::new(u, -v, 1.0),
                _ => Vec3::new(-u, -v, -1.0),
            }
            .normalize();
            let h = d.y;
            let (sky, horizon, ground) = (Vec3::new(0.35, 0.55, 0.9), Vec3::new(0.85, 0.88, 0.92), Vec3::new(0.25, 0.22, 0.2));
            let c = if h >= 0.0 { horizon.lerp(sky, h.powf(0.6)) } else { horizon.lerp(ground, (-h).powf(0.4)) };
            rgba.extend_from_slice(&[(c.x * 255.0) as u8, (c.y * 255.0) as u8, (c.z * 255.0) as u8, 255]);
        }
    }
    TextureData { width: n, height: n, rgba, filter: goat_gfx::Filter::Linear, wrap: goat_gfx::Wrap::Clamp, mipmaps: false, format: Default::default(), compressed: None, bc: None }
}

/// The cache key of a shader variant: the path, then its keywords sorted.
fn variant_key(key: &str, keywords: &[String]) -> String {
    if keywords.is_empty() {
        return key.to_string();
    }
    let mut k = keywords.to_vec();
    k.sort();
    k.dedup();
    format!("{key}|{}", k.join(" "))
}