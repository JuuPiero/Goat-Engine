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

pub mod bake;
pub mod bjson;
pub mod glb;

/// Model files (glTF / GLB, and FBX / OBJ: converted to GLB by the editor, `goat build` and the desktop player).
pub fn is_model_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    [".glb", ".gltf", ".fbx", ".obj"].iter().any(|e| k.ends_with(e))
}
pub mod image;
pub mod batch;
pub mod material;
pub mod model;
pub mod server;

pub use batch::StaticBatchPlugin;
pub use material::MaterialFile;
pub use model::{set_model_converter, AnimChannel, AnimPath, Interpolation, ModelAnimation, ModelConverter, ModelData, ModelMaterial, ModelNode, ModelPrimitive, ModelSkin, SkinVertices};
pub use server::{material, mesh, spawn_model, texture, AssetServer, AssetsPlugin, LoadedModel, ModelInstance, ModelNodes};

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
///
/// Keys may also name a file by its GUID, `guid:<32 hex>` (optionally `#sub-asset`): every
/// asset has a `<file>.meta` (`{ "guid": "..." }`, written by the editor) and the GUID stays
/// the same when the file is renamed or moved. The GUID index is built on first use by scanning
/// the `.meta` files (on disk and in memory).
#[derive(Clone, Debug, Default)]
pub struct AssetIo {
    pub root: PathBuf,
    /// Files served from memory first (the web / the editor's engine, which has no file system).
    pub memory: std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, std::sync::Arc<Vec<u8>>>>>,
    guids: std::sync::Arc<std::sync::RwLock<Option<std::collections::HashMap<String, String>>>>,
}

/// Extension of the GUID files next to assets.
pub const META_EXT: &str = ".meta";

/// The GUID of a `.meta` file's text.
pub fn parse_meta_guid(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let g = v.get("guid")?.as_str()?.to_ascii_lowercase();
    (g.len() == 32 && g.bytes().all(|b| b.is_ascii_hexdigit())).then_some(g)
}

impl AssetIo {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        AssetIo { root: root.into(), ..Default::default() }
    }

    /// The path key of `key`: `guid:<hex>[#sub]` → `dir/file.ext[#sub]`; other keys unchanged.
    /// An unknown GUID stays as is (reading it then fails with its name).
    pub fn canonical(&self, key: &str) -> String {
        let Some(rest) = key.strip_prefix("guid:") else { return key.to_string() };
        let (guid, sub) = match rest.split_once('#') {
            Some((g, s)) => (g, Some(s)),
            None => (rest, None),
        };
        match self.path_of_guid(guid) {
            Some(path) => match sub {
                Some(s) => format!("{path}#{s}"),
                None => path,
            },
            None => key.to_string(),
        }
    }

    /// The file key of a GUID.
    pub fn path_of_guid(&self, guid: &str) -> Option<String> {
        let guid = guid.to_ascii_lowercase();
        if let Some(found) = self.guids.read().ok().and_then(|g| g.as_ref().map(|m| m.get(&guid).cloned())) {
            if found.is_some() {
                return found;
            }
        }
        // (re)scan: files may have been added since
        let index = self.scan_guids();
        let found = index.get(&guid).cloned();
        if let Ok(mut g) = self.guids.write() {
            *g = Some(index);
        }
        found
    }

    /// The GUID of a file key (its `.meta`), if it has one.
    pub fn guid_of(&self, key: &str) -> Option<String> {
        self.read_string(&format!("{}{META_EXT}", file_of(key))).ok().and_then(|t| parse_meta_guid(&t))
    }

    fn scan_guids(&self) -> std::collections::HashMap<String, String> {
        let mut out = std::collections::HashMap::new();
        if let Ok(m) = self.memory.read() {
            for (k, v) in m.iter() {
                if let Some(file) = k.strip_suffix(META_EXT) {
                    if let Some(g) = std::str::from_utf8(v).ok().and_then(parse_meta_guid) {
                        out.insert(g, file.to_string());
                    }
                }
            }
        }
        fn walk(dir: &Path, prefix: &str, out: &mut std::collections::HashMap<String, String>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let key = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
                let p = e.path();
                if p.is_dir() {
                    walk(&p, &key, out);
                } else if let Some(file) = key.strip_suffix(META_EXT) {
                    if let Some(g) = std::fs::read_to_string(&p).ok().and_then(|t| parse_meta_guid(&t)) {
                        out.entry(g).or_insert_with(|| file.to_string());
                    }
                }
            }
        }
        walk(&self.root, "", &mut out);
        out
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
        if key.starts_with("guid:") {
            let path = self.canonical(key);
            if path.starts_with("guid:") {
                return Err(AssetError::Format(key.to_string(), "no asset has this GUID".into()));
            }
            return self.read(&path);
        }
        if let Some(b) = self.memory.read().ok().and_then(|m| m.get(key).cloned()) {
            return Ok(b.as_ref().clone());
        }
        std::fs::read(self.path(key)).map_err(|e| AssetError::Io(key.to_string(), e))
    }
    pub fn read_string(&self, key: &str) -> Result<String, AssetError> {
        let bytes = self.read(key)?;
        // binary JSON of a build: back to text for loaders that parse text
        if bjson::is_binary(&bytes) {
            let v = bjson::decode(&bytes).map_err(|e| AssetError::Format(key.to_string(), e))?;
            return Ok(serde_json::to_string(&v).unwrap_or_default());
        }
        // files written by Windows tools may start with a UTF-8 BOM
        let text = String::from_utf8(bytes).map_err(|e| AssetError::Format(key.to_string(), e.to_string()))?;
        Ok(text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text))
    }
    /// A JSON file as a value: text, or the binary JSON builds store (no text round trip).
    pub fn read_json(&self, key: &str) -> Result<serde_json::Value, AssetError> {
        let bytes = self.read(key)?;
        if bjson::is_binary(&bytes) {
            return bjson::decode(&bytes).map_err(|e| AssetError::Format(key.to_string(), e));
        }
        let text = std::str::from_utf8(&bytes).map_err(|e| AssetError::Format(key.to_string(), e.to_string()))?;
        serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(text)).map_err(|e| AssetError::Format(key.to_string(), e.to_string()))
    }

    /// Every file key (memory and disk, `.meta` files left out), sorted: `Resources.loadAll` and
    /// friends search it.
    pub fn list_all(&self) -> Vec<String> {
        let mut out = std::collections::BTreeSet::new();
        if let Ok(m) = self.memory.read() {
            out.extend(m.keys().filter(|k| !k.ends_with(META_EXT)).cloned());
        }
        fn walk(dir: &Path, prefix: &str, out: &mut std::collections::BTreeSet<String>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let key = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
                let p = e.path();
                if p.is_dir() {
                    walk(&p, &key, out);
                } else if !key.ends_with(META_EXT) {
                    out.insert(key);
                }
            }
        }
        walk(&self.root, "", &mut out);
        out.into_iter().collect()
    }

    pub fn exists(&self, key: &str) -> bool {
        if key.starts_with("guid:") {
            let path = self.canonical(key);
            return !path.starts_with("guid:") && self.exists(&path);
        }
        self.memory.read().is_ok_and(|m| m.contains_key(key)) || self.path(key).is_file()
    }
}

/// `dir/of/key` + a relative reference (`../textures/a.png`) → a key.
pub fn resolve_relative(base_key: &str, reference: &str) -> String {
    // GUID / built-in references are absolute
    if reference.starts_with("guid:") || reference.starts_with("builtin:") {
        return reference.to_string();
    }
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

    #[test]
    fn guid_keys_resolve_through_meta_files() {
        let io = super::AssetIo::new("does-not-exist");
        io.put("textures/a.png", vec![1, 2, 3]);
        io.put("textures/a.png.meta", br#"{ "guid": "0123456789ABCDEF0123456789abcdef" }"#.to_vec());
        let g = "guid:0123456789abcdef0123456789abcdef";
        assert_eq!(io.canonical(g), "textures/a.png");
        assert_eq!(io.canonical(&format!("{g}#mesh:0")), "textures/a.png#mesh:0");
        assert_eq!(io.read(g).unwrap(), vec![1, 2, 3]);
        assert!(io.exists(g));
        assert_eq!(io.guid_of("textures/a.png#sub").as_deref(), Some("0123456789abcdef0123456789abcdef"));
        // files added later are found by a rescan
        io.put("b.txt", vec![9]);
        io.put("b.txt.meta", br#"{"guid":"ffffffffffffffffffffffffffffffff"}"#.to_vec());
        assert_eq!(io.read("guid:ffffffffffffffffffffffffffffffff").unwrap(), vec![9]);
        assert!(io.read("guid:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee").is_err());
        assert_eq!(super::parse_meta_guid("{\"guid\": \"short\"}"), None);
    }
}
