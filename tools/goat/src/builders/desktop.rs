//! Desktop (Windows / Linux / macOS): the host's cargo toolchain, wgpu (Vulkan / Metal).

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{copy_dir, copy_into, exe, run_command, BuildOptions, Builder, GAME_ASSETS};
use crate::config::Config;

pub struct Desktop;

const PACKAGE: &str = "goat-player-desktop";
const BIN: &str = "goat-hello";

impl Builder for Desktop {
    fn id(&self) -> &'static str {
        "desktop"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["windows", "linux", "macos", "pc"]
    }
    fn description(&self) -> &'static str {
        "this computer (wgpu: Vulkan / Metal)"
    }

    fn check(&self, _config: &Config) -> Vec<String> {
        match Command::new("cargo").arg("--version").output() {
            Ok(o) if o.status.success() => Vec::new(),
            _ => vec!["cargo not found (install Rust from https://rustup.rs)".into()],
        }
    }

    fn build(&self, config: &Config, options: &BuildOptions) -> Result<PathBuf, String> {
        let mut cmd = Command::new("cargo");
        cmd.current_dir(&config.root).args(["build", "-p", PACKAGE]);
        if options.release {
            cmd.arg("--release");
        }
        run_command(&mut cmd)?;
        let built = config.target_dir().join(options.profile()).join(exe(BIN));
        let out = config.out_dir(self.id())?;
        let assets = config.root.join(GAME_ASSETS);
        if assets.is_dir() {
            copy_dir(&assets, &out.join("assets"))?;
        }
        copy_into(&out, &[built])
    }

    fn run(&self, _config: &Config, artifact: &Path, args: &[String]) -> Result<(), String> {
        run_command(Command::new(artifact).args(args))
    }
}
