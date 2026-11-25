//! Project Hub (like Unity Hub / PiaEngine's): the projects the editor knows (recent ones in
//! `~/.goat/hub.json`, `GOAT_HOME` to move it; the engine's examples), a folder browser, new
//! projects from a 2D / 3D template.
//!
//! A GoatEngine project is a folder with `goat.project.json` and `assets/` (the editor works in
//! `assets/`; older projects kept the settings in `assets/project.json`, moved on open).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HubProject {
    pub path: String,
    pub name: String,
    /// "2d" / "3d" (project.json `dimension`), None when unknown.
    pub dimension: Option<String>,
    pub last_opened: Option<u64>,
    pub modified: Option<u64>,
    pub exists: bool,
    pub example: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct HubFile {
    #[serde(default)]
    recent: Vec<Recent>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Recent {
    path: String,
    last_opened: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    pub path: String,
    pub parent: Option<String>,
    pub is_project: bool,
    pub entries: Vec<DirEntry>,
    pub roots: Vec<Root>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_project: bool,
}

#[derive(Serialize)]
pub struct Root {
    pub label: String,
    pub path: String,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// `GOAT_HOME`, else `~/.goat`.
pub fn home() -> PathBuf {
    if let Some(h) = std::env::var_os("GOAT_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(h);
    }
    user_home().join(".goat")
}

fn user_home() -> PathBuf {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn hub_file() -> PathBuf {
    home().join("hub.json")
}

fn load() -> HubFile {
    std::fs::read_to_string(hub_file()).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or_default()
}

fn save(f: &HubFile) -> Result<(), String> {
    std::fs::create_dir_all(home()).map_err(|e| e.to_string())?;
    std::fs::write(hub_file(), serde_json::to_string_pretty(f).unwrap_or_default()).map_err(|e| e.to_string())
}

/// Paths compared the way the OS does (case-insensitive on Windows, slashes unified).
fn norm(p: &str) -> String {
    let s = p.replace('\\', "/").trim_end_matches('/').to_string();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

pub fn display(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

pub const PROJECT_FILE: &str = "goat.project.json";

pub fn is_project(dir: &Path) -> bool {
    dir.join(PROJECT_FILE).is_file() || dir.join("assets").join("project.json").is_file()
}

fn project_json(dir: &Path) -> Option<serde_json::Value> {
    let t = std::fs::read_to_string(dir.join(PROJECT_FILE)).or_else(|_| std::fs::read_to_string(dir.join("assets").join("project.json"))).ok()?;
    serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()
}

/// Older projects: assets/project.json → goat.project.json (out of the Project panel).
pub fn migrate(dir: &Path) -> Result<(), String> {
    let old = dir.join("assets").join("project.json");
    let new = dir.join(PROJECT_FILE);
    if old.is_file() && !new.exists() {
        std::fs::rename(&old, &new).map_err(|e| format!("{}: {e}", display(&old)))?;
        let _ = std::fs::remove_file(dir.join("assets").join("project.json.meta"));
    }
    Ok(())
}

fn describe(dir: &Path, last_opened: Option<u64>, example: bool) -> HubProject {
    let json = project_json(dir);
    let name = json.as_ref().and_then(|j| j.get("name")).and_then(|n| n.as_str()).filter(|n| !n.is_empty()).map(str::to_string);
    let name = name.unwrap_or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| display(dir)));
    let modified = std::fs::metadata(dir.join("assets")).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64);
    HubProject {
        path: display(dir),
        name,
        dimension: json.as_ref().and_then(|j| j.get("dimension")).and_then(|d| d.as_str()).map(str::to_string),
        last_opened,
        modified,
        exists: is_project(dir),
        example,
    }
}

/// Recent projects (newest first), then the engine's examples not opened yet.
pub fn projects(engine_root: Option<&Path>) -> Vec<HubProject> {
    let f = load();
    let mut out: Vec<HubProject> = Vec::new();
    let mut recent = f.recent.clone();
    recent.sort_by(|a, b| b.last_opened.cmp(&a.last_opened));
    let examples: Vec<PathBuf> = engine_root
        .map(|r| std::fs::read_dir(r.join("games")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| is_project(p)).collect())
        .unwrap_or_default();
    let is_example = |p: &str| examples.iter().any(|e| norm(&display(e)) == norm(p));
    for r in &recent {
        out.push(describe(Path::new(&r.path), Some(r.last_opened), is_example(&r.path)));
    }
    for e in &examples {
        if !out.iter().any(|p| norm(&p.path) == norm(&display(e))) {
            out.push(describe(e, None, true));
        }
    }
    out
}

/// Remembers `dir` as just opened.
pub fn record(dir: &Path) -> Result<(), String> {
    let mut f = load();
    let p = display(dir);
    f.recent.retain(|r| norm(&r.path) != norm(&p));
    f.recent.push(Recent { path: p, last_opened: now_ms() });
    save(&f)
}

pub fn remove(path: &str) -> Result<(), String> {
    let mut f = load();
    f.recent.retain(|r| norm(&r.path) != norm(path));
    save(&f)
}

/// The project opened last (still there), for `--last`.
pub fn last() -> Option<PathBuf> {
    let f = load();
    f.recent.iter().max_by_key(|r| r.last_opened).map(|r| PathBuf::from(&r.path)).filter(|p| is_project(p))
}

/// Where new projects go by default: `~/GoatProjects`.
pub fn default_location() -> String {
    display(&user_home().join("GoatProjects"))
}

fn roots() -> Vec<Root> {
    let mut out = vec![Root { label: "Home".into(), path: display(&user_home()) }];
    if cfg!(windows) {
        for c in b'A'..=b'Z' {
            let p = format!("{}:/", c as char);
            if Path::new(&p).is_dir() {
                out.push(Root { label: format!("{}:", c as char), path: p });
            }
        }
    } else {
        out.push(Root { label: "/".into(), path: "/".into() });
    }
    out
}

/// A folder's sub-folders (hidden ones left out); `""` = the home folder.
pub fn ls(path: &str) -> Result<DirListing, String> {
    let dir = if path.trim().is_empty() { user_home() } else { PathBuf::from(path.trim()) };
    let rd = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", display(&dir)))?;
    let mut entries: Vec<DirEntry> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| !e.file_name().to_string_lossy().starts_with(['.', '$']))
        .map(|e| DirEntry { name: e.file_name().to_string_lossy().into_owned(), path: display(&e.path()), is_project: is_project(&e.path()) })
        .collect();
    entries.sort_by_key(|e| (!e.is_project, e.name.to_lowercase()));
    Ok(DirListing { path: display(&dir), parent: dir.parent().map(display), is_project: is_project(&dir), entries, roots: roots() })
}

/// A new project from the 2D / 3D template: `location/name/assets` with project settings and a
/// Main scene. Returns the project folder.
pub fn create(name: &str, location: &str, dimension: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) {
        return Err("the project name can't contain \\ / : * ? \" < > |".into());
    }
    let dir = PathBuf::from(location.trim()).join(name);
    if dir.exists() && std::fs::read_dir(&dir).map(|mut d| d.next().is_some()).unwrap_or(false) {
        return Err(format!("{} already exists and isn't empty", display(&dir)));
    }
    let assets = dir.join("assets");
    for sub in ["scenes", "scripts", "materials", "textures", "prefabs"] {
        std::fs::create_dir_all(assets.join(sub)).map_err(|e| e.to_string())?;
    }
    let is2d = dimension == "2d";
    let settings = serde_json::json!({
        "name": name,
        "version": "1.0",
        "dimension": if is2d { "2d" } else { "3d" },
        "scenes": [{ "path": "scenes/Main.scene.json", "enabled": true }],
        "fixed_timestep": 1.0 / 60.0,
        "max_delta": 0.1,
        "gravity": [0, -9.81, 0],
        "gravity_2d": [0, -9.81],
        "tags": [],
        "layers": ["Default", "TransparentFX", "Ignore Raycast", "", "Water", "UI"]
    });
    let scene = if is2d {
        serde_json::json!({ "version": 1, "entities": [
            { "id": 1, "name": "Main Camera", "components": {
                "Transform": { "position": [0, 0, 10] },
                "Camera": { "projection": { "orthographic": { "height": 10, "near": 0.1, "far": 50 } }, "clear": "#1f2433" } } },
            { "id": 2, "name": "Square", "components": {
                "Transform": { "scale": [2, 2, 1] },
                "SpriteRenderer": { "texture": "builtin:white", "pixels_per_unit": 8, "color": "#4f8cff" } } }
        ] })
    } else {
        serde_json::json!({ "version": 1, "entities": [
            { "id": 1, "name": "Main Camera", "components": {
                "Transform": { "position": [0, 2.5, 7], "euler": [-15, 0, 0] },
                "Camera": { "projection": { "perspective": { "fov": 60, "near": 0.1, "far": 200 } }, "clear": "#1f2433" } } },
            { "id": 2, "name": "Directional Light", "components": {
                "Transform": { "euler": [-50, 30, 0] },
                "DirectionalLight": { "color": "#fff4e0", "intensity": 1.0 } } },
            { "id": 3, "name": "Ground", "components": {
                "Transform": { "scale": [2, 1, 2] },
                "MeshRenderer": { "mesh": "builtin:plane", "material": "builtin:default" } } },
            { "id": 4, "name": "Cube", "components": {
                "Transform": { "position": [0, 0.5, 0] },
                "MeshRenderer": { "mesh": "builtin:cube", "material": "builtin:default" } } }
        ] })
    };
    let write = |p: PathBuf, v: &serde_json::Value| std::fs::write(p, serde_json::to_string_pretty(v).unwrap_or_default() + "\n").map_err(|e| e.to_string());
    write(dir.join(PROJECT_FILE), &settings)?;
    write(assets.join("scenes").join("Main.scene.json"), &scene)?;
    std::fs::write(dir.join(".gitignore"), "# editor state (GUID index...), like Unity's Library\n.goat/\n").map_err(|e| e.to_string())?;
    Ok(display(&dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_list_and_forget_projects() {
        let tmp = std::env::temp_dir().join(format!("goat-hub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::set_var("GOAT_HOME", tmp.join("home"));
        let p = create("My Game", &display(&tmp), "3d").unwrap();
        assert!(is_project(Path::new(&p)));
        assert!(create("My Game", &display(&tmp), "2d").is_err(), "not over an existing project");
        assert!(create("a/b", &display(&tmp), "2d").is_err());
        record(Path::new(&p)).unwrap();
        let list = projects(None);
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].name.as_str(), list[0].dimension.as_deref()), ("My Game", Some("3d")));
        let l = ls(&display(&tmp)).unwrap();
        assert!(l.entries.iter().any(|e| e.name == "My Game" && e.is_project));
        remove(&p).unwrap();
        assert!(projects(None).is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
