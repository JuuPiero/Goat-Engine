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
        vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(JsHost::new(io())))]
    }
}

fn find(e: &Engine, name: &str) -> Option<Entity> {
    e.world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e)
}

#[test]
fn typescript_behaviours_run_with_the_engine_api() {
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(G);
    e.frame(0.5, &mut gfx); // start (time doesn't advance on the first frame)
    e.frame(0.5, &mut gfx);
    let s = find(&e, "Spinner").unwrap();
    let t = *e.world.get::<&Transform>(s).unwrap();
    // props: speed 45 → 90°/s doubled by util.ts; offset → start position; 2 updates moved x by 2
    assert_eq!(t.position, Vec3::new(2.0, 2.0, 0.0), "start + translate");
    let (y, _, _) = t.rotation.to_euler(EulerRot::YXZ);
    assert!((y.to_degrees() - 45.0).abs() < 0.01, "rotated 90°/s × 0.5 s (first frame dt = 0): {}", y.to_degrees());

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

#[cfg(feature = "typescript")]
#[test]
fn typescript_compiles_and_reports_errors() {
    let js = compile_typescript("a.ts", "export const n: number = 1 as number; interface I { x: number }\nexport type T = I;").unwrap();
    assert!(js.contains("export const n = 1"), "{js}");
    assert!(!js.contains("interface"));
    assert!(compile_typescript("b.ts", "let = ;").is_err());
}
