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
    let dir = std::env::temp_dir().join(format!("goat-cs-test-{}", std::process::id()));
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
    let dir = std::env::temp_dir().join(format!("goat-cs-fx-{}", std::process::id()));
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
