//! [`AssetServer`]: loads assets by key once, keeps their GPU handles, maps handles back to keys.

use std::collections::HashMap;

use goat_gfx::{GfxError, GraphicsBackend, MaterialDesc, MaterialId, MeshData, MeshId, TextureData, TextureId};
use goat_runtime::{Context, Entity, GlobalTransform, MeshRenderer, Name, Parent, Plugin, Transform, World};

use crate::image::{decode_png, to_power_of_two, TextureImport};
use crate::material::MaterialFile;
use crate::model::{load_model, ModelData};
use crate::{ext_of, file_of, resolve_relative, AssetError, AssetIo};

/// A model's GPU resources, by glTF mesh / primitive and material index.
pub struct LoadedModel {
    pub data: ModelData,
    pub meshes: Vec<Vec<MeshId>>,
    pub materials: Vec<MaterialId>,
}

/// On a model instance's root: the entity of each glTF node (animation targets, skin joints).
#[derive(Clone, Debug, Default)]
pub struct ModelNodes(pub Vec<Option<Entity>>);

/// On an entity drawing a skinned primitive: deformed on the CPU every frame by the animation
/// plugin into its own `instance` mesh.
#[derive(Clone, Debug)]
pub struct SkinnedMesh {
    pub model: String,
    pub mesh: usize,
    pub primitive: usize,
    pub skin: usize,
    /// The model instance's root (holds [`ModelNodes`]).
    pub root: Entity,
    pub instance: Option<MeshId>,
}

/// Put on the root entity of a spawned model (scenes save the reference, not the hierarchy).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInstance {
    pub key: String,
}

pub struct AssetServer {
    pub io: AssetIo,
    textures: HashMap<String, TextureId>,
    materials: HashMap<String, MaterialId>,
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

    /// Names resources made in code (`runtime:...`) so scenes can refer to them.
    pub fn register_texture(&mut self, key: &str, id: TextureId) {
        self.textures.insert(key.to_string(), id);
        self.texture_keys.insert(id, key.to_string());
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
        if key == "builtin:white" {
            // 8×8 white, 8 pixels per unit by convention: tinted sprites / UI rectangles
            return Ok(TextureData::checker(8, 1, goat_core::Color::WHITE, goat_core::Color::WHITE));
        }
        let bytes = self.io.read(key)?;
        let t = match ext_of(key).as_str() {
            "png" => decode_png(&bytes).map_err(|e| AssetError::Format(key.into(), e))?,
            other => return Err(AssetError::Format(key.into(), format!("unsupported image format .{other} (PNG only)"))),
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
        let t = self.texture_data(key)?;
        let id = Self::upload_texture(gfx, key, &t)?;
        self.register_texture(key, id);
        self.texture_sizes.insert(id, (t.width, t.height));
        Ok(id)
    }

    /// `builtin:default`, a `.mat.json` file, or `<model>#material:<i>`.
    pub fn material(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Result<MaterialId, AssetError> {
        if let Some(id) = self.materials.get(key) {
            return Ok(*id);
        }
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
            let texture = match &file.texture {
                Some(t) => {
                    let tkey = match t.strip_prefix('/') {
                        Some(rooted) => rooted.to_string(),
                        None => resolve_relative(key, t),
                    };
                    Some(self.texture(gfx, &tkey)?)
                }
                None => None,
            };
            MaterialDesc { texture, ..file.desc() }
        };
        let id = gfx.create_material(&desc).map_err(|e| AssetError::Gpu(key.into(), e))?;
        self.register_material(key, id);
        Ok(id)
    }

    /// `builtin:cube|sphere|plane`, or `<model>#mesh:<mesh>:<primitive>`.
    pub fn mesh(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Result<MeshId, AssetError> {
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
            "builtin:plane" => MeshData::plane(10.0, 10.0),
            _ => return Err(AssetError::Format(key.into(), "unknown mesh (builtin:cube|sphere|plane or a model sub-asset)".into())),
        };
        let id = gfx.create_mesh(&data).map_err(|e| AssetError::Gpu(key.into(), e))?;
        self.register_mesh(key, id);
        Ok(id)
    }

    /// Loads a glTF / GLB and uploads its meshes and materials.
    pub fn model(&mut self, gfx: &mut dyn GraphicsBackend, key: &str) -> Result<&LoadedModel, AssetError> {
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
            self.models.insert(key.to_string(), LoadedModel { data, meshes, materials: Vec::new() });
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
        let fallback = self.material(gfx, "builtin:default")?;
        let model = self.model(gfx, key)?;
        let file = file_of(key).to_string();
        let _ = world.insert_one(root, ModelInstance { key: file.clone() });
        let mut nodes: Vec<Option<Entity>> = vec![None; model.data.nodes.len()];
        #[allow(clippy::too_many_arguments)]
        fn spawn_node(world: &mut World, model: &LoadedModel, file: &str, fallback: MaterialId, node: usize, parent: Entity, root: Entity, nodes: &mut Vec<Option<Entity>>, depth: u32) {
            let Some(n) = model.data.nodes.get(node) else { return };
            if depth > 64 {
                return;
            }
            let t = Transform { position: n.translation, rotation: n.rotation, scale: n.scale };
            let e = world.spawn((Name(n.name.clone()), t, GlobalTransform::default(), Parent(parent)));
            nodes[node] = Some(e);
            if let Some(mi) = n.mesh {
                let prims = &model.data.meshes[mi];
                let renderer = |pi: usize| {
                    let material = prims[pi].material.and_then(|m| model.materials.get(m).copied()).unwrap_or(fallback);
                    MeshRenderer::new(model.meshes[mi][pi], material)
                };
                for pi in 0..prims.len() {
                    let target = if prims.len() == 1 { e } else { world.spawn((Name(format!("{} [{pi}]", n.name)), Transform::default(), GlobalTransform::default(), Parent(e))) };
                    let _ = world.insert_one(target, renderer(pi));
                    if let (Some(skin), Some(_)) = (n.skin, &prims[pi].skin_vertices) {
                        let _ = world.insert_one(target, SkinnedMesh { model: file.to_string(), mesh: mi, primitive: pi, skin, root, instance: None });
                    }
                }
            }
            for &c in &n.children {
                spawn_node(world, model, file, fallback, c, e, root, nodes, depth + 1);
            }
        }
        for &r in &model.data.roots {
            spawn_node(world, model, &file, fallback, r, root, root, &mut nodes, 0);
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
