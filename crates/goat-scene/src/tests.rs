use super::*;
use goat_assets::AssetIo;
use goat_gfx::NullBackend;
use goat_runtime::{Behaviour, Camera, MeshFilter, MeshRenderer};

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
    // the old MeshRenderer { mesh, material } became MeshFilter + MeshRenderer (Unity)
    let r = (*world.get::<&MeshRenderer>(crate_e).unwrap()).clone();
    assert_eq!(r.layer, 4);
    assert_eq!(world.get::<&MeshFilter>(crate_e).unwrap().submeshes.len(), 1);
    let mat = gfx.material(r.material().unwrap()).unwrap();
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
    assert_eq!(crate_entry.components["MeshRenderer"]["materials"][0], "materials/red.mat.json");
    assert_eq!(crate_entry.components["MeshFilter"]["mesh"], "builtin:cube");
    assert!(crate_entry.components["MeshRenderer"].get("mesh").is_none(), "saved in the new format");

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

fn named(world: &World, n: &str) -> Entity {
    world.query::<&Name>().iter().find(|(_, x)| x.0 == n).map(|(e, _)| e).unwrap_or_else(|| panic!("{n} not spawned"))
}

/// Adds a prefab nesting post.prefab.json, and a variant of post.
fn prefab_fixture(name: &str) -> std::path::PathBuf {
    let dir = fixture(name);
    std::fs::write(
        dir.join("prefabs/gate.prefab.json"),
        r#"{ "version": 1, "entities": [
            { "id": 1, "name": "Gate" },
            { "id": 2, "name": "Left", "parent": 1, "prefab": "prefabs/post.prefab.json", "components": { "Transform": { "position": [-1, 0, 0] } } },
            { "id": 3, "name": "Right", "parent": 1, "prefab": "prefabs/post.prefab.json",
              "overrides": [ { "target": [2], "name": "Right Cap" } ] }
        ] }"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("prefabs/tall post.prefab.json"),
        r#"{ "version": 1, "base": "prefabs/post.prefab.json",
            "overrides": [ { "target": [2], "components": { "Transform": { "position": [0, 3, 0] } } } ],
            "entities": [ { "id": 10, "name": "Flag", "parent": 1, "components": { "Transform": { "position": [0, 4, 0] } } } ] }"#,
    )
    .unwrap();
    dir
}

fn spawn_text(dir: &std::path::Path, text: &str) -> (World, NullBackend, AssetServer) {
    let mut gfx = NullBackend::new();
    let mut assets = AssetServer::new(AssetIo::new(dir));
    let mut world = World::new();
    let file = parse("s", text).unwrap();
    spawn_file(&mut world, &mut gfx, &mut assets, &registry(), &file, "s", None).unwrap();
    (world, gfx, assets)
}

fn local_y(world: &World, n: &str) -> f32 {
    world.get::<&Transform>(named(world, n)).unwrap().position.y
}

#[test]
fn overrides_reach_nested_prefabs_and_variants_extend_their_base() {
    let dir = prefab_fixture("overrides");
    let scene = r#"{ "version": 1, "entities": [
        { "id": 1, "name": "G", "prefab": "prefabs/gate.prefab.json",
          "overrides": [ { "target": [3, 2], "components": { "Transform": { "position": [0, 2, 0] } } },
                         { "target": [2, 1], "components": { "MeshRenderer": null } } ] },
        { "id": 2, "name": "V", "prefab": "prefabs/tall post.prefab.json" }
    ] }"#;
    let (world, _gfx, _assets) = spawn_text(&dir, scene);
    // the gate's own override renamed the right cap, the scene's override moved it
    assert_eq!(local_y(&world, "Right Cap"), 2.0);
    // the scene removed the left post's renderer (null patch); the right one keeps it
    let posts: Vec<Entity> = world.query::<&Name>().iter().filter(|(_, n)| n.0 == "Post").map(|(e, _)| e).collect();
    assert_eq!(posts.len(), 3);
    assert_eq!(posts.iter().filter(|&&e| world.get::<&MeshRenderer>(e).is_ok()).count(), 2);
    // the variant: base cap raised, plus its own flag
    assert!(world.query::<&Name>().iter().any(|(_, n)| n.0 == "Flag"));
    let caps: Vec<f32> = world.query::<(&Name, &Transform)>().iter().filter(|(_, (n, _))| n.0 == "Cap").map(|(_, (_, t))| t.position.y).collect();
    assert!(caps.contains(&3.0), "{caps:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn edits_inside_instances_save_as_overrides() {
    let dir = prefab_fixture("save-overrides");
    set_track_overrides(true);
    let scene = r#"{ "version": 1, "entities": [
        { "id": 1, "name": "G", "prefab": "prefabs/gate.prefab.json", "overrides": [ { "target": [2, 2], "name": "Left Cap" } ] }
    ] }"#;
    let (mut world, mut gfx, mut assets) = spawn_text(&dir, scene);
    // edit: move the right cap, rename the gate root, add a child under the left post
    world.get::<&mut Transform>(named(&world, "Right Cap")).unwrap().position.y = 5.0;
    world.get::<&mut Name>(named(&world, "Gate")).unwrap().0 = "Arch".into();
    let left_post = world.query::<(&Name, &Parent)>().iter().find(|(_, (n, p))| n.0 == "Post" && world.get::<&Name>(p.0).is_ok_and(|x| x.0 == "Left")).map(|(e, _)| e).unwrap();
    world.spawn((Name("Lantern".into()), Transform::from_position(goat_core::math::Vec3::new(0.0, 0.5, 0.0)), GlobalTransform::default(), Parent(left_post)));

    let reg = registry();
    let saved = save_world(&world, &assets, &reg);
    // only the instance and the added lantern are stored
    assert_eq!(saved.entities.len(), 2, "{}", serde_json::to_string_pretty(&saved).unwrap());
    let g = &saved.entities[0];
    let targets: Vec<Vec<u64>> = g.overrides.iter().map(|o| o.target.clone()).collect();
    assert_eq!(targets, vec![vec![2, 2], vec![1], vec![3, 2]]);
    assert_eq!(g.overrides[0].name.as_deref(), Some("Left Cap"));
    assert_eq!(g.overrides[1].name.as_deref(), Some("Arch"));
    assert_eq!(g.overrides[2].components["Transform"]["position"][1], 5.0);
    let lantern = &saved.entities[1];
    assert_eq!((lantern.parent, lantern.parent_target.clone()), (Some(g.id), vec![2, 1]));

    // reload what was saved: the edits come back, and saving again is stable
    let mut world2 = World::new();
    spawn_file(&mut world2, &mut gfx, &mut assets, &reg, &saved, "saved", None).unwrap();
    assert_eq!(local_y(&world2, "Right Cap"), 5.0);
    assert!(world2.get::<&Name>(named(&world2, "Arch")).is_ok());
    let lantern_parent = world2.get::<&Parent>(named(&world2, "Lantern")).unwrap().0;
    assert_eq!(world2.get::<&Name>(lantern_parent).unwrap().0, "Post");
    let saved2 = save_world(&world2, &assets, &reg);
    assert_eq!(serde_json::to_string(&saved).unwrap(), serde_json::to_string(&saved2).unwrap());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn variants_refuse_colliding_ids_and_guid_keys_load() {
    let dir = prefab_fixture("variant-guid");
    std::fs::write(dir.join("prefabs/bad.prefab.json"), r#"{ "version": 1, "base": "prefabs/post.prefab.json", "entities": [ { "id": 2, "name": "Clash" } ] }"#).unwrap();
    std::fs::write(dir.join("prefabs/post.prefab.json.meta"), r#"{ "guid": "00112233445566778899aabbccddeeff" }"#).unwrap();
    let assets = AssetServer::new(AssetIo::new(&dir));
    assert!(resolve_file(&assets, "prefabs/bad.prefab.json").is_err());
    let (world, _, _) = spawn_text(&dir, r#"{ "version": 1, "entities": [ { "id": 1, "name": "ByGuid", "prefab": "guid:00112233445566778899aabbccddeeff" } ] }"#);
    assert!(world.query::<&Name>().iter().any(|(_, n)| n.0 == "Cap"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn newer_versions_are_refused() {
    assert!(parse("x", r#"{ "version": 99, "entities": [] }"#).is_err());
}

#[test]
fn scenes_resolve_by_build_index_and_name() {
    let s: goat_runtime::ProjectSettings = serde_json::from_str(include_str!("../../../games/hello/goat.project.json")).unwrap();
    assert_eq!(s.gravity.y, -9.81);
    assert_eq!(resolve_scene(Some(&s), "1").as_deref(), Some("scenes/playground.scene.json"));
    assert_eq!(resolve_scene(Some(&s), "navigation").as_deref(), Some("scenes/navigation.scene.json"));
    assert_eq!(resolve_scene(Some(&s), "9"), None);
    assert_eq!(resolve_scene(None, "scenes/x.scene.json").as_deref(), Some("scenes/x.scene.json"));
}
