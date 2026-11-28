//! `goat`: the GoatEngine command line.
//!
//! ```text
//! goat platforms                     list the build targets and whether their toolchain is ready
//! goat doctor                        check every toolchain (what is missing and how to install it)
//! goat build <platform> [--debug]    build the player for a platform into out/<platform>/
//! goat run <platform> [--debug] [-- args...]   build, then run (desktop window / 3DS emulator)
//! ```
//!
//! Each platform is a [`Builder`] (builders/*.rs). Adding a platform = a backend crate + a platform
//! crate + a player + one Builder registered in [`builders::all`].
//!
//! Machine-specific paths (devkitPro, libclang, emulators) come from environment variables or from
//! `goat.local.toml` at the workspace root (not committed), e.g.
//! ```toml
//! devkitpro = "E:/App/devkitPro"
//! libclang = "E:/App/LLVM/bin"
//! citra = "E:/App/nightly/citra.exe"
//! ```

mod builders;
mod config;

use std::process::ExitCode;

use builders::{BuildOptions, Builder};
use config::Config;

fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  goat platforms\n  goat doctor\n  goat build <platform> [--debug]\n  goat run <platform> [--debug] [-- args...]\n  goat model <model.fbx|.obj> [out.glb] [--3ds]   convert a model to GLB (what builds do)\n\nplatforms: {}",
        builders::all().iter().map(|b| b.id()).collect::<Vec<_>>().join(", ")
    );
    ExitCode::from(2)
}

/// `goat model <in> [out] [--3ds]`: FBX / OBJ → GLB (what `goat build` does to every model it ships).
fn convert_model(args: &[String]) -> ExitCode {
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let Some(input) = files.first().map(std::path::PathBuf::from) else { return usage() };
    let out = files.get(1).map(std::path::PathBuf::from).unwrap_or_else(|| input.with_extension("glb"));
    let target = if args.iter().any(|a| a == "--3ds") { goat_fbx::Target::N3ds } else { goat_fbx::Target::Desktop };
    let dir = input.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    let name = input.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let bytes = match std::fs::read(&input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("goat: {}: {e}", input.display());
            return ExitCode::FAILURE;
        }
    };
    let read = |k: &str| std::fs::read(dir.join(k)).ok();
    let opts = goat_fbx::options_for(&name, &read, target);
    match goat_fbx::convert(&bytes, &name, &read, &opts) {
        Ok(glb) => match std::fs::write(&out, &glb) {
            Ok(()) => {
                println!("goat: {} → {} ({:.1} KB)", input.display(), out.display(), glb.len() as f64 / 1024.0);
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("goat: {}: {e}", out.display());
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("goat: {e}");
            ExitCode::FAILURE
        }
    }
}

fn find_builder(id: &str) -> Option<Box<dyn Builder>> {
    builders::all().into_iter().find(|b| b.id() == id || b.aliases().contains(&id))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else { return usage() };
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("goat: {e}");
            return ExitCode::FAILURE;
        }
    };
    match cmd {
        "model" => return convert_model(&args[1..]),
        "platforms" | "doctor" => {
            let mut all_ok = true;
            for b in builders::all() {
                let problems = b.check(&config);
                let state = if problems.is_empty() { "ready" } else { "missing toolchain" };
                println!("{:<10} {:<40} {}", b.id(), b.description(), state);
                if cmd == "doctor" {
                    for p in &problems {
                        println!("    - {p}");
                    }
                }
                all_ok &= problems.is_empty();
            }
            if cmd == "doctor" && !all_ok {
                println!("\nSet the paths in the environment or in {}", config.local_file().display());
            }
            ExitCode::SUCCESS
        }
        "build" | "run" => {
            let Some(id) = args.get(1) else { return usage() };
            let Some(builder) = find_builder(id) else {
                eprintln!("goat: unknown platform '{id}'");
                return usage();
            };
            let (own, passthrough) = match args.iter().position(|a| a == "--") {
                Some(i) => (&args[2..i], args[i + 1..].to_vec()),
                None => (&args[2..], Vec::new()),
            };
            let options = BuildOptions { release: !own.iter().any(|a| a == "--debug") };
            let problems = builder.check(&config);
            if !problems.is_empty() {
                eprintln!("goat: the {} toolchain is not ready:", builder.id());
                for p in problems {
                    eprintln!("  - {p}");
                }
                return ExitCode::FAILURE;
            }
            let artifact = match builder.build(&config, &options) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("goat: build failed: {e}");
                    return ExitCode::FAILURE;
                }
            };
            println!("built {}", artifact.display());
            if cmd == "run" {
                if let Err(e) = builder.run(&config, &artifact, &passthrough) {
                    eprintln!("goat: run failed: {e}");
                    return ExitCode::FAILURE;
                }
            }
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}
