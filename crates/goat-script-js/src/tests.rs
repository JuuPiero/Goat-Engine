use super::*;
use goat_gfx::NullBackend;
use goat_runtime::{Button, Engine, Game, GlobalTransform, Name, Plugin, Transform};
use goat_script::{Script, ScriptPlugin, ScriptRef};

fn io() -> AssetIo {
    let io = AssetIo::new("/nonexistent-goat-test-root");
    io.put(
        "scripts/Spin.ts",
        br#"
import { Behaviour, Time, Vector3, Debug } from "goat";
import { double } from "./util";

export default class Spin extends Behaviour {
  speed: number = 90;
  offset: Vector3 = Vector3.zero;
  frames = 0;
  start(): void {
    this.transform.position = this.offset;
    Debug.log("spin start", this.name, this.speed);
  }
  update(): void {
    this.frames++;
    this.transform.rotate(0, double(this.speed) * Time.deltaTime, 0);
    this.transform.translate(1, 0, 0);
  }
}
"#
        .to_vec(),
    );
    io.put("scripts/util.ts", b"export function double(x: number): number { return x * 2; }\n".to_vec());
    io.put(
        "scripts/Controls.js",
        br#"
import { Behaviour, Input, GameObject } from "goat";
export class Controls extends Behaviour {
  update() {
    if (Input.getButtonDown("A")) {
      const box = GameObject.create("Box");
      box.transform.position = { x: 0, y: 5, z: 0 };
      box.setComponent("MeshRenderer", { mesh: "builtin:cube" });
      const mr = box.getComponent("MeshRenderer");
      this.gameObject.name = "Controls:" + (mr ? mr.mesh : "none");
    }
    if (Input.getButtonDown("B")) GameObject.destroy(GameObject.find("Box"));
    const n = this.gameObject.call("Test.double", 21);
    if (n !== 42) throw new Error("extension call returned " + n);
  }
}
"#
        .to_vec(),
    );
    io.put("scripts/Broken.ts", b"export default class Broken { update() { this.nope.x = 1; } }\n".to_vec());
    io.put("scripts/BadSyntax.ts", b"export default class { update( { }\n".to_vec());
    io
}

struct G;
impl Game for G {
    fn start(&mut self, ctx: &mut GameContext) {
        ctx.resources.insert(goat_assets::AssetServer::new(io()));
        goat_script::extensions(ctx).add("Test.double", |_, _, args| Ok(Value::from(args[0].as_f64().unwrap_or(0.0) * 2.0)));
        let w = &mut *ctx.world;
        w.spawn((Name("Spinner".into()), Transform::default(), GlobalTransform::default(), Script(vec![ScriptRef::new("scripts/Spin.ts").with_props(serde_json::json!({ "speed": 45, "offset": [0, 2, 0] }))])));
        w.spawn((Name("Controls".into()), Transform::default(), GlobalTransform::default(), Script::one("scripts/Controls.js")));
        w.spawn((Name("Broken".into()), Transform::default(), Script(vec![ScriptRef::new("scripts/Broken.ts"), ScriptRef::new("scripts/BadSyntax.ts")])));
    }
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(io())))]
    }
}

fn find(e: &Engine, name: &str) -> Option<Entity> {
    e.world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e)
}

#[test]
fn typescript_behaviours_run_with_the_engine_api() {
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(G);
    e.frame(0.1, &mut gfx); // start (time doesn't advance on the first frame)
    e.frame(0.1, &mut gfx); // (one frame is at most Time::max_delta = 0.1 s)
    let s = find(&e, "Spinner").unwrap();
    let t = *e.world.get::<&Transform>(s).unwrap();
    // props: speed 45 → 90°/s doubled by util.ts; offset → start position; 2 updates moved x by 2
    assert_eq!(t.position, Vec3::new(2.0, 2.0, 0.0), "start + translate");
    let (y, _, _) = t.rotation.to_euler(EulerRot::YXZ);
    assert!((y.to_degrees() - 9.0).abs() < 0.01, "rotated 90°/s × 0.1 s (first frame dt = 0): {}", y.to_degrees());

    // input + spawning + components as JSON + extension calls
    e.input.begin_frame();
    e.input.set(Button::A, true);
    e.frame(1.0 / 60.0, &mut gfx);
    let b = find(&e, "Box").expect("spawned by the script");
    assert_eq!(e.world.get::<&Transform>(b).unwrap().position.y, 5.0);
    assert!(find(&e, "Controls:builtin:cube").is_some(), "read the component back");
    e.input.begin_frame();
    e.input.set(Button::A, false);
    e.input.set(Button::B, true);
    e.frame(1.0 / 60.0, &mut gfx);
    assert!(find(&e, "Box").is_none(), "destroyed by the script");
}

#[test]
fn errors_disable_only_the_failing_script() {
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(G);
    for _ in 0..4 {
        e.frame(1.0 / 60.0, &mut gfx);
    }
    // the spinner kept running although Broken threw and BadSyntax didn't compile
    let s = find(&e, "Spinner").unwrap();
    assert_eq!(e.world.get::<&Transform>(s).unwrap().position.x, 4.0);
}

/// C#: tools/goat-cs translates tools/goat-cs/tests/Hover.cs, this host runs the module.
/// Skipped (with a note) when dotnet or the built tool isn't there.
#[test]
fn csharp_scripts_run_after_goat_cs() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/goat-cs");
    let tool = format!("{root}/bin/goat-cs.dll");
    if !std::path::Path::new(&tool).exists() {
        eprintln!("skipped: build tools/goat-cs first (dotnet build -c Release -o bin)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("goat-cs-test-{}", std::process::id()) + &module_path!().replace("::", "_"));
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::copy(format!("{root}/tests/Hover.cs"), dir.join("scripts/Hover.cs")).unwrap();
    let out = std::process::Command::new("dotnet").arg(&tool).arg(&dir).output();
    let Ok(out) = out else {
        eprintln!("skipped: no dotnet");
        return;
    };
    assert!(out.status.success(), "goat-cs failed: {}", String::from_utf8_lossy(&out.stderr));
    let js = std::fs::read_to_string(dir.join("scripts/Hover.cs.js")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    struct Cs(String);
    impl Game for Cs {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(AssetIo::new("/nonexistent")));
            goat_script::extensions(ctx).add("Test.double", |_, _, args| Ok(Value::from(args[0].as_f64().unwrap_or(0.0) * 2.0)));
            ctx.world.spawn((Name("Hoverer".into()), Transform::default(), GlobalTransform::default(), Script(vec![ScriptRef::new("scripts/Hover.cs").with_props(serde_json::json!({ "height": 2, "offset": [0, 3, 0] }))])));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            let io = AssetIo::new("/nonexistent");
            io.put("scripts/Hover.cs.js", self.0.clone().into_bytes());
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(io)))]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(Cs(js));
    e.frame(0.0, &mut gfx);
    e.frame(0.1, &mut gfx);
    let h = find(&e, "Hoverer").unwrap();
    let p = e.world.get::<&Transform>(h).unwrap().position;
    // y = offset.y + sin(t × speed) × height (t = 0.1, speed 2) ; x moved 0.5 per frame (2 updates)
    assert!((p.y - (3.0 + (0.2f32).sin() * 2.0)).abs() < 1e-4, "hover y {p}");
    assert!((p.x - 1.0).abs() < 1e-5, "hover x {p}");
    e.input.begin_frame();
    e.input.set(Button::A, true);
    e.frame(1.0 / 60.0, &mut gfx);
    assert!(find(&e, "Coin 0").is_some(), "the C# script spawned a coin");
    e.input.begin_frame();
    e.input.set(Button::A, false);
    e.input.set(Button::B, true);
    e.frame(1.0 / 60.0, &mut gfx);
    assert!(find(&e, "Coin 0").is_none(), "and destroyed it");
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_compiles_and_reports_errors() {
    let js = compile_typescript("a.ts", "export const n: number = 1 as number; interface I { x: number }\nexport type T = I;").unwrap();
    assert!(js.contains("export const n = 1"), "{js}");
    assert!(!js.contains("interface"));
    assert!(compile_typescript("b.ts", "let = ;").is_err());
}

/// Coroutines (generators + yield instructions), Awaitable and DOTween from TypeScript.
#[cfg(feature = "typescript")]
#[test]
fn coroutines_awaitables_and_tweens() {
    let io = AssetIo::new("/nonexistent-goat-fx");
    io.put(
        "scripts/Fx.ts",
        br#"
import { Behaviour, WaitForSeconds, WaitUntil, Awaitable, DOTween, Ease, LoopType, Vector3, Time } from "goat";
export default class Fx extends Behaviour {
  log: string[] = [];
  value = 0;
  start(): void {
    this.startCoroutine(this.routine());
    this.wait();
    this.transform.doMove(new Vector3(4, 0, 0), 0.4).setEase(Ease.Linear).onComplete(() => this.log.push("moved"));
    DOTween.to(() => this.value, (v: number) => (this.value = v), 10, 0.2).setEase(Ease.Linear);
    DOTween.sequence().appendInterval(0.3).appendCallback(() => this.log.push("seq"));
    const s = this.transform.doScale(2, 0.1).setLoops(2, LoopType.Yoyo);
    this.startCoroutine(function* (this: Fx) { yield s.waitForCompletion(); this.log.push("yoyo"); });
  }
  *routine(): Generator<any> {
    this.log.push("co0");
    yield new WaitForSeconds(0.2);
    this.log.push("co1");
    yield null;
    yield new WaitUntil(() => Time.time > 0.45);
    this.log.push("co2");
  }
  async wait(): Promise<void> {
    await Awaitable.waitForSecondsAsync(0.25);
    this.log.push("await");
  }
  update(): void {
    this.gameObject.name = this.log.join(",") + "|" + this.value.toFixed(1);
  }
}
"#
        .to_vec(),
    );
    struct Fx(AssetIo);
    impl Game for Fx {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(self.0.clone()));
            ctx.world.spawn((Name("fx".into()), Transform::default(), GlobalTransform::default(), Script::one("scripts/Fx.ts")));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(self.0.clone()))), Box::new(goat_tween::TweenPlugin)]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(Fx(io));
    e.frame(0.0, &mut gfx);
    for _ in 0..9 {
        e.frame(0.1, &mut gfx);
    }
    let (name, x, scale) = e
        .world
        .query::<(&Name, &Transform)>()
        .iter()
        .next()
        .map(|(_, (n, t))| (n.0.clone(), t.position.x, t.scale.x))
        .unwrap();
    let (log, value) = name.split_once('|').unwrap();
    let log: Vec<&str> = log.split(',').collect();
    assert_eq!(value, "10.0", "{name}");
    assert!((x - 4.0).abs() < 1e-4 && (scale - 1.0).abs() < 1e-4, "{x} {scale}");
    for want in ["co0", "co1", "co2", "await", "seq", "moved", "yoyo"] {
        assert!(log.contains(&want), "{want} missing: {name}");
    }
    let pos = |s: &str| log.iter().position(|x| *x == s).unwrap();
    assert!(pos("co0") < pos("co1") && pos("co1") < pos("await") && pos("await") < pos("co2"), "{name}");
}

/// C# coroutines (IEnumerator + yield return), async/await and DOTween through goat-cs.
#[test]
fn csharp_coroutines_and_tweens() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/goat-cs");
    let tool = format!("{root}/bin/goat-cs.dll");
    if !std::path::Path::new(&tool).exists() {
        eprintln!("skipped: build tools/goat-cs first");
        return;
    }
    let dir = std::env::temp_dir().join(format!("goat-cs-fx-{}", std::process::id()) + &module_path!().replace("::", "_"));
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::copy(format!("{root}/tests/Fx.cs"), dir.join("scripts/Fx.cs")).unwrap();
    let Ok(out) = std::process::Command::new("dotnet").arg(&tool).arg(&dir).output() else {
        eprintln!("skipped: no dotnet");
        return;
    };
    assert!(out.status.success(), "goat-cs failed: {}", String::from_utf8_lossy(&out.stdout));
    let js = std::fs::read_to_string(dir.join("scripts/Fx.cs.js")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let io = AssetIo::new("/nonexistent-cs-fx");
    io.put("scripts/Fx.cs.js", js.into_bytes());
    struct Cs(AssetIo);
    impl Game for Cs {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(self.0.clone()));
            ctx.world.spawn((Name("fx".into()), Transform::default(), GlobalTransform::default(), Script::one("scripts/Fx.cs")));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(self.0.clone()))), Box::new(goat_tween::TweenPlugin)]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(Cs(io));
    e.frame(0.0, &mut gfx);
    for _ in 0..6 {
        e.frame(0.1, &mut gfx);
    }
    // yoyo 2 × 0.4 s: at 0.6 s it is on its way back
    let x = e.world.query::<&Transform>().iter().next().map(|(_, t)| t.position.x).unwrap();
    assert!((x - 2.0).abs() < 1e-3, "x {x}");
    for _ in 0..4 {
        e.frame(0.1, &mut gfx);
    }
    let x = e.world.query::<&Transform>().iter().next().map(|(_, t)| t.position.x).unwrap();
    assert!(x.abs() < 1e-3, "back home: {x}");
}

/// The core API from C# (tools/goat-cs/tests/Core.cs): math operators, Random, curves, events, Invoke.
#[test]
fn csharp_core_api() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/goat-cs");
    let tool = format!("{root}/bin/goat-cs.dll");
    if !std::path::Path::new(&tool).exists() {
        eprintln!("skipped: build tools/goat-cs first");
        return;
    }
    let dir = std::env::temp_dir().join(format!("goat-cs-core-{}", std::process::id()) + &module_path!().replace("::", "_"));
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::copy(format!("{root}/tests/Core.cs"), dir.join("scripts/Core.cs")).unwrap();
    let Ok(out) = std::process::Command::new("dotnet").arg(&tool).arg(&dir).output() else {
        eprintln!("skipped: no dotnet");
        return;
    };
    assert!(out.status.success(), "goat-cs failed: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let js = std::fs::read_to_string(dir.join("scripts/Core.cs.js")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let io = AssetIo::new("/nonexistent-cs-core");
    io.put("scripts/Core.cs.js", js.into_bytes());
    struct Cs(AssetIo);
    impl Game for Cs {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(self.0.clone()));
            ctx.world.spawn((Name("core".into()), Transform::default(), GlobalTransform::default(), Script::one("scripts/Core.cs")));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(self.0.clone())))]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(Cs(io));
    e.frame(0.0, &mut gfx);
    for _ in 0..3 {
        e.frame(0.1, &mut gfx);
    }
    let name = e.world.query::<&Name>().iter().next().map(|(_, n)| n.0.clone()).unwrap();
    let parts: Vec<&str> = name.split('|').collect();
    assert_eq!(parts.len(), 14, "{name}");
    assert!(parts[0].contains('1') && parts[0].contains('2') && parts[0].contains('4'), "a + right×up = (1, 2, 4): {name}");
    assert_eq!(&parts[1..], &["-1", "1", "1", "3", "2.5", "true", "true", "4", "true", "8", "100", "false", "later"], "{name}");
}

/// Physics from TypeScript: raycast returns a GameObject; a character controller walks.
#[cfg(feature = "typescript")]
#[test]
fn physics_calls_return_game_objects() {
    let io = AssetIo::new("/nonexistent-goat-phys");
    io.put(
        "scripts/Probe.ts",
        br#"
import { Behaviour, Physics, Vector3, GameObject } from "goat";
export default class Probe extends Behaviour {
  frames = 0;
  update(): void {
    this.frames++;
    if (this.frames < 3) return;
    const hit = Physics.raycast(new Vector3(0, 10, 0), Vector3.down);
    const m = Physics.moveCharacter(this.gameObject, new Vector3(0.1, -0.5, 0));
    const near = Physics.overlapSphere(new Vector3(0, 0, 0), 0.1);
    const ground = GameObject.find("Ground");
    this.gameObject.name = `${hit ? hit.gameObject.name : "none"}|${hit ? hit.point.y.toFixed(2) : ""}|${m.isGrounded}|${near.length > 0 && near[0].equals(ground)}`;
  }
}
"#
        .to_vec(),
    );
    struct P(AssetIo);
    impl Game for P {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(self.0.clone()));
            ctx.world.spawn((Name("Ground".into()), Transform::from_position(Vec3::new(0.0, -0.5, 0.0)).with_scale(Vec3::new(20.0, 1.0, 20.0)), GlobalTransform::default(), goat_physics::Collider::unit_box()));
            ctx.world.spawn((Name("hero".into()), Transform::from_position(Vec3::new(5.0, 1.2, 0.0)), GlobalTransform::default(), goat_physics::CharacterController::default(), Script::one("scripts/Probe.ts")));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(goat_physics::PhysicsPlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(self.0.clone())))]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(P(io));
    e.frame(0.0, &mut gfx);
    for _ in 0..6 {
        e.frame(1.0 / 60.0, &mut gfx);
    }
    let name = e.world.query::<(&Name, &goat_physics::CharacterController)>().iter().next().map(|(_, (n, _))| n.0.clone()).unwrap();
    assert_eq!(name, "Ground|0.00|true|true", "{name}");
}

fn toggle_io() -> AssetIo {
    let io = AssetIo::new("/nonexistent-goat-test-root");
    io.put(
        "scripts/Toggler.ts",
        br#"
import { Behaviour, GameObject } from "goat";
export default class Toggler extends Behaviour {
  frames = 0;
  update(): void {
    this.frames++;
    const lamp = GameObject.find("Lamp");
    if (this.frames === 1) {
      lamp.tag = "Light";
      lamp.setActive(false);
      const found = GameObject.findGameObjectsWithTag("Light").length;
      this.gameObject.name = `t:${lamp.tag}:${lamp.activeSelf}:${lamp.activeInHierarchy}:${found}:${lamp.compareTag("Light")}`;
    }
    if (this.frames === 6) lamp.setActive(true);
  }
}
"#
        .to_vec(),
    );
    io.put("scripts/Counter.ts", b"import { Behaviour } from \"goat\";\nexport default class Counter extends Behaviour { n = 0; update(): void { this.n++; this.gameObject.name = 'n' + this.n; } }\n".to_vec());
    io
}

/// SetActive / tags from scripts: an inactive object's scripts (children too) don't run.
#[test]
fn set_active_and_tags_from_scripts() {
    struct T;
    impl Game for T {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(toggle_io()));
            let w = &mut *ctx.world;
            w.spawn((Name("Toggler".into()), Transform::default(), GlobalTransform::default(), Script::one("scripts/Toggler.ts")));
            let lamp = w.spawn((Name("Lamp".into()), Transform::default(), GlobalTransform::default()));
            w.spawn((Name("n0".into()), Transform::default(), GlobalTransform::default(), goat_runtime::Parent(lamp), Script::one("scripts/Counter.ts")));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(toggle_io())))]
        }
    }
    let mut e = Engine::new(T);
    let mut gfx = NullBackend::new();
    for _ in 0..3 {
        e.frame(1.0 / 60.0, &mut gfx);
    }
    // frame 1: the lamp went inactive (its child counter ran once, in the frame it was switched off at most)
    assert!(find(&e, "t:Light:false:false:0:true").is_some(), "{:?}", e.world.query::<&Name>().iter().map(|(_, n)| n.0.clone()).collect::<Vec<_>>());
    let counted = |e: &Engine| e.world.query::<&Name>().iter().find_map(|(_, n)| n.0.strip_prefix('n').and_then(|v| v.parse::<u32>().ok()));
    let before = counted(&e).unwrap_or(0);
    e.frame(1.0 / 60.0, &mut gfx);
    assert_eq!(counted(&e).unwrap_or(0), before, "the inactive child's script is paused");
    for _ in 0..3 {
        e.frame(1.0 / 60.0, &mut gfx);
    }
    assert!(counted(&e).unwrap_or(0) > before, "running again once reactivated");
}
/// SceneManager.loadScene(index | name) ? Build Settings (ProjectSettings), loaded at the end of the frame.
#[test]
fn scene_manager_loads_build_scenes() {
    struct S;
    fn sio() -> AssetIo {
        let io = AssetIo::new("/nonexistent-goat-test-root");
        io.put("scenes/menu.scene.json", br#"{ "version": 1, "entities": [ {"id":1,"name":"Menu","components":{"Script":{"src":"scripts/Next.ts"}}} ] }"#.to_vec());
        io.put("scenes/level1.scene.json", br#"{ "version": 1, "entities": [ {"id":1,"name":"Level","components":{"Script":{"src":"scripts/Report.ts"}}} ] }"#.to_vec());
        io.put("scripts/Next.ts", b"import { Behaviour, SceneManager } from 'goat';\nexport default class Next extends Behaviour { n = 0; update() { if (++this.n == 2) SceneManager.loadScene(SceneManager.sceneCountInBuildSettings - 1); } }\n".to_vec());
        io.put("scripts/Report.ts", b"import { Behaviour, SceneManager } from 'goat';\nexport default class Report extends Behaviour { start() { const s = SceneManager.getActiveScene(); this.gameObject.name = s.name + ':' + s.buildIndex; } }\n".to_vec());
        io
    }
    impl Game for S {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(sio()));
            ctx.resources.insert(goat_runtime::ProjectSettings { scenes: ["scenes/menu.scene.json", "scenes/level1.scene.json"].iter().map(|p| goat_runtime::SceneEntry { path: p.to_string(), enabled: true }).collect(), ..Default::default() });
            goat_scene::load_scene(ctx, "scenes/menu.scene.json", goat_scene::LoadMode::Single).unwrap();
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(sio())))]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(S);
    for _ in 0..4 {
        e.frame(1.0 / 60.0, &mut gfx);
    }
    assert!(find(&e, "Menu").is_none(), "the menu scene was unloaded");
    assert!(find(&e, "level1:1").is_some(), "level 1 loaded and its script saw the active scene");
}

/// Unity's callbacks: onCollisionEnter / onTriggerEnter from physics, onMouseDown / onPointerClick
/// from the pointer over a collider (PhysicsRaycaster on the camera).
#[test]
fn collision_trigger_and_pointer_messages() {
    struct M;
    fn mio() -> AssetIo {
        let io = AssetIo::new("/nonexistent-goat-test-root");
        io.put(
            "scripts/Probe.ts",
            br#"
import { Behaviour } from 'goat';
export default class Probe extends Behaviour {
  log: string[] = [];
  onCollisionEnter(c: any) { this.log.push('hit:' + c.gameObject.name); this.flush(); }
  onTriggerEnter(o: any) { this.log.push('trigger:' + o.name); this.flush(); }
  onMouseDown() { this.log.push('mouseDown'); this.flush(); }
  onPointerClick(e: any) { this.log.push('click:' + e.pointerCurrentRaycast.gameObject.name); this.flush(); }
  flush() { this.gameObject.name = this.log.join(','); }
}
"#
            .to_vec(),
        );
        io
    }
    impl Game for M {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(mio()));
            let w = &mut *ctx.world;
            // ground + a falling box with the probe script, through a trigger zone
            w.spawn((Name("Ground".into()), Transform::from_position(Vec3::new(0.0, -0.5, 0.0)).with_scale(Vec3::new(20.0, 1.0, 20.0)), GlobalTransform::default(), goat_physics::Collider::unit_box()));
            w.spawn((Name("Zone".into()), Transform::from_position(Vec3::new(0.0, 2.0, 0.0)).with_scale(Vec3::new(3.0, 1.0, 3.0)), GlobalTransform::default(), goat_physics::Collider::unit_box().trigger()));
            w.spawn((Name("Faller".into()), Transform::from_position(Vec3::new(0.0, 4.0, 0.0)), GlobalTransform::default(), goat_physics::RigidBody::dynamic(), goat_physics::Collider::unit_box(), Script::one("scripts/Probe.ts")));
            // a clickable box in front of a camera on the touch screen
            w.spawn((Name("Button3D".into()), Transform::from_position(Vec3::new(30.0, 0.0, 0.0)), GlobalTransform::default(), goat_physics::Collider::unit_box(), Script::one("scripts/Probe.ts")));
            w.spawn((
                Name("Cam".into()),
                Transform::from_position(Vec3::new(30.0, 0.0, 5.0)),
                GlobalTransform::default(),
                goat_runtime::Camera { display: goat_gfx::Display::Secondary, ..Default::default() },
                goat_physics::PhysicsRaycaster::default(),
            ));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(goat_physics::PhysicsPlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(mio())))]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(M);
    for i in 0..150 {
        e.input.begin_frame();
        // tap the middle of the touch screen (320 x 240) around frame 100
        if (100..104).contains(&i) {
            e.input.pointer = Some(goat_core::math::Vec2::new(160.0, 120.0));
            e.input.pointer_down = i < 103;
        } else {
            e.input.pointer = None;
            e.input.pointer_down = false;
        }
        e.frame(1.0 / 60.0, &mut gfx);
    }
    let names: Vec<String> = e.world.query::<&Name>().iter().map(|(_, n)| n.0.clone()).collect();
    assert!(names.iter().any(|n| n.contains("trigger:Zone") && n.contains("hit:Ground")), "{names:?}");
    assert!(names.iter().any(|n| n.contains("mouseDown") && n.contains("click:")), "{names:?}");
}

// ------------------------------------------------------------------ ScriptableObject

fn so_io() -> AssetIo {
    let io = AssetIo::new("/nonexistent-goat-test-root-so");
    io.put(
        "scripts/EnemyData.ts",
        br#"
import { ScriptableObject, Vector3 } from "goat";
export default class EnemyData extends ScriptableObject {
  hp = 10;
  speed = 2.5;
  tint = new Vector3(1, 1, 1);
  next = null;
  tags = ["a"];
  _cache = 0;
  enabledCount = 0;
  onEnable() { this.enabledCount++; }
}
"#
        .to_vec(),
    );
    io.put("data/Goblin.asset.json", br#"{ "version": 1, "script": "scripts/EnemyData.ts", "data": { "hp": 30, "tint": [1, 0, 0], "next": "data/Boss.asset.json" } }"#.to_vec());
    io.put("data/Boss.asset.json", b"\xEF\xBB\xBF{ \"version\": 1, \"script\": \"scripts/EnemyData.ts\", \"data\": { \"hp\": 500, \"next\": \"data/Goblin.asset.json\" } }".to_vec());
    io.put(
        "scripts/Enemy.ts",
        br#"
import { Behaviour, ScriptableObject, Resources, Vector3 } from "goat";
import EnemyData from "./EnemyData";
export default class Enemy extends Behaviour {
  data = null;
  start() {
    const d = this.data;
    const check = (ok, what) => { if (!ok) throw new Error("failed: " + what); };
    check(d instanceof EnemyData, "the field holds the asset");
    check(d.hp === 30 && d.speed === 2.5, "data over defaults");
    check(d.tint instanceof Vector3 && d.tint.x === 1 && d.tint.y === 0, "vectors");
    check(d.next.hp === 500 && d.next.next === d, "references between assets (cycle) share instances");
    check(d.name === "Goblin" && d.assetPath === "data/Goblin.asset.json", "name / assetPath");
    check(ScriptableObject.load("data/Goblin.asset.json") === d && Resources.load("data/Goblin.asset.json") === d, "load returns the shared instance");
    check(d.enabledCount === 1, "onEnable once");
    const fresh = ScriptableObject.createInstance(EnemyData);
    check(fresh.hp === 10 && fresh.enabledCount === 1 && fresh.assetPath === null, "createInstance");
    const copy = ScriptableObject.instantiate(d);
    check(copy !== d && copy.hp === 30 && copy.name === "Goblin (Clone)", "instantiate");
    this.gameObject.name = "ok:" + d.hp;
  }
}
"#
        .to_vec(),
    );
    io
}

#[derive(Default)]
struct SoProbe {
    defaults: Option<Result<Vec<(String, Value)>, String>>,
    is_so: Option<Result<bool, String>>,
    behaviour_is_so: Option<Result<bool, String>>,
}

struct SoGame;
impl Game for SoGame {
    fn start(&mut self, ctx: &mut GameContext) {
        ctx.resources.insert(goat_assets::AssetServer::new(so_io()));
        let mut host = JsHost::with_io(so_io());
        let probe = {
            let mut api = api_of(ctx);
            SoProbe {
                defaults: Some(host.defaults(&mut api, "scripts/EnemyData.ts")),
                is_so: Some(host.is_scriptable_object(&mut api, "scripts/EnemyData.ts")),
                behaviour_is_so: Some(host.is_scriptable_object(&mut api, "scripts/Enemy.ts")),
            }
        };
        ctx.resources.insert(probe);
        ctx.world.spawn((Name("Enemy".into()), Transform::default(), GlobalTransform::default(), Script(vec![ScriptRef::new("scripts/Enemy.ts").with_props(serde_json::json!({ "data": "data/Goblin.asset.json" }))])));
    }
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(so_io())))]
    }
}

#[test]
fn scriptable_objects_load_as_shared_assets() {
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(SoGame);
    e.frame(0.1, &mut gfx);
    e.frame(0.1, &mut gfx);
    assert!(find(&e, "ok:30").is_some(), "the Enemy script checked its asset: {:?}", e.world.query::<&Name>().iter().map(|(_, n)| n.0.clone()).collect::<Vec<_>>());
    let p = e.resources.get::<SoProbe>().unwrap();
    let d = p.defaults.clone().unwrap().unwrap();
    let names: Vec<&str> = d.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, ["hp", "speed", "tint", "next", "tags", "enabledCount"], "declaration order, no _private");
    let map: serde_json::Map<String, Value> = d.into_iter().collect();
    assert_eq!(Value::Object(map), serde_json::json!({ "hp": 10, "speed": 2.5, "tint": [1, 1, 1], "next": null, "tags": ["a"], "enabledCount": 0 }));
    assert_eq!(p.is_so.clone().unwrap(), Ok(true));
    assert_eq!(p.behaviour_is_so.clone().unwrap(), Ok(false));
}

// ------------------------------------------------------------------ core API (math, transforms, camera, materials)

fn core_io() -> AssetIo {
    let io = AssetIo::new("/nonexistent-goat-test-root-core");
    io.put(
        "scripts/CoreCheck.ts",
        br##"
import { Behaviour, Vector3, Vector2, Quaternion, Color, Mathf, Random, Plane, Ray, Matrix4x4, AnimationCurve, Keyframe, Gradient, Bounds, Rect, Camera, Screen, GameObject, Application, GoatEvent, Space } from "goat";
export default class CoreCheck extends Behaviour {
  start() {
    const fails = [];
    const near = (a, b, what, eps = 1e-3) => { if (Math.abs(a - b) > eps) fails.push(`${what}: ${a} != ${b}`); };
    const nearV = (a, b, what, eps = 1e-3) => { near(a.x, b.x, what + ".x", eps); near(a.y, b.y, what + ".y", eps); near(a.z, b.z, what + ".z", eps); };
    // quaternions agree with the engine's euler conversion and rotate like it
    const q = Quaternion.euler(0, 90, 0);
    nearV(q.mul(Vector3.right), new Vector3(0, 0, -1), "90 deg about Y turns +X to -Z");
    nearV(Quaternion.angleAxis(90, Vector3.up).mul(Vector3.right), new Vector3(0, 0, -1), "angleAxis");
    nearV(Quaternion.lookRotation(new Vector3(1, 0, 0)).mul(Vector3.forward), new Vector3(1, 0, 0), "lookRotation forward");
    near(Quaternion.angle(Quaternion.identity, q), 90, "angle");
    nearV(Quaternion.slerp(Quaternion.identity, q, 0.5).eulerAngles, new Vector3(0, 45, 0), "slerp");
    nearV(Quaternion.inverse(q).mul(q).mul(Vector3.up), Vector3.up, "inverse");
    nearV(Quaternion.fromToRotation(Vector3.up, Vector3.right).mul(Vector3.up), Vector3.right, "fromTo");
    // vectors
    near(Vector3.angle(Vector3.up, Vector3.right), 90, "Vector3.angle");
    nearV(Vector3.project(new Vector3(1, 1, 0), Vector3.right), Vector3.right, "project");
    nearV(Vector3.reflect(new Vector3(1, -1, 0), Vector3.up), new Vector3(1, 1, 0), "reflect");
    near(Vector2.signedAngle(Vector2.right, Vector2.up), 90, "signedAngle");
    // transforms: parent at (1, 0, 0) turned 90 deg about Y, child at local (1, 0, 0)
    const parent = GameObject.find("Parent").transform;
    const child = GameObject.find("Child").transform;
    nearV(child.position, new Vector3(1, 0, -1), "child world position");
    nearV(child.localPosition, new Vector3(1, 0, 0), "child local position");
    nearV(child.forward, new Vector3(-1, 0, 0), "child forward (parent turned)");
    nearV(parent.transformPoint(new Vector3(0, 0, -2)), new Vector3(-1, 0, 0), "transformPoint");
    nearV(parent.inverseTransformPoint(new Vector3(1, 0, -1)), new Vector3(1, 0, 0), "inverseTransformPoint");
    child.position = new Vector3(1, 2, 0);
    nearV(child.localPosition, new Vector3(0, 2, 0), "world position set through the parent");
    if (parent.childCount !== 1 || parent.getChild(0).name !== "Child" || parent.find("Child") === null || !child.isChildOf(parent)) fails.push("hierarchy");
    child.rotation = Quaternion.identity;
    nearV(child.localEulerAngles, new Vector3(0, -90, 0), "world rotation set through the parent");
    // camera
    const cam = Camera.main;
    if (!cam || cam.gameObject.name !== "Main Camera") fails.push("Camera.main");
    else {
      const sp = cam.worldToScreenPoint(new Vector3(0, 0, -5));
      near(sp.x, Screen.width / 2, "screen x"); near(sp.y, Screen.height / 2, "screen y"); near(sp.z, 5, "depth");
      const ray = cam.screenPointToRay(new Vector2(Screen.width / 2, Screen.height / 2));
      nearV(ray.direction, new Vector3(0, 0, -1), "center ray");
      nearV(cam.screenToWorldPoint(new Vector3(Screen.width / 2, Screen.height / 2, 5)), new Vector3(0, 0, -5), "screenToWorldPoint");
    }
    // random, color, math helpers
    Random.initState(42); const a = [Random.value, Random.range(0, 10), Random.range(1.5, 2.5)];
    Random.initState(42); const b = [Random.value, Random.range(0, 10), Random.range(1.5, 2.5)];
    if (JSON.stringify(a) !== JSON.stringify(b) || !Number.isInteger(a[1]) || a[2] < 1.5 || a[2] > 2.5) fails.push("Random");
    if (Color.fromHex("#ff8000").toHex() !== "#ff8000") fails.push("Color hex");
    const hsv = Color.RGBToHSV(Color.HSVToRGB(0.3, 0.5, 0.8)); near(hsv.h, 0.3, "hsv h"); near(hsv.s, 0.5, "hsv s");
    near(Mathf.deltaAngle(350, 10), 20, "deltaAngle");
    near(Mathf.inverseLerp(10, 20, 15), 0.5, "inverseLerp");
    const n = Mathf.perlinNoise(1.3, 2.7); if (!(n >= 0 && n <= 1)) fails.push("perlin");
    const vel = { value: 0 }; let x = 0; for (let i = 0; i < 200; i++) x = Mathf.smoothDamp(x, 10, vel, 0.3, Infinity, 1 / 60); near(x, 10, "smoothDamp", 0.05);
    near(new Plane(Vector3.up, 0).raycast(new Ray(new Vector3(0, 5, 0), Vector3.down)), 5, "plane raycast");
    const m = Matrix4x4.TRS(new Vector3(1, 2, 3), q, new Vector3(2, 2, 2));
    nearV(m.inverse.multiplyPoint(m.multiplyPoint(new Vector3(0.3, -1, 4))), new Vector3(0.3, -1, 4), "matrix inverse");
    near(AnimationCurve.linear(0, 0, 2, 10).evaluate(1), 5, "curve");
    near(new Gradient([{ color: Color.black, time: 0 }, { color: Color.white, time: 1 }]).evaluate(0.25).r, 0.25, "gradient");
    const bounds = new Bounds(Vector3.zero, new Vector3(2, 2, 2)); bounds.encapsulate(new Vector3(3, 0, 0)); near(bounds.max.x, 3, "bounds");
    if (!new Rect(0, 0, 2, 2).contains(new Vector2(1, 1))) fails.push("rect");
    let fired = 0; const ev = new GoatEvent(); const fn = () => fired++; ev.addListener(fn); ev.invoke(); ev.removeListener(fn); ev.invoke(); if (fired !== 1) fails.push("event");
    if (!Application.platform) fails.push("platform");
    // materials: the object gets its own copy, the shared material stays
    const box = GameObject.find("Box");
    box.renderer.material.color = new Color(1, 0, 0, 1);
    const c = box.renderer.material.color; near(c.r, 1, "material color r"); near(c.g, 0, "material color g");
    this.gameObject.name = fails.length ? "FAIL " + fails.join("; ") : "core ok";
  }
}
"##
        .to_vec(),
    );
    io
}

struct CoreGame;
impl Game for CoreGame {
    fn start(&mut self, ctx: &mut GameContext) {
        ctx.resources.insert(goat_assets::AssetServer::new(core_io()));
        let parent = ctx.world.spawn((Name("Parent".into()), Transform::from_position(Vec3::X).with_rotation(goat_core::math::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)), GlobalTransform::default()));
        ctx.world.spawn((Name("Child".into()), Transform::from_position(Vec3::X), GlobalTransform::default(), goat_runtime::Parent(parent)));
        ctx.world.spawn((Name("Main Camera".into()), Transform::default(), GlobalTransform::default(), goat_runtime::Camera::default()));
        let mesh = ctx.gfx.create_mesh(&goat_gfx::MeshData::cube(1.0)).unwrap();
        let mat = ctx.gfx.create_material(&goat_gfx::MaterialDesc::default()).unwrap();
        ctx.world.spawn((Name("Box".into()), Transform::default(), GlobalTransform::default(), goat_runtime::MeshRenderer::new(mesh, mat)));
        ctx.world.spawn((Name("Checker".into()), Transform::default(), GlobalTransform::default(), Script::one("scripts/CoreCheck.ts")));
    }
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(core_io())))]
    }
}

#[test]
fn core_api_math_transforms_camera_materials() {
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(CoreGame);
    e.frame(0.1, &mut gfx);
    e.frame(0.1, &mut gfx);
    let names: Vec<String> = e.world.query::<&Name>().iter().map(|(_, n)| n.0.clone()).collect();
    assert!(names.iter().any(|n| n == "core ok"), "{names:?}");
    // the box's material was copied, the original kept white
    let box_e = find(&e, "Box").unwrap();
    let id = e.world.get::<&goat_runtime::MeshRenderer>(box_e).unwrap().material;
    let d = goat_gfx::GraphicsBackend::material_desc(&gfx, id).unwrap();
    assert_eq!(d.base_color.to_array(), [1.0, 0.0, 0.0, 1.0]);
}
/// Play-mode tools: hot reload keeps state and swaps the code; the console sees `$sel` /
/// `$scripts`; `Debug.break()` asks the editor to pause.
#[test]
fn hot_reload_console_and_break() {
    let io = AssetIo::new("/nonexistent-goat-reload");
    io.put(
        "scripts/Counter.js",
        br#"
import { Behaviour, Debug } from "goat";
export default class Counter extends Behaviour {
  count = 0;
  update() { this.count += 1; this.gameObject.name = "c" + this.count; if (this.count === 3) Debug.break("three"); }
}
"#
        .to_vec(),
    );
    struct G(AssetIo);
    impl Game for G {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(self.0.clone()));
            ctx.world.spawn((Name("c".into()), Transform::default(), GlobalTransform::default(), Script::one("scripts/Counter.js")));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(self.0.clone())))]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(G(io.clone()));
    e.frame(0.0, &mut gfx);
    for _ in 0..2 {
        e.frame(0.1, &mut gfx);
    }
    let name = |e: &Engine| e.world.query::<&Name>().iter().next().map(|(_, n)| n.0.clone()).unwrap();
    assert_eq!(name(&e), "c3");
    let brk = e.resources.get_mut::<goat_script::ScriptDebug>().and_then(|d| d.break_requested.take());
    assert_eq!(brk.as_deref(), Some("three"));
    // new code: counts by ten, the running object keeps its count
    io.put(
        "scripts/Counter.js",
        br#"
import { Behaviour } from "goat";
export default class Counter extends Behaviour {
  count = 0;
  update() { this.count += 10; this.gameObject.name = "c" + this.count; }
}
"#
        .to_vec(),
    );
    e.resources.get_or_default::<goat_script::ScriptDebug>().reloads.push("scripts/Counter.js".into());
    e.frame(0.1, &mut gfx);
    assert_eq!(name(&e), "c13", "state kept, new code");
    // the console
    let ent = e.world.query::<&Name>().iter().next().map(|(en, _)| en).unwrap();
    e.resources.get_or_default::<goat_script::ScriptDebug>().evals.push((7, "$sel.name + ' ' + $scripts[0].count + ' ' + (1 + 2)".into(), Some(ent)));
    e.frame(0.1, &mut gfx);
    let results = std::mem::take(&mut e.resources.get_or_default::<goat_script::ScriptDebug>().results);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, 7);
    assert_eq!(results[0].1.as_ref().unwrap(), &Value::String("\"c13 13 3\"".into()));
}
/// Speed of the host: 100 behaviours doing vector math + engine calls for 200 frames
/// (`cargo test -p goat-script-js --release --features quickjs bench_ -- --ignored --nocapture`).
#[test]
#[ignore]
fn bench_hundred_behaviours() {
    let io = AssetIo::new("/nonexistent-goat-bench");
    io.put(
        "scripts/Busy.js",
        br#"
import { Behaviour, Vector3, Mathf, Time } from "goat";
export default class Busy extends Behaviour {
  t = 0;
  update() {
    this.t += Time.deltaTime;
    let acc = new Vector3(0, 0, 0);
    for (let i = 0; i < 50; i++) { acc = acc.add(new Vector3(Mathf.sin(this.t + i), Mathf.cos(i), 0.5).normalized); }
    this.transform.localPosition = acc.mul(0.01);
  }
}
"#
        .to_vec(),
    );
    struct G(AssetIo);
    impl Game for G {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(self.0.clone()));
            for i in 0..100 {
                ctx.world.spawn((Name(format!("b{i}")), Transform::default(), GlobalTransform::default(), Script::one("scripts/Busy.js")));
            }
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(self.0.clone())))]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(G(io));
    e.frame(0.0, &mut gfx);
    let start = std::time::Instant::now();
    for _ in 0..200 {
        e.frame(1.0 / 60.0, &mut gfx);
    }
    println!("{}: {:.2} ms per frame (100 behaviours)", module_path!(), start.elapsed().as_secs_f64() * 1000.0 / 200.0);
}

/// The low-level graphics API (Graphics / GL / Debug.drawLine / RenderTexture / Texture2D /
/// MaterialPropertyBlock) as views the backend receives.
#[test]
fn low_level_graphics_from_scripts() {
    fn gfx_io() -> AssetIo {
            let io = io();
            io.put(
                "scripts/Gfx.ts",
                br#"
import { Behaviour, Graphics, GL, Debug, Color, Vector3, Quaternion, Matrix4x4, MaterialPropertyBlock, RenderTexture, Texture2D, Camera, GameObject } from "goat";
export default class Gfx extends Behaviour {
  rt: any = null;
  start() {
    this.rt = new RenderTexture(64, 32, 16);
    Camera.of(GameObject.find("MiniCam")).targetTexture = this.rt;
    const t = new Texture2D(4, 2);
    t.setPixel(0, 0, Color.red);
    t.apply();
    const back = t.getPixel(0, 0);
    if (back.r !== 1 || back.g !== 0) throw new Error("getPixel " + back);
    const png = t.encodeToPNG();
    if (png[1] !== 0x50 || png[2] !== 0x4e) throw new Error("png header");
    const again = new Texture2D(1, 1);
    if (!again.loadImage(png) || again.width !== 4 || again.height !== 2) throw new Error("loadImage " + again.width);
    if (again.getPixel(0, 0).r !== 1 || again.getPixel(0, 1).r !== 1 || again.getPixel(0, 1).g !== 1) throw new Error("bottom-up rows");
    RenderTexture.active = this.rt;
    const r = new Texture2D(64, 32);
    r.readPixels({ x: 0, y: 0, width: 64, height: 32 }, 0, 0);
    if (this.rt.width !== 64 || this.rt.height !== 32) throw new Error("rt size");
    this.gameObject.name = "ok";
  }
  update() {
    Graphics.drawMesh("Cube", new Vector3(0, 0, -5), Quaternion.identity, "builtin:default");
    const block = new MaterialPropertyBlock();
    block.setVectorArray("_Color", [1, 0, 0, 1, 0, 1, 0, 1]);
    Graphics.drawMeshInstanced("Sphere", 0, "builtin:default", [Matrix4x4.TRS(new Vector3(1, 0, -5), Quaternion.identity, Vector3.one), Matrix4x4.TRS(new Vector3(-1, 0, -5), Quaternion.identity, Vector3.one)], 2, block);
    Debug.drawLine(new Vector3(0, 0, -3), new Vector3(1, 1, -3), Color.yellow);
    GL.pushMatrix(); GL.loadOrtho();
    GL.begin(GL.LINES); GL.color(Color.green); GL.vertex3(0, 0, 0); GL.vertex3(1, 1, 0); GL.end();
    GL.popMatrix();
  }
}
"#
                .to_vec(),
            );
            io
    }
    struct Gfx;
    impl Game for Gfx {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(gfx_io()));
            let w = &mut *ctx.world;
            w.spawn((Name("Cam".into()), Transform::default(), GlobalTransform::default(), goat_runtime::Camera::default()));
            w.spawn((Name("MiniCam".into()), Transform::default(), GlobalTransform::default(), goat_runtime::Camera::default()));
            w.spawn((Name("Gfx".into()), Transform::default(), GlobalTransform::default(), Script::one("scripts/Gfx.ts")));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(gfx_io())))]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(Gfx);
    for _ in 0..3 {
        e.frame(1.0 / 60.0, &mut gfx);
    }
    assert!(find(&e, "ok").is_some(), "the script's checks passed");
    let views = &gfx.last_views;
    let target = views.iter().find(|v| v.target.is_some()).expect("MiniCam draws into the render texture");
    assert_eq!(goat_gfx::GraphicsBackend::texture_size(&gfx, target.target.unwrap()), Some((64, 32)));
    assert_eq!(views.iter().position(|v| v.target.is_some()), Some(0), "render-texture cameras first");
    let screen = views.iter().find(|v| v.target.is_none() && v.depth == 0).unwrap();
    let tinted: Vec<_> = screen.items.iter().filter(|i| i.color != goat_core::Color::WHITE).map(|i| i.color).collect();
    assert_eq!(tinted.len(), 2, "two instance colors: {tinted:?}");
    assert!(screen.items.len() >= 4, "cube + 2 spheres + the line mesh: {}", screen.items.len());
    assert!(views.iter().any(|v| v.depth == i32::MAX), "GL in screen space: an overlay view");
}

/// Several scenes: Additive loads, loadSceneAsync (await), DontDestroyOnLoad surviving a Single
/// load, unloadSceneAsync, gameObject.scene, sceneLoaded events, the active scene.
#[test]
fn additive_scenes_and_dont_destroy_on_load() {
    struct S;
    fn sio() -> AssetIo {
        let io = AssetIo::new("/nonexistent-goat-test-root");
        io.put("scenes/boot.scene.json", br#"{ "version": 1, "entities": [ {"id":1,"name":"Boot","components":{"Script":{"src":"scripts/Boot.ts"}}}, {"id":2,"name":"Music","components":{}} ] }"#.to_vec());
        io.put("scenes/hud.scene.json", br#"{ "version": 1, "entities": [ {"id":1,"name":"HUD","components":{}} ] }"#.to_vec());
        io.put("scenes/level.scene.json", br#"{ "version": 1, "entities": [ {"id":1,"name":"Level","components":{}} ] }"#.to_vec());
        io.put(
            "scripts/Boot.ts",
            br#"
import { Behaviour, SceneManager, LoadSceneMode, GameObject } from 'goat';
export default class Boot extends Behaviour {
  log: string[] = [];
  async start() {
    this.dontDestroyOnLoad();
    this.dontDestroyOnLoad(GameObject.find('Music'));
    SceneManager.sceneLoaded.addListener((s: any) => this.log.push('loaded:' + s.name));
    await SceneManager.loadSceneAsync('hud', LoadSceneMode.Additive);
    this.log.push('count:' + SceneManager.sceneCount);
    this.log.push('hudScene:' + GameObject.find('HUD').scene.name);
    this.log.push('active:' + SceneManager.getActiveScene().name);
    SceneManager.loadScene('level');
    await SceneManager.loadSceneAsync('hud', LoadSceneMode.Additive);
    this.log.push('after:' + SceneManager.sceneCount + ':' + SceneManager.getActiveScene().name);
    this.log.push('musicScene:' + GameObject.find('Music').scene.name);
    SceneManager.setActiveScene(SceneManager.getSceneByName('hud'));
    this.log.push('active2:' + SceneManager.getActiveScene().name);
    await SceneManager.unloadSceneAsync('hud');
    this.log.push('final:' + SceneManager.sceneCount + ':' + SceneManager.getActiveScene().name + ':' + (GameObject.find('HUD') ? 'hud' : 'nohud'));
    this.gameObject.name = this.log.join(',');
  }
}
"#
            .to_vec(),
        );
        io
    }
    impl Game for S {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(sio()));
            goat_scene::load_scene(ctx, "scenes/boot.scene.json", goat_scene::LoadMode::Single).unwrap();
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(sio())))]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(S);
    for _ in 0..12 {
        e.frame(1.0 / 60.0, &mut gfx);
    }
    let names: Vec<String> = e.world.query::<&Name>().iter().map(|(_, n)| n.0.clone()).collect();
    let boot = names.iter().find(|n| n.contains("final:")).unwrap_or_else(|| panic!("the script finished: {names:?}"));
    assert_eq!(
        boot,
        "loaded:hud,count:2,hudScene:hud,active:boot,loaded:level,loaded:hud,after:2:level,musicScene:DontDestroyOnLoad,active2:hud,final:1:level:nohud"
    );
    assert!(names.iter().any(|n| n == "Music") && names.iter().any(|n| n == "Level"), "{names:?}");
}

/// TextAsset / JsonAsset / BufferAsset fields (decorators), Resources.load / loadAll / find /
/// loadPrefab, @nonSerialized / @hideInInspector and the field metadata the editor reads.
#[test]
fn data_assets_resources_and_field_decorators() {
    fn dio() -> AssetIo {
        let io = io();
        io.put("data/credits.txt", "\u{feff}Goat\nEngine".as_bytes().to_vec());
        io.put("data/level.json", br#"{ "name": "One", "buses": 3 }"#.to_vec());
        io.put("data/map.bytes", vec![1, 2, 3, 250]);
        io.put("Resources/Enemies/Goblin.json", br#"{ "hp": 7 }"#.to_vec());
        io.put("Resources/Enemies/Orc.json", br#"{ "hp": 12 }"#.to_vec());
        io.put("game/Resources/Sounds/hit.txt", b"boom".to_vec());
        io.put("Resources/Props/Crate.prefab.json", br#"{ "version": 1, "entities": [] }"#.to_vec());
        io.put(
            "scripts/Data.ts",
            br#"
import { Behaviour, TextAsset, JsonAsset, BufferAsset, Resources, field, nonSerialized, hideInInspector, range, button, header } from "goat";
export default class Data extends Behaviour {
  @field(TextAsset) credits: TextAsset | null = null;
  @field([JsonAsset]) levels: JsonAsset[] = [];
  @field(BufferAsset) map: BufferAsset | null = null;
  @nonSerialized cache = 5;
  @hideInInspector secret = 1;
  @header("Tuning") @range(0, 10) speed = 2;
  @button("Reset Speed") resetSpeed() { this.speed = 0; }
  start() {
    const out: any[] = [];
    out.push(this.credits!.lines.length, this.credits!.text.startsWith("Goat"));
    out.push(this.levels[0].json.buses, this.levels[0].name);
    out.push(this.map!.length, this.map!.bytes[3]);
    const g = Resources.load("Enemies/Goblin", JsonAsset) as JsonAsset;
    out.push(g.json.hp);
    out.push(Resources.loadAll("Enemies").length, Resources.loadAll("").length);
    out.push(Resources.find("Sounds/hit"), (Resources.load("Sounds/hit") as TextAsset).text);
    out.push(Resources.loadPrefab("Props/Crate"), Resources.load("Nope"));
    out.push(this.cache);
    this.gameObject.name = JSON.stringify(out);
  }
}
"#
            .to_vec(),
        );
        io
    }
    struct D;
    impl Game for D {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(dio()));
            let props = serde_json::json!({ "credits": "data/credits.txt", "levels": ["data/level.json"], "map": "data/map.bytes", "cache": 99 });
            ctx.world.spawn((Name("Data".into()), Transform::default(), GlobalTransform::default(), Script(vec![ScriptRef::new("scripts/Data.ts").with_props(props)])));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(dio())))]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(D);
    for _ in 0..3 {
        e.frame(1.0 / 60.0, &mut gfx);
    }
    let names: Vec<String> = e.world.query::<&Name>().iter().map(|(_, n)| n.0.clone()).collect();
    assert!(
        names.iter().any(|n| n == r#"[2,true,3,"level",4,250,7,2,4,"game/Resources/Sounds/hit.txt","boom","Resources/Props/Crate.prefab.json",null,5]"#),
        "{names:?}"
    );
    // what the editor reads: fields without the non-serialized one, and the metadata
    let mut host = JsHost::with_io(dio());
    let mut gfx = NullBackend::new();
    let mut eng = Engine::new(D);
    eng.frame(0.0, &mut gfx);
    eng.with_context(&mut gfx, |ctx| {
        let mut api = crate::api_of(ctx);
        let fields: Vec<String> = host.defaults(&mut api, "scripts/Data.ts").unwrap().into_iter().map(|(k, _)| k).collect();
        assert_eq!(fields, ["credits", "levels", "map", "secret", "speed"]);
        let meta = host.field_meta(&mut api, "scripts/Data.ts").unwrap();
        assert_eq!(meta["fields"]["credits"]["type"], "textAsset");
        assert_eq!(meta["fields"]["levels"]["array"], true);
        assert_eq!(meta["fields"]["secret"]["hide"], true);
        assert_eq!(meta["fields"]["speed"]["range"], serde_json::json!([0, 10]));
        assert_eq!(meta["fields"]["speed"]["header"], "Tuning");
        assert_eq!(meta["buttons"][0]["label"], "Reset Speed");
    });
}

/// Unity's Input: KeyCode keys, legacy names, mouse buttons, wheel, axes, virtual buttons, touches.
#[test]
fn unity_input_keys_mouse_axes() {
    fn kio() -> AssetIo {
        let io = io();
        io.put(
            "scripts/Keys.ts",
            br#"
import { Behaviour, Input, KeyCode } from "goat";
export default class Keys extends Behaviour {
  log: string[] = [];
  update() {
    const l: any[] = [];
    l.push(Input.getKey(KeyCode.Space), Input.getKeyDown("space"), Input.getKeyUp(KeyCode.A), Input.getKey("a"));
    l.push(Input.getMouseButton(0), Input.getMouseButtonDown(1), Input.mouseScrollDelta.y);
    l.push(Input.getAxisRaw("Horizontal"), Input.getButton("Jump"), Input.getButtonDown("Fire1"), Input.anyKeyDown);
    l.push(Input.mousePosition.x, Input.touchCount, Input.touchCount ? Input.getTouch(0).phase : "-");
    this.log.push(JSON.stringify(l));
    this.gameObject.name = this.log.join("|");
  }
}
"#
            .to_vec(),
        );
        io
    }
    struct K;
    impl Game for K {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(kio()));
            ctx.world.spawn((Name("Keys".into()), Transform::default(), GlobalTransform::default(), Script::one("scripts/Keys.ts")));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(kio())))]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(K);
    e.frame(1.0 / 60.0, &mut gfx); // start
    // frame 1: Space + A down, left mouse down at (40, 20), wheel up, d-pad right
    e.input.begin_frame();
    e.input.keys.insert("Space".into());
    e.input.keys.insert("KeyA".into());
    e.input.mouse_buttons = [true, false, false];
    e.input.pointer = Some(goat_core::math::Vec2::new(40.0, 20.0));
    e.input.pointer_down = true;
    e.input.scroll = goat_core::math::Vec2::new(0.0, 1.0);
    e.input.set(Button::Right, true);
    e.frame(1.0 / 60.0, &mut gfx);
    // frame 2: A released, right mouse pressed, left still down
    e.input.begin_frame();
    e.input.keys.remove("KeyA");
    e.input.mouse_buttons = [true, true, false];
    e.input.pointer = Some(goat_core::math::Vec2::new(44.0, 20.0));
    e.input.pointer_down = true;
    e.frame(1.0 / 60.0, &mut gfx);
    let name = e.world.query::<&Name>().iter().map(|(_, n)| n.0.clone()).find(|n| n.contains('[')).unwrap();
    let frames: Vec<&str> = name.split('|').collect();
    assert_eq!(frames[frames.len() - 2], r#"[true,true,false,true,true,false,1,1,true,true,true,40,1,"Began"]"#);
    assert_eq!(frames[frames.len() - 1], r#"[true,false,true,false,true,true,0,1,true,false,true,44,1,"Moved"]"#);
}

/// Unity's lifecycle (Awake → OnEnable → Start → Update, enabled off = OnDisable and no Update,
/// on again = OnEnable), onRenderImage on a camera, Graphics.drawTexture, ProfilerMarker.
#[test]
fn lifecycle_enabled_and_image_effects() {
    fn lio() -> AssetIo {
        let io = io();
        io.put(
            "scripts/Life.ts",
            br#"
import { Behaviour } from "goat";
export default class Life extends Behaviour {
  log: string[] = [];
  awake() { this.log.push("awake"); }
  onEnable() { this.log.push("enable"); }
  start() { this.log.push("start"); }
  update() { this.log.push("update"); this.gameObject.name = "LIFE " + this.log.join(","); }
  onDisable() { this.log.push("disable"); this.gameObject.name = "LIFE " + this.log.join(","); }
}
"#
            .to_vec(),
        );
        io.put(
            "scripts/Switch.ts",
            br#"
import { Behaviour, GameObject, Graphics, Rect, Color, Texture2D, ProfilerMarker, Sprite } from "goat";
import Life from "./Life";
export default class Switch extends Behaviour {
  n = 0;
  tex: any = null;
  marker = new ProfilerMarker("Switch.Work");
  update() {
    const life = GameObject.findObjectOfType(Life)!;
    this.n++;
    if (this.n === 2) life.enabled = false;
    if (this.n === 4) life.enabled = true;
    if (!this.tex) { this.tex = new Texture2D(4, 4); this.tex.apply(); }
    Graphics.drawTexture(new Rect(10, 10, 32, 32), this.tex, Color.red);
    this.marker.auto(() => { for (let i = 0; i < 1000; i++); });
    const s = Sprite.create(this.tex);
    if (!s._key) throw new Error("sprite key");
  }
}
"#
            .to_vec(),
        );
        io.put(
            "scripts/Effect.ts",
            br#"
import { Behaviour, Graphics } from "goat";
export default class Effect extends Behaviour {
  calls = 0;
  onRenderImage(src: any, dst: any) { this.calls++; Graphics.blit(src, dst); this.gameObject.name = "EFFECT " + this.calls + " " + src.width; }
}
"#
            .to_vec(),
        );
        io
    }
    struct L;
    impl Game for L {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(lio()));
            let w = &mut *ctx.world;
            w.spawn((Name("Life".into()), Transform::default(), GlobalTransform::default(), Script::one("scripts/Life.ts")));
            w.spawn((Name("Switch".into()), Transform::default(), GlobalTransform::default(), Script::one("scripts/Switch.ts")));
            w.spawn((Name("Cam".into()), Transform::default(), GlobalTransform::default(), goat_runtime::Camera::default(), Script::one("scripts/Effect.ts")));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(lio())))]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(L);
    for _ in 0..7 {
        e.frame(1.0 / 60.0, &mut gfx);
    }
    let names: Vec<String> = e.world.query::<&Name>().iter().map(|(_, n)| n.0.clone()).collect();
    let life = names.iter().find(|n| n.starts_with("LIFE ")).unwrap();
    // frames: 1 update, then Switch (after Life in the list) disables it during frame 2 → frames 3
    // skip, enabled again in frame 4 → OnEnable before frame 5's update
    assert!(life.starts_with("LIFE awake,enable,start,update,update,disable,enable,update"), "{life}");
    let effect = names.iter().find(|n| n.starts_with("EFFECT ")).unwrap_or_else(|| panic!("{names:?}"));
    assert!(effect.ends_with(" 400"), "source sized like the Main display: {effect}");
    let views = &gfx.last_views;
    assert!(views.iter().any(|v| v.target.is_some()), "the camera draws into its source texture");
    let screen: Vec<_> = views.iter().filter(|v| v.target.is_none()).collect();
    assert!(screen.len() >= 2, "the effect's blit + drawTexture on the screen: {}", screen.len());
    assert!(screen.iter().any(|v| v.items.iter().any(|i| i.color == goat_core::Color::rgba(1.0, 0.0, 0.0, 1.0))), "drawTexture tinted red");
    let prof = e.resources.get::<goat_runtime::Profiler>().unwrap();
    assert!(prof.last.contains_key("Switch.Work"), "{:?}", prof.last.keys().collect::<Vec<_>>());
}

/// @serializable structs (a value and a list) loaded from props, their defaults for the
/// Inspector, @createAssetMenu, groups.
#[test]
fn serializable_structs_and_asset_menus() {
    fn sio() -> AssetIo {
        let io = io();
        io.put(
            "scripts/Waves.ts",
            br#"
import { Behaviour, ScriptableObject, Vector3, field, serializable, createAssetMenu, boxGroup, textArea } from "goat";
@serializable
export class Wave { count = 3; delay = 1.5; spawn = new Vector3(0, 1, 0); }
export default class Waves extends Behaviour {
  @field(Wave) first: Wave = new Wave();
  @field([Wave]) waves: Wave[] = [];
  @boxGroup("Text") @textArea(4) intro = "";
  start() {
    const w = this.waves[1];
    this.gameObject.name = [this.first instanceof Wave, this.first.count, this.waves.length, w instanceof Wave, w.count, w.delay, w.spawn.y].join(",");
  }
}
@createAssetMenu({ menuName: "Data/Enemy", fileName: "Enemy" })
export class EnemyData extends ScriptableObject { hp = 10; }
"#
            .to_vec(),
        );
        io
    }
    struct W;
    impl Game for W {
        fn start(&mut self, ctx: &mut GameContext) {
            ctx.resources.insert(goat_assets::AssetServer::new(sio()));
            let props = serde_json::json!({ "first": { "count": 9 }, "waves": [{ "count": 1 }, { "count": 4, "delay": 0.25, "spawn": [0, 7, 0] }] });
            ctx.world.spawn((Name("Waves".into()), Transform::default(), GlobalTransform::default(), Script(vec![ScriptRef::new("scripts/Waves.ts").with_props(props)])));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::with_io(sio())))]
        }
    }
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(W);
    for _ in 0..3 {
        e.frame(1.0 / 60.0, &mut gfx);
    }
    assert!(find(&e, "true,9,2,true,4,0.25,7").is_some(), "{:?}", e.world.query::<&Name>().iter().map(|(_, n)| n.0.clone()).collect::<Vec<_>>());
    let mut host = JsHost::with_io(sio());
    e.with_context(&mut gfx, |ctx| {
        let mut api = crate::api_of(ctx);
        let meta = host.field_meta(&mut api, "scripts/Waves.ts").unwrap();
        assert_eq!(meta["fields"]["waves"]["type"], "struct");
        assert_eq!(meta["fields"]["waves"]["structDefaults"], serde_json::json!({ "count": 3, "delay": 1.5, "spawn": [0, 1, 0] }));
        assert_eq!(meta["fields"]["intro"]["group"]["name"], "Text");
        assert_eq!(meta["fields"]["intro"]["multiline"], 4);
    });
}