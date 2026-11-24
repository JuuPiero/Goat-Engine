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
