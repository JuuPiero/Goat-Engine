//! Platform builders. One file per platform; register it in [`all`].

mod desktop;
mod n3ds;

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;

pub struct BuildOptions {
    pub release: bool,
}

impl BuildOptions {
    pub fn profile(&self) -> &'static str {
        if self.release {
            "release"
        } else {
            "debug"
        }
    }
}

/// Builds (and runs) the player of one platform.
pub trait Builder {
    /// Command-line id (`goat build <id>`).
    fn id(&self) -> &'static str;
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }
    fn description(&self) -> &'static str;
    /// What is missing to build for this platform (empty = ready).
    fn check(&self, config: &Config) -> Vec<String>;
    /// Builds and copies the result into `out/<id>/`; returns the main artifact.
    fn build(&self, config: &Config, options: &BuildOptions) -> Result<PathBuf, String>;
    /// Runs a built artifact (a window, an emulator...).
    fn run(&self, config: &Config, artifact: &Path, args: &[String]) -> Result<(), String>;
}

pub fn all() -> Vec<Box<dyn Builder>> {
    vec![Box::new(desktop::Desktop), Box::new(n3ds::Nintendo3ds)]
}

/// Runs a command, streaming its output; fails on a non-zero exit.
pub(crate) fn run_command(cmd: &mut Command) -> Result<(), String> {
    let shown = format!("{cmd:?}");
    let status = cmd.status().map_err(|e| format!("{shown}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{shown} exited with {status}"))
    }
}

/// Copies `files` into `dir`, returning the copy of the first one.
pub(crate) fn copy_into(dir: &Path, files: &[PathBuf]) -> Result<PathBuf, String> {
    let mut first = None;
    for f in files {
        let name = f.file_name().ok_or_else(|| format!("bad artifact path {}", f.display()))?;
        let dest = dir.join(name);
        std::fs::copy(f, &dest).map_err(|e| format!("copy {} → {}: {e}", f.display(), dest.display()))?;
        first.get_or_insert(dest);
    }
    first.ok_or_else(|| "nothing to copy".into())
}

/// The sample game's assets folder (copied next to desktop builds; packed into romfs on the 3DS).
pub(crate) const GAME_ASSETS: &str = "games/hello/assets";

/// The assets folder a build ships: `GOAT_ASSETS` (the editor's open project) or [`GAME_ASSETS`].
pub(crate) fn game_assets(config: &Config) -> std::path::PathBuf {
    std::env::var_os("GOAT_ASSETS").filter(|v| !v.is_empty()).map(std::path::PathBuf::from).unwrap_or_else(|| config.root.join(GAME_ASSETS))
}

/// The game's assets as a build ships them, in `dest`: a copy where scripts are compiled —
/// C# → JavaScript (tools/goat-cs, needs the .NET SDK when there are .cs files), TypeScript →
/// JavaScript (oxc) — and their sources dropped (consoles load the JavaScript); then baked for
/// the target ([`bake_assets`]: binary JSON, precompressed textures) unless `GOAT_NO_BAKE` is set.
pub(crate) fn stage_assets(config: &Config, dest: &Path, target: goat_assets::bake::Target) -> Result<(), String> {
    let src = game_assets(config);
    copy_dir(&src, dest)?;
    // the project settings (goat.project.json next to assets/) ship at the root of the assets
    if let Some(settings) = src.parent().map(|p| p.join(goat_runtime_settings_file())).filter(|p| p.is_file()) {
        std::fs::copy(&settings, dest.join(goat_runtime_settings_file())).map_err(|e| format!("{}: {e}", settings.display()))?;
    }
    let mut files = Vec::new();
    walk(dest, &mut files);
    if files.iter().any(|f| f.extension().is_some_and(|e| e == "cs")) {
        goat_cs(config, dest)?;
    }
    let mut compiled = 0;
    for f in &files {
        let name = f.to_string_lossy();
        if name.ends_with(".ts") && !name.ends_with(".d.ts") {
            let key = f.strip_prefix(dest).unwrap_or(f).to_string_lossy().replace('\\', "/");
            let text = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
            let js = goat_script_js::compile_typescript(&key, &text).map_err(|e| format!("{key}: {e}"))?;
            std::fs::write(f.with_extension("js"), js).map_err(|e| format!("{}: {e}", f.display()))?;
            std::fs::remove_file(f).map_err(|e| e.to_string())?;
            // the script keeps its GUID under its built name
            let meta = std::path::PathBuf::from(format!("{name}.meta"));
            if meta.is_file() {
                let _ = std::fs::rename(&meta, format!("{}.meta", f.with_extension("js").display()));
            }
            compiled += 1;
        } else if name.ends_with(".cs") || name.ends_with(".d.ts") {
            std::fs::remove_file(f).map_err(|e| e.to_string())?;
        }
    }
    if compiled > 0 {
        println!("goat: compiled {compiled} TypeScript file(s)");
    }
    if std::env::var_os("GOAT_NO_BAKE").is_none() {
        bake_assets(dest, target)?;
    }
    Ok(())
}

/// JSON → binary JSON and textures → the target GPU's block compression, in place (same names:
/// loaders look at the bytes). Prints what it saved.
pub(crate) fn bake_assets(dest: &Path, target: goat_assets::bake::Target) -> Result<(), String> {
    use goat_assets::bake::{bake_json, bake_texture, is_bakeable_json};
    let mut files = Vec::new();
    walk(dest, &mut files);
    let (mut json_n, mut json_before, mut json_after) = (0usize, 0u64, 0u64);
    let (mut tex_n, mut tex_before, mut tex_after) = (0usize, 0u64, 0u64);
    let mut codecs = std::collections::BTreeMap::<String, usize>::new();
    // FBX / OBJ models → GLB under the same name (loaders look at the bytes), before the textures:
    // the files they use are compressed with the others; embedded ones are compressed here
    let (mut model_n, mut model_before, mut model_after) = (0usize, 0u64, 0u64);
    let fbx_target = if target == goat_assets::bake::Target::N3ds { goat_fbx::Target::N3ds } else { goat_fbx::Target::Desktop };
    for f in &files {
        let key = f.strip_prefix(dest).unwrap_or(f).to_string_lossy().replace('\\', "/");
        if !goat_fbx::is_convertible(&key) {
            continue;
        }
        let bytes = std::fs::read(f).map_err(|e| format!("{key}: {e}"))?;
        let read = |k: &str| std::fs::read(dest.join(k)).ok();
        let opts = goat_fbx::options_for(&key, &read, fbx_target);
        match goat_fbx::convert(&bytes, &key, &read, &opts) {
            Ok(glb) => {
                model_n += 1;
                model_before += bytes.len() as u64;
                model_after += glb.len() as u64;
                std::fs::write(f, glb).map_err(|e| format!("{key}: {e}"))?;
            }
            Err(e) => println!("goat: {key}: not converted ({e})"),
        }
    }
    // textures first: they read their `.import.json` as text
    for f in &files {
        let key = f.strip_prefix(dest).unwrap_or(f).to_string_lossy().replace('\\', "/");
        let ext = f.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
            continue;
        }
        let bytes = std::fs::read(f).map_err(|e| format!("{key}: {e}"))?;
        let import = std::fs::read_to_string(format!("{}.import.json", f.display())).ok();
        match bake_texture(&bytes, &ext, import.as_deref(), target) {
            Ok(Some((ktx, codec))) => {
                tex_n += 1;
                tex_before += bytes.len() as u64;
                tex_after += ktx.len() as u64;
                *codecs.entry(format!("{codec:?}")).or_default() += 1;
                std::fs::write(f, ktx).map_err(|e| format!("{key}: {e}"))?;
            }
            Ok(None) => {}
            Err(e) => println!("goat: {key}: kept as it is ({e})"),
        }
    }
    for f in &files {
        let key = f.strip_prefix(dest).unwrap_or(f).to_string_lossy().replace('\\', "/");
        if !is_bakeable_json(&key) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        if let Some(bin) = bake_json(&text) {
            json_n += 1;
            json_before += text.len() as u64;
            json_after += bin.len() as u64;
            std::fs::write(f, bin).map_err(|e| format!("{key}: {e}"))?;
        }
    }
    let kb = |b: u64| b as f64 / 1024.0;
    if model_n > 0 {
        println!("goat: {model_n} FBX / OBJ model(s) → GLB: {:.1} KB → {:.1} KB", kb(model_before), kb(model_after));
    }
    if json_n > 0 {
        println!("goat: {json_n} JSON file(s) → binary: {:.1} KB → {:.1} KB", kb(json_before), kb(json_after));
    }
    if tex_n > 0 {
        let list: Vec<String> = codecs.iter().map(|(c, n)| format!("{n} {c}")).collect();
        println!("goat: {tex_n} texture(s) precompressed ({}): {:.1} KB → {:.1} KB on disk", list.join(", "), kb(tex_before), kb(tex_after));
    }
    Ok(())
}

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Runs tools/goat-cs on a folder (building the tool first when needed).
fn goat_cs(config: &Config, dir: &Path) -> Result<(), String> {
    let tool_dir = config.root.join("tools").join("goat-cs");
    let dll = tool_dir.join("bin").join("goat-cs.dll");
    let stale = std::fs::metadata(&dll).and_then(|m| m.modified()).ok().is_none_or(|built| {
        ["Program.cs", "JsWriter.cs", "Api/GoatEngine.cs"].iter().any(|f| std::fs::metadata(tool_dir.join(f)).and_then(|m| m.modified()).is_ok_and(|m| m > built))
    });
    if stale {
        println!("goat: building tools/goat-cs (C# → JavaScript)");
        run_command(std::process::Command::new("dotnet").current_dir(&tool_dir).args(["build", "-c", "Release", "-o", "bin", "-v", "q"]))
            .map_err(|e| format!("C# scripts need the .NET SDK (dotnet) to build tools/goat-cs: {e}"))?;
    }
    run_command(std::process::Command::new("dotnet").arg(&dll).arg(dir))
}

/// Copies a folder tree (assets) into `dest`, replacing what is there.
pub(crate) fn copy_dir(src: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        std::fs::remove_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let to = dest.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
    }
    Ok(())
}

pub(crate) fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// `goat.project.json` (goat-runtime's `PROJECT_FILE`).
fn goat_runtime_settings_file() -> &'static str {
    "goat.project.json"
}
