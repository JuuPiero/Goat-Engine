use super::*;
use goat_assets::AssetIo;
use goat_gfx::NullBackend;
use goat_runtime::{Behaviour, Camera, MeshRenderer};

#[derive(Serialize, Deserialize)]
struct Spin {
    speed: f32,
}
impl Behaviour for Spin {}

/// An asset folder in the temp dir with a material, a texture, a one-triangle glTF and a prefab.
fn fixture(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("goat-scene-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("materials")).unwrap();
    std::fs::create_dir_all(dir.join("textures")).unwrap();
    std::fs::create_dir_all(dir.join("models")).unwrap();
    std::fs::create_dir_all(dir.join("prefabs")).unwrap();
    std::fs::write(dir.join("materials/red.mat.json"), r##"{ "base_color": "#ff0000", "texture": "../textures/dot.png" }"##).unwrap();
    // 8×8 white PNG
    let mut png_bytes = Vec::new();
    {
        let mut e = png::Encoder::new(&mut png_bytes, 8, 8);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&[255u8; 8 * 8 * 4]).unwrap();
    }
    std::fs::write(dir.join("textures/dot.png"), png_bytes).unwrap();
    let floats: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    let bin: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
    std::fs::write(dir.join("models/tri.bin"), bin).unwrap();
    std::fs::write(
        dir.join("models/tri.gltf"),
        r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
        "nodes":[{"name":"Tri","mesh":0,"children":[1]},{"name":"Child","translation":[0,1,0]}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}],
        "bufferViews":[{"buffer":0,"byteLength":36}],
        "buffers":[{"byteLength":36,"uri":"tri.bin"}]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("prefabs/post.prefab.json"),
        r#"{ "version": 1, "entities": [
            { "id": 1, "name": "Post", "components": { "MeshRenderer": { "mesh": "builtin:cube", "material": "materials/red.mat.json" } } },
            { "id": 2, "name": "Cap", "parent": 1, "components": { "Transform": { "position": [0, 1, 0] }, "MeshRenderer": { "mesh": "builtin:sphere" } } }
        ] }"#,
    )
    .unwrap();
    dir
}

const SCENE: &str = r##"{ "version": 1, "entities": [
  { "id": 1, "name": "Camera", "components": { "Transform": { "position": [0, 2, 5] }, "Camera": { "projection": { "orthographic": { "height": 10, "near": 0.1, "far": 50 } }, "clear": "#102030", "culling_mask": 3 } } },
  { "id": 2, "name": "Crate", "components": { "Transform": { "position": [1, 0, 0], "scale": [2, 2, 2] },
      "MeshRenderer": { "mesh": "builtin:cube", "material": "materials/red.mat.json", "layer": 4 } },
    "scripts": [ { "type": "Spin", "speed": 2.5 } ] },
  { "id": 3, "name": "Lamp", "parent": 2, "components": { "Transform": { "position": [0, 3, 0] }, "PointLight": { "color": "#ff8800", "intensity": 2, "range": 6 } } },
  { "id": 4, "name": "Model", "model": "models/tri.gltf" },
  { "id": 5, "name": "Post A", "prefab": "prefabs/post.prefab.json", "components": { "Transform": { "position": [5, 0, 0] } } },
  { "id": 6, "name": "Settings", "components": { "RenderSettings": { "ambient": "#333333", "fog": { "color": "#102030", "start": 5, "end": 20 } } } }
] }"##;

fn registry() -> SceneRegistry {
    let mut r = SceneRegistry::default();
    r.behaviour::<Spin>("Spin");
    r
}

#[test]
fn load_save_round_trip() {
    let dir = fixture("roundtrip");
    let mut gfx = NullBackend::new();
    let mut assets = AssetServer::new(AssetIo::new(&dir));
    let reg = registry();
    let file = parse("test.scene.json", SCENE).unwrap();

    let mut world = World::new();
    let roots = spawn_file(&mut world, &mut gfx, &mut assets, &reg, &file, "test.scene.json", None).unwrap();
    assert_eq!(roots.len(), 5, "Lamp is a child of Crate");

    // components came through
    let crate_e = world.query::<&Name>().iter().find(|(_, n)| n.0 == "Crate").map(|(e, _)| e).unwrap();
    let r = *world.get::<&MeshRenderer>(crate_e).unwrap();
    assert_eq!(r.layer, 4);
    let mat = gfx.material(r.material).unwrap();
    assert_eq!(mat.base_color, goat_core::Color::rgb(1.0, 0.0, 0.0));
    assert!(mat.texture.is_some(), "texture resolved relative to the material file");
    assert_eq!(world.get::<&Scripts>(crate_e).unwrap().len(), 1);
    let cam = world.query::<&Camera>().iter().next().map(|(_, c)| *c).unwrap();
    assert_eq!(cam.culling_mask, 3);
    // model: Tri (+ Child) spawned under the "Model" entity; prefab: Post + Cap under "Post A"
    for n in ["Tri", "Child", "Post", "Cap"] {
        assert!(world.query::<&Name>().iter().any(|(_, x)| x.0 == n), "{n} spawned");
    }

    // save: generated children are not stored, the rest matches the source
    let saved = save_world(&world, &assets, &reg);
    assert_eq!(saved.entities.len(), 6);
    let model = saved.entities.iter().find(|e| e.name.as_deref() == Some("Model")).unwrap();
    assert_eq!(model.model.as_deref(), Some("models/tri.gltf"));
    let crate_entry = saved.entities.iter().find(|e| e.name.as_deref() == Some("Crate")).unwrap();
    assert_eq!(crate_entry.scripts[0]["speed"], 2.5);
    assert_eq!(crate_entry.components["MeshRenderer"]["material"], "materials/red.mat.json");

    // load what was saved into a new world and save again: identical
    let mut world2 = World::new();
    spawn_file(&mut world2, &mut gfx, &mut assets, &reg, &saved, "saved", None).unwrap();
    let saved2 = save_world(&world2, &assets, &reg);
    assert_eq!(serde_json::to_string_pretty(&saved).unwrap(), serde_json::to_string_pretty(&saved2).unwrap());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn unknown_behaviour_and_component_are_skipped_not_fatal() {
    let dir = fixture("unknown");
    let mut gfx = NullBackend::new();
    let mut assets = AssetServer::new(AssetIo::new(&dir));
    let file = parse("x", r#"{ "version": 1, "entities": [ { "id": 1, "components": { "Wobble": {} }, "scripts": [ { "type": "Nope" } ] } ] }"#).unwrap();
    let mut world = World::new();
    let roots = spawn_file(&mut world, &mut gfx, &mut assets, &SceneRegistry::default(), &file, "x", None).unwrap();
    assert_eq!(roots.len(), 1);
    assert!(world.get::<&Transform>(roots[0]).is_ok());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn newer_versions_are_refused() {
    assert!(parse("x", r#"{ "version": 99, "entities": [] }"#).is_err());
}
