//! Save data: Unity's `PlayerPrefs` (small key → value settings) and save files (any bytes / JSON
//! per slot), stored where each platform keeps user data:
//!
//! - desktop: `%APPDATA%/<game>/` (Windows), `~/Library/Application Support/<game>/` (macOS),
//!   `$XDG_DATA_HOME` or `~/.local/share/<game>/` (Linux)
//! - Nintendo 3DS: `sdmc:/3ds/<game>/save/` on the SD card
//! - anything else / tests: in memory ([`SaveBackend::memory`])
//!
//! ```ignore
//! let save = goat_save::save(ctx);
//! save.prefs.set_int("highscore", 1200);
//! save.write_json("slot1", &state)?;
//! ```
//!
//! [`SavePlugin`] writes PlayerPrefs at most once a second while they changed, and when the game
//! ends (Unity writes them on quit; `save.flush()` = `PlayerPrefs.Save()`). Feature `script` =
//! `PlayerPrefs.*` / `Save.*` calls for scripts.

#[cfg(feature = "script")]
pub mod script;

use std::collections::BTreeMap;
use std::path::PathBuf;

use goat_runtime::{Context, Plugin};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

// ------------------------------------------------------------------ storage

/// Where save data lives.
pub enum SaveBackend {
    /// A directory (created on the first write).
    Dir(PathBuf),
    /// Nothing persists (tests, platforms without storage yet).
    Memory(BTreeMap<String, Vec<u8>>),
}

impl SaveBackend {
    pub fn memory() -> Self {
        SaveBackend::Memory(BTreeMap::new())
    }

    /// The platform's user-data directory for `game` (see the crate docs).
    pub fn platform(game: &str) -> Self {
        let game = sanitize(game);
        #[cfg(target_os = "horizon")]
        {
            // libctru mounts the SD card as the default device: "/3ds/..." = sdmc:/3ds/...
            return SaveBackend::Dir(PathBuf::from(format!("/3ds/{game}/save")));
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = game;
            return SaveBackend::memory();
        }
        #[cfg(all(not(target_os = "horizon"), not(target_arch = "wasm32")))]
        {
            let base = if cfg!(windows) {
                std::env::var_os("APPDATA").map(PathBuf::from)
            } else if cfg!(target_os = "macos") {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
            } else {
                std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            };
            match base {
                Some(b) => SaveBackend::Dir(b.join(game)),
                None => SaveBackend::memory(),
            }
        }
    }

    fn path(dir: &std::path::Path, name: &str) -> PathBuf {
        dir.join(sanitize(name))
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        match self {
            SaveBackend::Dir(d) => std::fs::read(Self::path(d, name)).ok(),
            SaveBackend::Memory(m) => m.get(name).cloned(),
        }
    }

    /// Writes through a temporary file + rename, so a crash / power loss mid-write keeps the
    /// previous save intact.
    pub fn write(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            SaveBackend::Dir(d) => {
                std::fs::create_dir_all(&*d)?;
                let path = Self::path(d, name);
                let tmp = path.with_extension("tmp");
                std::fs::write(&tmp, bytes)?;
                // the 3DS SD card (FAT) doesn't replace on rename
                let _ = std::fs::remove_file(&path);
                std::fs::rename(&tmp, &path)
            }
            SaveBackend::Memory(m) => {
                m.insert(name.into(), bytes.to_vec());
                Ok(())
            }
        }
    }

    pub fn delete(&mut self, name: &str) {
        match self {
            SaveBackend::Dir(d) => {
                let _ = std::fs::remove_file(Self::path(d, name));
            }
            SaveBackend::Memory(m) => {
                m.remove(name);
            }
        }
    }

    pub fn exists(&self, name: &str) -> bool {
        match self {
            SaveBackend::Dir(d) => Self::path(d, name).is_file(),
            SaveBackend::Memory(m) => m.contains_key(name),
        }
    }

    /// Saved names (save slots).
    pub fn list(&self) -> Vec<String> {
        match self {
            SaveBackend::Dir(d) => {
                let mut v: Vec<String> = std::fs::read_dir(d)
                    .map(|r| r.flatten().filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.ends_with(".tmp")).collect())
                    .unwrap_or_default();
                v.sort();
                v
            }
            SaveBackend::Memory(m) => m.keys().cloned().collect(),
        }
    }
}

/// File names keep letters, digits, `-`, `_`, `.` (no paths out of the save directory).
fn sanitize(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect();
    let s = s.trim_start_matches('.').to_string();
    if s.is_empty() {
        "_".into()
    } else {
        s
    }
}

// ------------------------------------------------------------------ PlayerPrefs

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Pref {
    Int(i64),
    Float(f64),
    String(String),
}

/// Unity's PlayerPrefs: small settings by key.
#[derive(Default)]
pub struct PlayerPrefs {
    values: BTreeMap<String, Pref>,
    dirty: bool,
}

impl PlayerPrefs {
    pub fn get_int(&self, key: &str, default: i64) -> i64 {
        match self.values.get(key) {
            Some(Pref::Int(i)) => *i,
            Some(Pref::Float(f)) => *f as i64,
            _ => default,
        }
    }
    pub fn get_float(&self, key: &str, default: f64) -> f64 {
        match self.values.get(key) {
            Some(Pref::Float(f)) => *f,
            Some(Pref::Int(i)) => *i as f64,
            _ => default,
        }
    }
    pub fn get_string(&self, key: &str, default: &str) -> String {
        match self.values.get(key) {
            Some(Pref::String(s)) => s.clone(),
            _ => default.to_string(),
        }
    }
    pub fn get(&self, key: &str) -> Option<&Pref> {
        self.values.get(key)
    }
    pub fn set_int(&mut self, key: &str, v: i64) {
        self.set(key, Pref::Int(v));
    }
    pub fn set_float(&mut self, key: &str, v: f64) {
        self.set(key, Pref::Float(v));
    }
    pub fn set_string(&mut self, key: &str, v: &str) {
        self.set(key, Pref::String(v.into()));
    }
    pub fn set(&mut self, key: &str, v: Pref) {
        if self.values.get(key) != Some(&v) {
            self.values.insert(key.into(), v);
            self.dirty = true;
        }
    }
    pub fn has_key(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }
    pub fn delete_key(&mut self, key: &str) {
        if self.values.remove(key).is_some() {
            self.dirty = true;
        }
    }
    pub fn delete_all(&mut self) {
        if !self.values.is_empty() {
            self.values.clear();
            self.dirty = true;
        }
    }
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.values.keys()
    }
}

const PREFS_FILE: &str = "playerprefs.json";

// ------------------------------------------------------------------ the resource

/// Save data of the game (a resource).
pub struct SaveData {
    pub backend: SaveBackend,
    pub prefs: PlayerPrefs,
    since_flush: f32,
}

impl SaveData {
    /// Opens the storage and loads the PlayerPrefs.
    pub fn open(backend: SaveBackend) -> Self {
        let values = backend.read(PREFS_FILE).and_then(|b| serde_json::from_slice::<BTreeMap<String, Pref>>(&b).ok()).unwrap_or_default();
        SaveData { backend, prefs: PlayerPrefs { values, dirty: false }, since_flush: 0.0 }
    }

    /// Writes the PlayerPrefs if they changed (`PlayerPrefs.Save()`).
    pub fn flush(&mut self) -> std::io::Result<()> {
        if !self.prefs.dirty {
            return Ok(());
        }
        let bytes = serde_json::to_vec_pretty(&self.prefs.values).map_err(std::io::Error::other)?;
        self.backend.write(PREFS_FILE, &bytes)?;
        self.prefs.dirty = false;
        self.since_flush = 0.0;
        Ok(())
    }

    pub fn write_bytes(&mut self, slot: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.backend.write(&slot_file(slot), bytes)
    }
    pub fn read_bytes(&self, slot: &str) -> Option<Vec<u8>> {
        self.backend.read(&slot_file(slot))
    }
    pub fn write_json<T: Serialize>(&mut self, slot: &str, value: &T) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
        self.write_bytes(slot, &bytes)
    }
    pub fn read_json<T: DeserializeOwned>(&self, slot: &str) -> Option<T> {
        self.read_bytes(slot).and_then(|b| serde_json::from_slice(&b).ok())
    }
    pub fn delete(&mut self, slot: &str) {
        self.backend.delete(&slot_file(slot));
    }
    pub fn exists(&self, slot: &str) -> bool {
        self.backend.exists(&slot_file(slot))
    }
    /// The save slots written so far.
    pub fn slots(&self) -> Vec<String> {
        self.backend.list().into_iter().filter_map(|n| n.strip_suffix(".save").map(str::to_string)).collect()
    }
}

impl Drop for SaveData {
    fn drop(&mut self) {
        if let Err(e) = self.flush() {
            goat_core::warn!("PlayerPrefs not saved: {e}");
        }
    }
}

fn slot_file(slot: &str) -> String {
    format!("{}.save", sanitize(slot))
}

/// `goat_save::save(ctx)` (the [`SavePlugin`] opens it in `build`; in-memory without the plugin).
pub fn save<'a>(ctx: &'a mut Context) -> &'a mut SaveData {
    if ctx.resources.get::<SaveData>().is_none() {
        ctx.resources.insert(SaveData::open(SaveBackend::memory()));
    }
    ctx.resources.get_mut::<SaveData>().unwrap()
}

/// Opens the game's save storage and keeps PlayerPrefs written.
pub struct SavePlugin {
    backend: Option<SaveBackend>,
}

impl SavePlugin {
    /// The platform's user-data directory for this game (`game` = folder name).
    pub fn new(game: &str) -> Self {
        SavePlugin { backend: Some(SaveBackend::platform(game)) }
    }
    pub fn with_backend(backend: SaveBackend) -> Self {
        SavePlugin { backend: Some(backend) }
    }
}

impl Plugin for SavePlugin {
    fn build(&mut self, ctx: &mut Context) {
        let backend = self.backend.take().unwrap_or_else(SaveBackend::memory);
        if let SaveBackend::Dir(d) = &backend {
            goat_core::info!("save data: {}", d.display());
        }
        ctx.resources.insert(SaveData::open(backend));
        #[cfg(feature = "script")]
        script::register(goat_script::extensions(ctx));
    }
    fn late_update(&mut self, ctx: &mut Context) {
        let dt = ctx.time.unscaled_delta;
        if let Some(s) = ctx.resources.get_mut::<SaveData>() {
            s.since_flush += dt;
            if s.prefs.dirty && s.since_flush >= 1.0 {
                if let Err(e) = s.flush() {
                    goat_core::warn!("PlayerPrefs not saved: {e}");
                    s.since_flush = 0.0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_and_slots_persist_in_a_directory() {
        let dir = std::env::temp_dir().join(format!("goat-save-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        {
            let mut s = SaveData::open(SaveBackend::Dir(dir.clone()));
            s.prefs.set_int("highscore", 1200);
            s.prefs.set_float("volume", 0.5);
            s.prefs.set_string("name", "Goat");
            s.write_json("slot 1", &serde_json::json!({ "level": 3, "coins": [1, 2] })).unwrap();
            s.write_bytes("../escape", b"x").unwrap();
        } // dropped: prefs flushed
        let s = SaveData::open(SaveBackend::Dir(dir.clone()));
        assert_eq!(s.prefs.get_int("highscore", 0), 1200);
        assert_eq!(s.prefs.get_float("volume", 1.0), 0.5);
        assert_eq!(s.prefs.get_string("name", ""), "Goat");
        assert_eq!(s.prefs.get_int("missing", 7), 7);
        let v: serde_json::Value = s.read_json("slot 1").unwrap();
        assert_eq!(v["level"], 3);
        assert!(dir.join("slot_1.save").is_file());
        assert!(dir.join("_escape.save").is_file(), "names can't leave the save directory");
        assert_eq!(s.slots(), ["_escape", "slot_1"]);
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
