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
            if name.ends_with(SIDECAR) || name.starts_with('.') {
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
        let sidecar = PathBuf::from(format!("{}{SIDECAR}", a.display()));
        if sidecar.exists() {
            std::fs::rename(&sidecar, format!("{}{SIDECAR}", b.display())).map_err(err)?;
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
            let sidecar = PathBuf::from(format!("{}{SIDECAR}", p.display()));
            if sidecar.exists() {
                paths.push(sidecar);
            }
            paths.push(p);
        }
        trash::delete_all(paths).map_err(err)
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
        let dir = std::env::temp_dir().join(format!("goat-editor-fs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
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
}
