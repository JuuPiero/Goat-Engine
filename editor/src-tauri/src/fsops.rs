//! File operations of the Asset panel (no Tauri here: unit-tested).
//!
//! Keys are paths relative to the project's assets root with `/` (the engine's asset keys).
//! Sidecars (`<file>.import.json`) are hidden and travel with their file. Moving / renaming
//! rewrites every reference in the project's JSON assets (scenes, prefabs, materials), the way
//! Unity keeps references through GUIDs.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

pub const SIDECAR: &str = ".import.json";
/// `<file>.meta`: `{ "guid": "<32 hex>" }`, the file's identity (kept through renames / moves).
pub const META: &str = goat_assets::META_EXT;

/// Files that travel with an asset and are hidden in the Asset panel.
fn companions(path: &Path) -> [PathBuf; 2] {
    [PathBuf::from(format!("{}{SIDECAR}", path.display())), PathBuf::from(format!("{}{META}", path.display()))]
}

fn is_companion(name: &str) -> bool {
    name.ends_with(SIDECAR) || name.ends_with(META)
}

/// A new random GUID (32 hex chars).
pub fn new_guid() -> String {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let half = |salt: u64| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(nanos);
        h.write_u64(n);
        h.write_u64(salt);
        h.write_u32(std::process::id());
        h.finish()
    };
    format!("{:016x}{:016x}", half(1), half(2))
}

/// What [`Project::sync_guids`] found.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GuidSync {
    /// Files moved / renamed outside the editor: (old key, new key); references were updated.
    pub moved: Vec<(String, String)>,
    /// Files that got a new `.meta`.
    pub created: usize,
    /// `.meta` files whose asset is gone (deleted).
    pub orphans_removed: usize,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Entry {
    pub name: String,
    pub key: String,
    pub is_dir: bool,
    pub size: u64,
    /// Seconds since the epoch.
    pub modified: u64,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Folder {
    pub name: String,
    pub key: String,
    pub children: Vec<Folder>,
}

pub fn kind_of(name: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "folder";
    }
    let n = name.to_ascii_lowercase();
    let ends = |s: &str| n.ends_with(s);
    if ends(".scene.json") {
        "scene"
    } else if ends(".prefab.json") {
        "prefab"
    } else if ends(".mat.json") {
        "material"
    } else if ends(".controller.json") {
        "controller"
    } else if ends(".inputactions.json") {
        "input"
    } else if ends(".ts") || ends(".js") || ends(".cs") || ends(".mjs") {
        "script"
    } else if ends(".png") || ends(".jpg") || ends(".jpeg") {
        "image"
    } else if ends(".glb") || ends(".gltf") {
        "model"
    } else if ends(".wav") || ends(".ogg") {
        "audio"
    } else if ends(".ttf") || ends(".otf") {
        "font"
    } else if ends(".json") || ends(".txt") || ends(".md") {
        "text"
    } else {
        "file"
    }
}

pub struct Project {
    pub assets: PathBuf,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl Project {
    pub fn new(assets: impl Into<PathBuf>) -> Self {
        Project { assets: assets.into() }
    }

    pub fn abs(&self, key: &str) -> PathBuf {
        let mut p = self.assets.clone();
        for part in key.split('/').filter(|s| !s.is_empty() && *s != "." && *s != "..") {
            p.push(part);
        }
        p
    }

    /// The key of an absolute path inside the assets root.
    pub fn key_of(&self, abs: &Path) -> Option<String> {
        let rel = dunce(abs).strip_prefix(dunce(&self.assets)).ok()?.to_path_buf();
        Some(rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
    }

    pub fn list(&self, dir: &str) -> Result<Vec<Entry>, String> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(self.abs(dir)).map_err(err)? {
            let e = e.map_err(err)?;
            let name = e.file_name().to_string_lossy().into_owned();
            if is_companion(&name) || name.starts_with('.') {
                continue;
            }
            let md = e.metadata().map_err(err)?;
            let key = if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") };
            let modified = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            out.push(Entry { kind: kind_of(&name, md.is_dir()).into(), name, key, is_dir: md.is_dir(), size: md.len(), modified });
        }
        out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(out)
    }

    pub fn tree(&self) -> Folder {
        fn walk(p: &Project, key: &str, name: &str) -> Folder {
            let children = p.list(key).unwrap_or_default().into_iter().filter(|e| e.is_dir).map(|e| walk(p, &e.key, &e.name)).collect();
            Folder { name: name.into(), key: key.into(), children }
        }
        walk(self, "", "Assets")
    }

    /// Every file key under the root (search, reference updates).
    pub fn all_files(&self) -> Vec<String> {
        fn walk(p: &Project, dir: &str, out: &mut Vec<String>) {
            for e in p.list(dir).unwrap_or_default() {
                if e.is_dir {
                    walk(p, &e.key, out);
                } else {
                    out.push(e.key);
                }
            }
        }
        let mut out = Vec::new();
        walk(self, "", &mut out);
        out
    }

    /// `dir/name`, or `dir/name 1`, `name 2`... when taken (keeps the extension(s)).
    pub fn free_key(&self, dir: &str, name: &str) -> String {
        let join = |n: &str| if dir.is_empty() { n.to_string() } else { format!("{dir}/{n}") };
        if !self.abs(&join(name)).exists() {
            return join(name);
        }
        let (stem, ext) = match name.find('.') {
            Some(i) if i > 0 => (&name[..i], &name[i..]),
            _ => (name, ""),
        };
        let stem = stem.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end();
        (1..).map(|i| join(&format!("{stem} {i}{ext}"))).find(|k| !self.abs(k).exists()).unwrap()
    }

    pub fn create_folder(&self, dir: &str, name: &str) -> Result<String, String> {
        let key = self.free_key(dir, name);
        std::fs::create_dir_all(self.abs(&key)).map_err(err)?;
        Ok(key)
    }

    pub fn create_file(&self, dir: &str, name: &str, content: &str) -> Result<String, String> {
        let key = self.free_key(dir, name);
        std::fs::write(self.abs(&key), content).map_err(err)?;
        Ok(key)
    }

    /// Moves files / folders into `dest_dir` (renaming on collisions) and rewrites references.
    /// Returns the new keys. Moving a folder into itself is refused.
    pub fn move_into(&self, keys: &[String], dest_dir: &str) -> Result<Vec<String>, String> {
        let mut renames = Vec::new();
        for key in keys {
            if dest_dir == key || dest_dir.starts_with(&format!("{key}/")) {
                return Err(format!("can't move {key} into itself"));
            }
            let parent = key.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            if parent == dest_dir {
                renames.push((key.clone(), key.clone()));
                continue;
            }
            let name = key.rsplit('/').next().unwrap_or(key);
            let new = self.free_key(dest_dir, name);
            self.rename_path(key, &new)?;
            renames.push((key.clone(), new));
        }
        let changed: Vec<(String, String)> = renames.iter().filter(|(a, b)| a != b).cloned().collect();
        self.update_references(&changed)?;
        Ok(renames.into_iter().map(|(_, b)| b).collect())
    }

    /// Renames one item (new file name, same folder) and rewrites references.
    pub fn rename(&self, key: &str, new_name: &str) -> Result<String, String> {
        if new_name.is_empty() || new_name.contains(['/', '\\']) {
            return Err("invalid name".into());
        }
        let parent = key.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let new = if parent.is_empty() { new_name.to_string() } else { format!("{parent}/{new_name}") };
        if new == key {
            return Ok(new);
        }
        if self.abs(&new).exists() && !new.eq_ignore_ascii_case(key) {
            return Err(format!("{new_name} already exists"));
        }
        self.rename_path(key, &new)?;
        self.update_references(&[(key.to_string(), new.clone())])?;
        Ok(new)
    }

    fn rename_path(&self, from: &str, to: &str) -> Result<(), String> {
        let (a, b) = (self.abs(from), self.abs(to));
        std::fs::rename(&a, &b).map_err(|e| format!("{from}: {e}"))?;
        for (src, dst) in companions(&a).into_iter().zip(companions(&b)) {
            if src.exists() {
                std::fs::rename(&src, dst).map_err(err)?;
            }
        }
        // keep the GUID index in step (so the next sync doesn't see a move)
        let mut index = self.load_index();
        let mut changed = false;
        for v in index.values_mut() {
            if v == from {
                *v = to.to_string();
                changed = true;
            } else if let Some(rest) = v.strip_prefix(&format!("{from}/")) {
                *v = format!("{to}/{rest}");
                changed = true;
            }
        }
        if changed {
            self.save_index(&index);
        }
        Ok(())
    }

    /// Copies (external files or project items) into `dest_dir`; returns the new keys.
    pub fn copy_into(&self, sources: &[PathBuf], dest_dir: &str) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        for src in sources {
            let name = src.file_name().ok_or("bad path")?.to_string_lossy().into_owned();
            let key = self.free_key(dest_dir, &name);
            copy_recursive(src, &self.abs(&key))?;
            let sidecar = PathBuf::from(format!("{}{SIDECAR}", src.display()));
            if sidecar.is_file() {
                std::fs::copy(&sidecar, format!("{}{SIDECAR}", self.abs(&key).display())).map_err(err)?;
            }
            out.push(key);
        }
        // copies are new assets: new GUIDs (also for .meta files copied inside folders)
        self.sync_guids()?;
        Ok(out)
    }

    pub fn duplicate(&self, keys: &[String]) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        for k in keys {
            let parent = k.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            out.extend(self.copy_into(&[self.abs(k)], parent)?);
        }
        Ok(out)
    }

    /// Moves items to the OS recycle bin (with their sidecars).
    pub fn delete(&self, keys: &[String]) -> Result<(), String> {
        let mut paths = Vec::new();
        for k in keys {
            let p = self.abs(k);
            paths.extend(companions(&p).into_iter().filter(|c| c.exists()));
            paths.push(p);
        }
        trash::delete_all(paths).map_err(err)
    }

    // ------------------------------------------------------------------ GUIDs

    /// The GUID index of the last sync (`<project>/.goat/guids.json`: guid → key). Local editor
    /// state, like Unity's Library folder.
    fn index_path(&self) -> PathBuf {
        self.assets.parent().unwrap_or(&self.assets).join(".goat").join("guids.json")
    }

    fn load_index(&self) -> std::collections::BTreeMap<String, String> {
        std::fs::read_to_string(self.index_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    fn save_index(&self, index: &std::collections::BTreeMap<String, String>) {
        let p = self.index_path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let text = serde_json::to_string_pretty(index).unwrap_or_default();
        if std::fs::read_to_string(&p).ok().as_deref() != Some(text.as_str()) {
            let _ = std::fs::write(p, text);
        }
    }

    /// The GUID of an asset (from its `.meta`).
    pub fn guid_of(&self, key: &str) -> Option<String> {
        std::fs::read_to_string(format!("{}{META}", self.abs(key).display())).ok().and_then(|t| goat_assets::parse_meta_guid(&t))
    }

    fn write_meta(&self, key: &str, guid: &str) -> Result<(), String> {
        std::fs::write(format!("{}{META}", self.abs(key).display()), format!("{{ \"guid\": \"{guid}\" }}\n")).map_err(err)
    }

    /// Brings the `.meta` files and the GUID index up to date, like Unity's asset refresh:
    /// - files without a `.meta` get one (a file moved outside the editor *without* its `.meta`
    ///   takes the orphaned `.meta` of the same file name, when exactly one matches);
    /// - a `.meta` whose GUID another file already has (copied outside the editor) gets a new GUID;
    /// - `.meta` files without their asset are removed;
    /// - files whose GUID now sits at another path were moved / renamed outside the editor:
    ///   every reference to the old key is rewritten ([`Project::update_references`]).
    pub fn sync_guids(&self) -> Result<GuidSync, String> {
        let mut out = GuidSync::default();
        let old_index = self.load_index();
        let old_path_of: std::collections::HashMap<&str, &str> = old_index.iter().map(|(g, k)| (g.as_str(), k.as_str())).collect();
        // every file + every .meta (keys), hidden folders skipped
        let mut files = Vec::new();
        let mut metas = Vec::new();
        fn walk(p: &Project, dir: &str, files: &mut Vec<String>, metas: &mut Vec<String>) {
            let Ok(rd) = std::fs::read_dir(p.abs(dir)) else { return };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let key = if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") };
                if e.path().is_dir() {
                    walk(p, &key, files, metas);
                } else if name.ends_with(META) {
                    metas.push(key);
                } else if !name.ends_with(SIDECAR) {
                    files.push(key);
                }
            }
        }
        walk(self, "", &mut files, &mut metas);
        files.sort();
        let file_set: std::collections::HashSet<&str> = files.iter().map(String::as_str).collect();
        // orphaned metas, by file name (to re-attach moved files)
        let mut orphans: Vec<(String, String)> = Vec::new(); // (meta key, its file name)
        for m in &metas {
            let file = &m[..m.len() - META.len()];
            if !file_set.contains(file) {
                orphans.push((m.clone(), file.rsplit('/').next().unwrap_or(file).to_string()));
            }
        }
        let mut index = std::collections::BTreeMap::new();
        for key in &files {
            let mut guid = self.guid_of(key);
            if guid.is_none() {
                let name = key.rsplit('/').next().unwrap_or(key);
                let matches: Vec<usize> = orphans.iter().enumerate().filter(|(_, (_, n))| n == name).map(|(i, _)| i).collect();
                if matches.len() == 1 {
                    let (meta, _) = orphans.remove(matches[0]);
                    let target = format!("{}{META}", self.abs(key).display());
                    std::fs::rename(self.abs(&meta), &target).map_err(err)?;
                    guid = self.guid_of(key);
                }
            }
            let taken = guid.as_ref().is_some_and(|g| index.contains_key(g));
            let guid = match guid {
                // a duplicate GUID: the copy that isn't where the index had it gets a new one
                Some(g) if taken => {
                    let mine_by_index = old_path_of.get(g.as_str()) == Some(&key.as_str());
                    if mine_by_index {
                        // the earlier holder is the copy: swap
                        let other: String = index.remove(&g).unwrap();
                        let fresh = new_guid();
                        self.write_meta(&other, &fresh)?;
                        index.insert(fresh, other);
                        g
                    } else {
                        let fresh = new_guid();
                        self.write_meta(key, &fresh)?;
                        out.created += 1;
                        fresh
                    }
                }
                Some(g) => g,
                None => {
                    let fresh = new_guid();
                    self.write_meta(key, &fresh)?;
                    out.created += 1;
                    fresh
                }
            };
            index.insert(guid, key.clone());
        }
        for (meta, _) in orphans {
            std::fs::remove_file(self.abs(&meta)).map_err(err)?;
            out.orphans_removed += 1;
        }
        for (g, new_key) in &index {
            if let Some(old) = old_path_of.get(g.as_str()) {
                if *old != new_key && !file_set.contains(*old) {
                    out.moved.push((old.to_string(), new_key.clone()));
                }
            }
        }
        if !out.moved.is_empty() {
            self.update_references(&out.moved)?;
        }
        self.save_index(&index);
        Ok(out)
    }

    /// Rewrites references after `(old key, new key)` renames (files or folders):
    /// - every JSON asset: string values equal to an old key, or starting with it followed by
    ///   `/` (folder) or `#` (sub-asset);
    /// - material files: their relative `texture` path, recomputed from both new locations.
    pub fn update_references(&self, renames: &[(String, String)]) -> Result<usize, String> {
        if renames.is_empty() {
            return Ok(0);
        }
        let map_key = |k: &str| -> Option<String> {
            for (old, new) in renames {
                if k == old {
                    return Some(new.clone());
                }
                for sep in ['/', '#'] {
                    if let Some(rest) = k.strip_prefix(&format!("{old}{sep}")) {
                        return Some(format!("{new}{sep}{rest}"));
                    }
                }
            }
            None
        };
        // where each file was before the renames (to resolve relative paths)
        let old_location = |current: &str| -> String {
            for (old, new) in renames {
                if current == new {
                    return old.clone();
                }
                if let Some(rest) = current.strip_prefix(&format!("{new}/")) {
                    return format!("{old}/{rest}");
                }
            }
            current.to_string()
        };
        let mut changed = 0;
        for key in self.all_files() {
            if !key.ends_with(".json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(self.abs(&key)) else { continue };
            let Ok(mut v) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else { continue };
            let mut dirty = false;
            fn walk(v: &mut Value, f: &dyn Fn(&str) -> Option<String>, dirty: &mut bool) {
                match v {
                    Value::String(s) => {
                        if let Some(n) = f(s) {
                            *s = n;
                            *dirty = true;
                        }
                    }
                    Value::Array(a) => a.iter_mut().for_each(|x| walk(x, f, dirty)),
                    Value::Object(o) => o.values_mut().for_each(|x| walk(x, f, dirty)),
                    _ => {}
                }
            }
            if key.ends_with(".mat.json") {
                // the texture path is relative to the material file
                if let Some(Value::String(t)) = v.get("texture").cloned() {
                    let was_at = old_location(&key);
                    let old_target = if let Some(r) = t.strip_prefix('/') { r.to_string() } else { goat_assets::resolve_relative(&was_at, &t) };
                    let new_target = map_key(&old_target).unwrap_or(old_target);
                    let rel = relative_key(&key, &new_target);
                    if rel != t {
                        v["texture"] = Value::String(rel);
                        dirty = true;
                    }
                }
            } else {
                walk(&mut v, &map_key, &mut dirty);
            }
            if dirty {
                let out = serde_json::to_string_pretty(&v).map_err(err)?;
                std::fs::write(self.abs(&key), out + "\n").map_err(err)?;
                changed += 1;
            }
        }
        Ok(changed)
    }
}

/// `target` relative to the folder of `from` (`materials/a.mat.json`, `textures/b.png` → `../textures/b.png`).
pub fn relative_key(from: &str, target: &str) -> String {
    let from_dir: Vec<&str> = from.split('/').collect::<Vec<_>>().split_last().map(|(_, d)| d.to_vec()).unwrap_or_default();
    let to: Vec<&str> = target.split('/').collect();
    let common = from_dir.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), from_dir.len() - common).collect();
    parts.extend(to[common..].iter().map(|s| s.to_string()));
    parts.join("/")
}

fn copy_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    if src.is_dir() {
        std::fs::create_dir_all(dest).map_err(err)?;
        for e in std::fs::read_dir(src).map_err(err)? {
            let e = e.map_err(err)?;
            copy_recursive(&e.path(), &dest.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dest).map(|_| ()).map_err(|e| format!("{}: {e}", src.display()))
    }
}

/// Strips Windows' `\\?\` prefix so canonical and plain paths compare.
fn dunce(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> Project {
        let base = std::env::temp_dir().join(format!("goat-editor-fs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        // <project>/assets: the GUID index goes to <project>/.goat
        let dir = base.join("assets");
        for d in ["textures", "materials", "prefabs", "models"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        std::fs::write(dir.join("textures/a.png"), b"png").unwrap();
        std::fs::write(dir.join("textures/a.png.import.json"), b"{}").unwrap();
        std::fs::write(dir.join("models/m.glb"), b"glb").unwrap();
        std::fs::write(dir.join("materials/a.mat.json"), r#"{ "texture": "../textures/a.png" }"#).unwrap();
        std::fs::write(
            dir.join("prefabs/p.prefab.json"),
            r#"{ "version": 1, "entities": [ { "id": 1, "model": "models/m.glb", "components": { "MeshRenderer": { "mesh": "models/m.glb#mesh:0:0", "material": "materials/a.mat.json" } } } ] }"#,
        )
        .unwrap();
        Project::new(dir)
    }

    fn read(p: &Project, key: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(p.abs(key)).unwrap()).unwrap()
    }

    #[test]
    fn listing_hides_sidecars_and_sorts_folders_first() {
        let p = project("list");
        let root = p.list("").unwrap();
        assert!(root.iter().all(|e| e.is_dir));
        let tex = p.list("textures").unwrap();
        assert_eq!(tex.len(), 1);
        assert_eq!((tex[0].key.as_str(), tex[0].kind.as_str()), ("textures/a.png", "image"));
        assert_eq!(p.tree().children.len(), 4);
    }

    #[test]
    fn moving_files_and_folders_rewrites_references() {
        let p = project("move");
        // move the texture: the material's relative path follows, the sidecar too
        let new = p.move_into(&["textures/a.png".into()], "materials").unwrap();
        assert_eq!(new, vec!["materials/a.png".to_string()]);
        assert!(p.abs("materials/a.png.import.json").exists());
        assert_eq!(read(&p, "materials/a.mat.json")["texture"], "a.png");
        // rename the models folder: keys with sub-assets follow
        p.rename("models", "meshes").unwrap();
        let prefab = read(&p, "prefabs/p.prefab.json");
        assert_eq!(prefab["entities"][0]["model"], "meshes/m.glb");
        assert_eq!(prefab["entities"][0]["components"]["MeshRenderer"]["mesh"], "meshes/m.glb#mesh:0:0");
        // move the material elsewhere: its texture path is recomputed, the prefab follows
        p.create_folder("", "lib").unwrap();
        p.move_into(&["materials/a.mat.json".into()], "lib").unwrap();
        assert_eq!(read(&p, "lib/a.mat.json")["texture"], "../materials/a.png");
        assert_eq!(read(&p, "prefabs/p.prefab.json")["entities"][0]["components"]["MeshRenderer"]["material"], "lib/a.mat.json");
        // a folder can't go into itself
        assert!(p.move_into(&["lib".into()], "lib").is_err());
    }

    #[test]
    fn collisions_get_numbered_names() {
        let p = project("dup");
        let d = p.duplicate(&["textures/a.png".into()]).unwrap();
        assert_eq!(d, vec!["textures/a 1.png".to_string()]);
        assert!(p.abs("textures/a 1.png.import.json").exists());
        assert_eq!(p.free_key("textures", "a 1.png"), "textures/a 2.png");
        assert_eq!(relative_key("a/b/c.mat.json", "a/x/y.png"), "../x/y.png");
    }

    #[test]
    fn guids_follow_files_moved_outside_the_editor() {
        let p = project("guid");
        let first = p.sync_guids().unwrap();
        assert_eq!((first.created, first.moved.len()), (4, 0));
        let guid = p.guid_of("models/m.glb").unwrap();
        assert_eq!(guid.len(), 32);
        assert!(p.list("models").unwrap().iter().all(|e| !e.name.ends_with(".meta")));
        assert_eq!(p.sync_guids().unwrap(), GuidSync::default());

        // renamed in the file manager together with its .meta
        std::fs::rename(p.abs("models/m.glb"), p.abs("models/robot.glb")).unwrap();
        std::fs::rename(p.abs("models/m.glb.meta"), p.abs("models/robot.glb.meta")).unwrap();
        let s = p.sync_guids().unwrap();
        assert_eq!(s.moved, vec![("models/m.glb".to_string(), "models/robot.glb".to_string())]);
        let prefab = read(&p, "prefabs/p.prefab.json");
        assert_eq!(prefab["entities"][0]["components"]["MeshRenderer"]["mesh"], "models/robot.glb#mesh:0:0");

        // moved alone (its .meta left behind): the orphaned .meta of the same name re-attaches
        std::fs::rename(p.abs("models/robot.glb"), p.abs("prefabs/robot.glb")).unwrap();
        let s = p.sync_guids().unwrap();
        assert_eq!((s.moved.len(), s.created, s.orphans_removed), (1, 0, 0));
        assert_eq!(p.guid_of("prefabs/robot.glb").as_deref(), Some(guid.as_str()));
        assert_eq!(read(&p, "prefabs/p.prefab.json")["entities"][0]["model"], "prefabs/robot.glb");

        // copied with its .meta: the copy gets a new GUID, the original keeps its own
        std::fs::copy(p.abs("prefabs/robot.glb"), p.abs("models/copy.glb")).unwrap();
        std::fs::copy(p.abs("prefabs/robot.glb.meta"), p.abs("models/copy.glb.meta")).unwrap();
        let s = p.sync_guids().unwrap();
        assert!(s.moved.is_empty());
        assert_eq!(p.guid_of("prefabs/robot.glb").as_deref(), Some(guid.as_str()));
        assert_ne!(p.guid_of("models/copy.glb").as_deref(), Some(guid.as_str()));

        // deleted outside: its .meta goes too
        std::fs::remove_file(p.abs("models/copy.glb")).unwrap();
        assert_eq!(p.sync_guids().unwrap().orphans_removed, 1);

        // renames in the editor keep the GUID and the index (no "move" seen afterwards)
        p.rename("prefabs/robot.glb", "bot.glb").unwrap();
        assert_eq!(p.guid_of("prefabs/bot.glb").as_deref(), Some(guid.as_str()));
        assert!(p.sync_guids().unwrap().moved.is_empty());
        // duplicates in the editor get their own GUID
        let d = p.duplicate(&["prefabs/bot.glb".into()]).unwrap();
        assert_ne!(p.guid_of(&d[0]), p.guid_of("prefabs/bot.glb"));
    }
}
