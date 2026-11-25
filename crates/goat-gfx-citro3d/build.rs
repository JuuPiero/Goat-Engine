// Assembles the PICA200 vertex shaders with devkitPro's `picasso` (3DS builds only).
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=shaders");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("horizon") {
        return;
    }
    let devkitpro = std::env::var("DEVKITPRO").expect("DEVKITPRO must be set for 3DS builds");
    let exe = if cfg!(windows) { "picasso.exe" } else { "picasso" };
    let picasso = PathBuf::from(&devkitpro).join("tools").join("bin").join(exe);
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    for name in ["standard", "reflect", "outline", "receive", "shadow"] {
        let src = PathBuf::from("shaders").join(format!("{name}.v.pica"));
        let dst = out.join(format!("{name}.shbin"));
        let status = Command::new(&picasso).arg("-o").arg(&dst).arg(&src).status().unwrap_or_else(|e| panic!("running {}: {e}", picasso.display()));
        assert!(status.success(), "picasso failed on {}", src.display());
    }
}
