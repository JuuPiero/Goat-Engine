//! Assets: everything a game loads from files.
//!
//! - Files live in the game's `assets/` folder. At run time that folder is the platform's
//!   [`default_root`]: `romfs:/` on the 3DS (packed into the .3dsx), the `assets` folder next to
//!   the executable on desktop (or `GOAT_ASSETS`).
//! - Asset **keys** are paths relative to the root (`textures/crate.png`), built-ins
//!   (`builtin:cube`) or sub-assets of a file (`models/robot.glb#mesh:0:1`, `#material:2`). Scenes
//!   store keys, never GPU handles.
//! - [`AssetServer`] (a resource inserted by [`AssetsPlugin`]) loads and caches by key and maps
//!   GPU handles back to keys (scene saving).
//!
//! Formats: PNG ([`image`]), glTF 2.0 / GLB ([`model`]), material files `*.mat.json`
//! ([`material`]), texture import settings `<texture>.import.json`.

pub mod image;
pub mod batch;
pub mod material;
pub mod model;
pub mod server;

pub use batch::StaticBatchPlugin;
pub use material::MaterialFile;
pub use model::{AnimChannel, AnimPath, Interpolation, ModelAnimation, ModelData, ModelNode, ModelSkin, SkinVertices};
pub use server::{material, mesh, spawn_model, texture, AssetServer, AssetsPlugin, LoadedModel, ModelInstance, ModelNodes, SkinnedMesh};

use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum AssetError {
    Io(String, std::io::Error),
    Format(String, String),
    Gpu(String, goat_gfx::GfxError),
}

impl std::fmt::Display for AssetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetError::Io(k, e) => write!(f, "{k}: {e}"),
            AssetError::Format(k, e) => write!(f, "{k}: {e}"),
            AssetError::Gpu(k, e) => write!(f, "{k}: {e}"),
        }
    }
}

impl std::error::Error for AssetError {}

/// The assets folder of this platform.
pub fn default_root() -> PathBuf {
    if cfg!(target_os = "horizon") {
        return PathBuf::from("romfs:/");
    }
    if let Some(dir) = std::env::var_os("GOAT_ASSETS").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    let next_to_exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("assets")));
    match next_to_exe {
        Some(d) if d.is_dir() => d,
        _ => PathBuf::from("assets"),
    }
}

/// Reads files under a root folder.
#[derive(Clone, Debug, Default)]
pub struct AssetIo {
    pub root: PathBuf,
    /// Files served from memory first (the web / the editor's engine, which has no file system).
    pub memory: std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, std::sync::Arc<Vec<u8>>>>>,
}

impl AssetIo {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        AssetIo { root: root.into(), memory: Default::default() }
    }
    /// Adds / replaces a file served from memory.
    pub fn put(&self, key: &str, bytes: Vec<u8>) {
        if let Ok(mut m) = self.memory.write() {
            m.insert(key.to_string(), std::sync::Arc::new(bytes));
        }
    }
    pub fn path(&self, key: &str) -> PathBuf {
        // keys always use '/'; romfs:/ paths too
        let mut p = self.root.clone();
        for part in key.split('/').filter(|s| !s.is_empty()) {
            p.push(part);
        }
        p
    }
    pub fn read(&self, key: &str) -> Result<Vec<u8>, AssetError> {
        if let Some(b) = self.memory.read().ok().and_then(|m| m.get(key).cloned()) {
            return Ok(b.as_ref().clone());
        }
        std::fs::read(self.path(key)).map_err(|e| AssetError::Io(key.to_string(), e))
    }
    pub fn read_string(&self, key: &str) -> Result<String, AssetError> {
        let bytes = self.read(key)?;
        // files written by Windows tools may start with a UTF-8 BOM
        let text = String::from_utf8(bytes).map_err(|e| AssetError::Format(key.to_string(), e.to_string()))?;
        Ok(text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text))
    }
    pub fn exists(&self, key: &str) -> bool {
        self.memory.read().is_ok_and(|m| m.contains_key(key)) || self.path(key).is_file()
    }
}

/// `dir/of/key` + a relative reference (`../textures/a.png`) → a key.
pub fn resolve_relative(base_key: &str, reference: &str) -> String {
    let mut parts: Vec<&str> = base_key.split('/').collect();
    parts.pop(); // the file name
    for part in reference.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// The file part of a key (`models/a.glb#mesh:0:0` → `models/a.glb`).
pub fn file_of(key: &str) -> &str {
    key.split('#').next().unwrap_or(key)
}

pub(crate) fn ext_of(key: &str) -> String {
    Path::new(file_of(key)).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    #[test]
    fn relative_keys() {
        assert_eq!(super::resolve_relative("models/robot.gltf", "robot.bin"), "models/robot.bin");
        assert_eq!(super::resolve_relative("models/robot.gltf", "../textures/a.png"), "textures/a.png");
        assert_eq!(super::file_of("models/a.glb#mesh:0:1"), "models/a.glb");
    }
}
