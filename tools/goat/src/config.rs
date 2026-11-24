//! Workspace location + machine-specific tool paths (env vars, then `goat.local.toml`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Config {
    /// The GoatEngine workspace root (the directory holding the workspace Cargo.toml).
    pub root: PathBuf,
    local: HashMap<String, String>,
}

impl Config {
    pub fn load() -> Result<Config, String> {
        let root = find_root().ok_or("not inside a GoatEngine workspace (no Cargo.toml with [workspace] above the current directory)")?;
        let mut local = HashMap::new();
        let file = root.join("goat.local.toml");
        if let Ok(text) = std::fs::read_to_string(&file) {
            for (n, line) in text.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                    continue;
                }
                let (k, v) = line.split_once('=').ok_or_else(|| format!("{}:{}: expected key = \"value\"", file.display(), n + 1))?;
                local.insert(k.trim().to_string(), v.trim().trim_matches('"').replace("\\\\", "\\"));
            }
        }
        Ok(Config { root, local })
    }

    pub fn local_file(&self) -> PathBuf {
        self.root.join("goat.local.toml")
    }

    /// A tool path: the environment variable first, then `goat.local.toml`'s `key` — the first one
    /// that exists (devkitPro's Windows installer sets DEVKITPRO=/opt/devkitpro, an MSYS path
    /// native programs can't open).
    pub fn path(&self, env: &str, key: &str) -> Option<PathBuf> {
        let candidates: Vec<PathBuf> = [
            std::env::var_os(env).filter(|v| !v.is_empty()).map(PathBuf::from),
            self.local.get(key).map(PathBuf::from),
        ]
        .into_iter()
        .flatten()
        .collect();
        candidates.iter().find(|p| p.exists()).or(candidates.first()).cloned()
    }

    /// The target directory cargo uses for this workspace.
    pub fn target_dir(&self) -> PathBuf {
        std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| self.root.join("target"))
    }

    /// `out/<platform>/`, created.
    pub fn out_dir(&self, platform: &str) -> Result<PathBuf, String> {
        let dir = self.root.join("out").join(platform);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(dir)
    }
}

fn find_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let mut dir: Option<&Path> = Some(&cwd);
    while let Some(d) = dir {
        let manifest = d.join("Cargo.toml");
        if std::fs::read_to_string(&manifest).is_ok_and(|t| t.contains("[workspace]") && t.contains("goat-core")) {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}
