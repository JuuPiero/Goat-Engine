//! GoatEngine editor: the Tauri shell. The UI (React, `editor/src`) calls these commands; file
//! system changes are pushed as `assets-changed` events (watcher), drops from the OS / other
//! editor windows arrive through Tauri's drag-drop events in the UI.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod fsops;

use std::path::PathBuf;
use std::sync::Mutex;

use fsops::{Entry, Folder, Project};
use serde::Serialize;
use tauri::{Emitter, Manager, State};

struct AppState {
    project: Mutex<Project>,
    _watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

#[derive(Serialize)]
struct ProjectInfo {
    name: String,
    root: String,
    assets: String,
}

type Res<T> = Result<T, String>;

fn with<T>(s: &State<AppState>, f: impl FnOnce(&Project) -> Res<T>) -> Res<T> {
    let guard = s.project.lock().map_err(|e| e.to_string())?;
    f(&guard)
}

#[tauri::command]
fn project_info(s: State<AppState>) -> Res<ProjectInfo> {
    with(&s, |p| {
        let root = p.assets.parent().unwrap_or(&p.assets).to_path_buf();
        Ok(ProjectInfo { name: root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), root: root.display().to_string(), assets: p.assets.display().to_string() })
    })
}
#[tauri::command]
fn list_dir(s: State<AppState>, dir: String) -> Res<Vec<Entry>> {
    with(&s, |p| p.list(&dir))
}
#[tauri::command]
fn folder_tree(s: State<AppState>) -> Res<Folder> {
    with(&s, |p| Ok(p.tree()))
}
#[tauri::command]
fn search(s: State<AppState>, query: String) -> Res<Vec<Entry>> {
    let q = query.to_lowercase();
    with(&s, |p| {
        let mut out = Vec::new();
        for key in p.all_files() {
            let name = key.rsplit('/').next().unwrap_or(&key).to_string();
            if name.to_lowercase().contains(&q) {
                let md = std::fs::metadata(p.abs(&key)).map_err(|e| e.to_string())?;
                out.push(Entry { kind: fsops::kind_of(&name, false).into(), name, key, is_dir: false, size: md.len(), modified: 0 });
            }
        }
        Ok(out)
    })
}
#[tauri::command]
fn abs_paths(s: State<AppState>, keys: Vec<String>) -> Res<Vec<String>> {
    with(&s, |p| Ok(keys.iter().map(|k| p.abs(k).display().to_string()).collect()))
}
/// Keys of absolute paths inside the project (None for outside ones).
#[tauri::command]
fn keys_of(s: State<AppState>, paths: Vec<String>) -> Res<Vec<Option<String>>> {
    with(&s, |p| Ok(paths.iter().map(|a| p.key_of(&PathBuf::from(a))).collect()))
}
#[tauri::command]
fn move_items(s: State<AppState>, keys: Vec<String>, dest: String) -> Res<Vec<String>> {
    with(&s, |p| p.move_into(&keys, &dest))
}
#[tauri::command]
fn copy_paths(s: State<AppState>, paths: Vec<String>, dest: String) -> Res<Vec<String>> {
    with(&s, |p| p.copy_into(&paths.iter().map(PathBuf::from).collect::<Vec<_>>(), &dest))
}
#[tauri::command]
fn rename_item(s: State<AppState>, key: String, name: String) -> Res<String> {
    with(&s, |p| p.rename(&key, &name))
}
#[tauri::command]
fn duplicate_items(s: State<AppState>, keys: Vec<String>) -> Res<Vec<String>> {
    with(&s, |p| p.duplicate(&keys))
}
#[tauri::command]
fn delete_items(s: State<AppState>, keys: Vec<String>) -> Res<()> {
    with(&s, |p| p.delete(&keys))
}
#[tauri::command]
fn create_folder(s: State<AppState>, dir: String, name: String) -> Res<String> {
    with(&s, |p| p.create_folder(&dir, &name))
}
#[tauri::command]
fn create_asset(s: State<AppState>, dir: String, kind: String) -> Res<String> {
    let (name, content) = match kind.as_str() {
        "material" => ("New Material.mat.json", "{\n  \"base_color\": \"#ffffff\",\n  \"shininess\": 32\n}\n"),
        "scene" => ("New Scene.scene.json", "{\n  \"version\": 1,\n  \"entities\": []\n}\n"),
        "prefab" => ("New Prefab.prefab.json", "{\n  \"version\": 1,\n  \"entities\": [ { \"id\": 1, \"name\": \"Root\" } ]\n}\n"),
        "controller" => (
            "New Animator Controller.controller.json",
            "{\n  \"parameters\": [],\n  \"layers\": [\n    { \"name\": \"Base Layer\", \"states\": [ { \"name\": \"Idle\", \"clip\": \"\", \"position\": [260, 120] } ], \"transitions\": [] }\n  ]\n}\n",
        ),
        _ => return Err(format!("unknown asset kind {kind}")),
    };
    with(&s, |p| p.create_file(&dir, name, content))
}
#[tauri::command]
fn read_text(s: State<AppState>, key: String) -> Res<String> {
    with(&s, |p| std::fs::read_to_string(p.abs(&key)).map_err(|e| e.to_string()))
}
#[tauri::command]
fn write_text(s: State<AppState>, key: String, text: String) -> Res<()> {
    with(&s, |p| std::fs::write(p.abs(&key), text).map_err(|e| e.to_string()))
}
/// Shows the item in the OS file manager.
#[tauri::command]
fn reveal(s: State<AppState>, key: String) -> Res<()> {
    with(&s, |p| {
        let path = p.abs(&key);
        #[cfg(windows)]
        let r = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
        #[cfg(target_os = "macos")]
        let r = std::process::Command::new("open").arg("-R").arg(&path).spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let r = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(&path)).spawn();
        r.map(|_| ()).map_err(|e| e.to_string())
    })
}
/// A PNG shown under the cursor while dragging out of the editor.
#[tauri::command]
fn drag_icon(app: tauri::AppHandle) -> Res<String> {
    let path = app.path().temp_dir().map_err(|e| e.to_string())?.join("goat-editor-drag.png");
    if !path.exists() {
        std::fs::write(&path, include_bytes!("../icons/drag.png")).map_err(|e| e.to_string())?;
    }
    Ok(path.display().to_string())
}

/// The project: `--project <dir>` (a game folder with `assets/`), else the sample next to the
/// engine sources, else the current folder.
fn find_project() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--project") {
        if let Some(dir) = args.get(i + 1) {
            return PathBuf::from(dir).join("assets");
        }
    }
    let mut dir = std::env::current_dir().unwrap_or_default();
    loop {
        let sample = dir.join("games").join("hello").join("assets");
        if sample.is_dir() {
            return sample;
        }
        if !dir.pop() {
            break;
        }
    }
    PathBuf::from("assets")
}

/// tools/goat-cs (C# → JavaScript), found above the editor executable.
fn goat_cs_dll() -> Option<PathBuf> {
    let mut d = std::env::current_exe().ok()?;
    while d.pop() {
        let p = d.join("tools").join("goat-cs").join("bin").join("goat-cs.dll");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

#[derive(Clone, serde::Serialize)]
struct ScriptLog {
    level: &'static str,
    text: String,
}

/// Translates changed C# scripts (debounced), reporting to the Console (`script-log` events).
fn csharp_worker(app: tauri::AppHandle, root: PathBuf) -> std::sync::mpsc::Sender<PathBuf> {
    let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
    std::thread::spawn(move || {
        let log = |level: &'static str, text: String| {
            let _ = app.emit("script-log", ScriptLog { level, text });
        };
        while let Ok(first) = rx.recv() {
            let mut files = std::collections::BTreeSet::from([first]);
            while let Ok(p) = rx.recv_timeout(std::time::Duration::from_millis(250)) {
                files.insert(p);
            }
            files.retain(|f| f.is_file());
            if files.is_empty() {
                continue;
            }
            let Some(dll) = goat_cs_dll() else {
                log("warn", "C#: tools/goat-cs isn't built (cd tools/goat-cs && dotnet build -c Release -o bin)".into());
                continue;
            };
            match std::process::Command::new("dotnet").arg(&dll).arg(&root).args(&files).output() {
                Ok(out) => {
                    for l in String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.trim().is_empty()) {
                        log("info", format!("C#: {l}"));
                    }
                    for l in String::from_utf8_lossy(&out.stderr).lines().filter(|l| !l.trim().is_empty()) {
                        log("error", l.to_string());
                    }
                }
                Err(e) => log("error", format!("C#: can't run dotnet ({e}) — install the .NET SDK")),
            }
        }
    });
    tx
}

fn watch(app: tauri::AppHandle, root: PathBuf) -> Option<notify::RecommendedWatcher> {
    use notify::{EventKind, RecursiveMode, Watcher};
    let cs = csharp_worker(app.clone(), root.clone());
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            if matches!(ev.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                for p in ev.paths.iter().filter(|p| p.extension().is_some_and(|e| e == "cs")) {
                    let _ = cs.send(p.clone());
                }
            }
            let _ = app.emit("assets-changed", ());
        }
    })
    .ok()?;
    w.watch(&root, RecursiveMode::Recursive).ok()?;
    Some(w)
}

fn main() {
    let assets = find_project();
    tauri::Builder::default()
        .plugin(tauri_plugin_drag::init())
        .setup(move |app| {
            let watcher = watch(app.handle().clone(), assets.clone());
            app.manage(AppState { project: Mutex::new(Project::new(assets.clone())), _watcher: Mutex::new(watcher) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            project_info,
            list_dir,
            folder_tree,
            search,
            abs_paths,
            keys_of,
            move_items,
            copy_paths,
            rename_item,
            duplicate_items,
            delete_items,
            create_folder,
            create_asset,
            read_text,
            write_text,
            reveal,
            drag_icon
        ])
        .run(tauri::generate_context!())
        .expect("goat-editor");
}
