//! `cargo run -p goat-fbx --example fbx2glb -- <model.fbx|.obj> [out.glb] [--3ds] [--scale N]`:
//! converts a model like `goat build` does and prints what came out (meshes / submeshes,
//! materials, textures, skins, animations), then reads the GLB back through the engine's loader.

use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(input) = args.iter().find(|a| !a.starts_with("--")).map(PathBuf::from) else {
        eprintln!("usage: fbx2glb <model.fbx|.obj> [out.glb] [--3ds] [--scale N]");
        std::process::exit(2);
    };
    let out = args.iter().filter(|a| !a.starts_with("--")).nth(1).map(PathBuf::from).unwrap_or_else(|| input.with_extension("glb"));
    let mut opts = goat_fbx::ConvertOptions::default();
    if args.iter().any(|a| a == "--3ds") {
        opts.target = goat_fbx::Target::N3ds;
    }
    if let Some(i) = args.iter().position(|a| a == "--scale") {
        opts.scale = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    }
    let dir = input.parent().unwrap_or(Path::new(".")).to_path_buf();
    let name = input.file_name().unwrap().to_string_lossy().to_string();
    let bytes = std::fs::read(&input).unwrap_or_else(|e| panic!("{}: {e}", input.display()));
    let read = |k: &str| std::fs::read(dir.join(k)).ok();
    let t = std::time::Instant::now();
    let (model, images) = goat_fbx::import(&bytes, &name, &read, &opts).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    let glb = goat_assets::glb::write_glb(&model, &images);
    let secs = t.elapsed().as_secs_f32();
    std::fs::write(&out, &glb).unwrap();
    let submeshes: usize = model.meshes.iter().map(|m| m.len()).sum();
    let tris: usize = model.meshes.iter().flatten().map(|p| p.mesh.indices.len() / 3).sum();
    println!("{} → {} ({:.1} KB → {:.1} KB, {secs:.2} s)", input.display(), out.display(), bytes.len() as f64 / 1024.0, glb.len() as f64 / 1024.0);
    println!("  nodes {}, meshes {}, submeshes {submeshes}, triangles {tris}", model.nodes.len(), model.meshes.len());
    println!("  materials: {}", model.materials.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", "));
    for (i, s) in images.iter().enumerate() {
        println!("  image {i}: {}", match s {
            Some(goat_assets::glb::ImageSource::Uri(u)) => format!("file {u}"),
            Some(goat_assets::glb::ImageSource::Embedded { mime, bytes }) => format!("embedded {mime} {:.1} KB", bytes.len() as f64 / 1024.0),
            None => "missing".into(),
        });
    }
    println!("  skins {} (joints {:?}), animations: {}", model.skins.len(), model.skins.iter().map(|s| s.joints.len()).collect::<Vec<_>>(), model.animations.iter().map(|a| format!("{} {:.2}s/{}ch", a.name, a.duration, a.channels.len())).collect::<Vec<_>>().join(", "));
    let back = goat_assets::model::parse_model(&goat_assets::AssetIo::new(&dir), &name, &glb).expect("the GLB reads back");
    println!("  read back: {} meshes, {} animations", back.meshes.len(), back.animations.len());
}
