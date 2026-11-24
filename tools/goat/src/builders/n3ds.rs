//! Nintendo 3DS: nightly Rust (`armv6k-nintendo-3ds`, build-std through cargo-3ds), devkitARM +
//! libctru + citro3d from devkitPro, bindgen (libclang). Output: a homebrew `.3dsx` (+ `.smdh`).

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{copy_into, exe, run_command, stage_assets, BuildOptions, Builder};
use crate::config::Config;

pub struct Nintendo3ds;

const PACKAGE: &str = "goat-player-3ds";
const BIN: &str = "goat-hello";
const TARGET: &str = "armv6k-nintendo-3ds";

impl Nintendo3ds {
    fn devkitpro(config: &Config) -> Option<PathBuf> {
        config.path("DEVKITPRO", "devkitpro").or_else(|| {
            [r"C:\devkitPro", "/opt/devkitpro"].into_iter().map(PathBuf::from).find(|p| p.is_dir())
        })
    }

    fn libclang(config: &Config) -> Option<PathBuf> {
        config.path("LIBCLANG_PATH", "libclang")
    }

    /// cargo with the devkitPro environment cargo-3ds and the -sys crates' build scripts expect.
    fn cargo(config: &Config) -> Result<Command, String> {
        let dkp = Self::devkitpro(config).ok_or("devkitPro not found")?;
        let arm = dkp.join("devkitARM");
        let mut path = vec![arm.join("bin"), dkp.join("tools").join("bin")];
        path.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        let mut cmd = Command::new("cargo");
        cmd.current_dir(&config.root)
            .env("DEVKITPRO", &dkp)
            .env("DEVKITARM", &arm)
            .env("PATH", std::env::join_paths(path).map_err(|e| e.to_string())?);
        if let Some(clang) = Self::libclang(config) {
            cmd.env("LIBCLANG_PATH", clang);
        }
        Ok(cmd)
    }
}

impl Builder for Nintendo3ds {
    fn id(&self) -> &'static str {
        "3ds"
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["n3ds", "nintendo-3ds"]
    }
    fn description(&self) -> &'static str {
        "Nintendo 3DS homebrew .3dsx (citro3d)"
    }

    fn check(&self, config: &Config) -> Vec<String> {
        let mut problems = Vec::new();
        let nightly = Command::new("rustup").args(["toolchain", "list"]).output();
        if !nightly.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("nightly")) {
            problems.push("Rust nightly missing: rustup toolchain install nightly --component rust-src".into());
        }
        let cargo3ds = Command::new("cargo").args(["3ds", "--help"]).output();
        if !cargo3ds.is_ok_and(|o| o.status.success()) {
            problems.push("cargo-3ds missing: cargo install --locked cargo-3ds".into());
        }
        match Self::devkitpro(config) {
            None => problems.push("devkitPro not found: install it (3ds-dev group) and set DEVKITPRO or `devkitpro` in goat.local.toml".into()),
            Some(dkp) => {
                if !dkp.join("devkitARM").join("bin").is_dir() {
                    problems.push(format!("{} has no devkitARM (install the 3ds-dev group)", dkp.display()));
                }
                if !dkp.join("libctru").join("lib").join("libcitro3d.a").is_file() {
                    problems.push(format!("{} has no libctru/citro3d (install the 3ds-dev group)", dkp.display()));
                }
                if !dkp.join("tools").join("bin").join(exe("picasso")).is_file() {
                    problems.push("picasso (PICA200 shader assembler) missing from devkitPro tools".into());
                }
            }
        }
        if cfg!(windows) && Self::libclang(config).is_none_or(|p| !p.join("libclang.dll").is_file()) {
            problems.push("libclang (bindgen) not found: install LLVM and set LIBCLANG_PATH or `libclang` (its bin folder)".into());
        }
        problems
    }

    fn build(&self, config: &Config, options: &BuildOptions) -> Result<PathBuf, String> {
        // romfs: the assets with scripts compiled (players/3ds/Cargo.toml points cargo-3ds here)
        stage_assets(config, &config.target_dir().join("3ds-romfs"))?;
        let mut cmd = Self::cargo(config)?;
        cmd.args(["+nightly", "3ds", "build", "-p", PACKAGE]);
        if options.release {
            cmd.arg("--release");
        }
        run_command(&mut cmd)?;
        let dir = config.target_dir().join(TARGET).join(options.profile());
        copy_into(&config.out_dir(self.id())?, &[dir.join(format!("{BIN}.3dsx")), dir.join(format!("{BIN}.smdh"))])
    }

    /// Opens the .3dsx in an emulator (`GOAT_CITRA` / `citra` in goat.local.toml: a Citra / Azahar
    /// executable). On hardware: copy it to /3ds/ on the SD card or send it with `3dslink`.
    fn run(&self, config: &Config, artifact: &Path, args: &[String]) -> Result<(), String> {
        let emulator = config
            .path("GOAT_CITRA", "citra")
            .ok_or("no emulator configured: set GOAT_CITRA or `citra` in goat.local.toml (or copy the .3dsx to the SD card)")?;
        run_command(Command::new(emulator).args(args).arg(artifact))
    }
}
