use super::*;
use goat_gfx::NullBackend;
use goat_runtime::{Button, Context, Engine, Game, GlobalTransform, Name, Plugin, Transform};
use goat_script::{Script, ScriptPlugin, ScriptRef};

extern "C" {
    fn goat_example_c_init();
    fn goat_example_cpp_init();
}

struct G;
impl Game for G {
    fn start(&mut self, ctx: &mut Context) {
        ctx.resources.insert(goat_assets::AssetServer::new(goat_assets::AssetIo::new(".")));
        goat_script::extensions(ctx).add("Test.double", |_, _, args| Ok(Value::from(args[0].as_f64().unwrap_or(0.0) * 2.0)));
        ctx.world.spawn((Name("Bobber".into()), Transform::from_position(Vec3::new(0.0, 1.0, 0.0)), GlobalTransform::default(), Script(vec![ScriptRef::new("native:Wobble").with_props(serde_json::json!({ "amount": 2 }))])));
        ctx.world.spawn((Name("Moon".into()), Transform::default(), GlobalTransform::default(), Script(vec![ScriptRef::new("native:Orbit").with_props(serde_json::json!({ "radius": 5 }))])));
    }
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(NativeHost::new()))]
    }
}

fn find(e: &Engine, name: &str) -> Option<Entity> {
    e.world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e)
}

#[test]
fn c_and_cpp_behaviours_drive_the_engine() {
    unsafe {
        goat_example_c_init();
        goat_example_cpp_init();
    }
    assert!(registered().contains(&"Wobble".to_string()) && registered().contains(&"Orbit".to_string()));
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(G);
    e.frame(0.0, &mut gfx);
    e.frame(0.1, &mut gfx); // (a frame is at most Time::max_delta = 0.1 s)
    // C: base 1 + sin(0.1 π) × 2
    let bob = find(&e, "Bobber").unwrap();
    let y = e.world.get::<&Transform>(bob).unwrap().position.y;
    assert!((y - (1.0 + (0.1f32 * std::f32::consts::PI).sin() * 2.0)).abs() < 1e-4, "wobble y {y}");
    // C++: 0.1 rad around a circle of radius 5
    let moon = find(&e, "Moon").unwrap();
    let p = e.world.get::<&Transform>(moon).unwrap().position;
    assert!((p - Vec3::new(0.1f32.cos() * 5.0, 0.0, 0.1f32.sin() * 5.0)).length() < 1e-4, "orbit {p}");
    // input, spawning, components as JSON, extension calls from C
    e.input.begin_frame();
    e.input.set(Button::A, true);
    e.frame(1.0 / 60.0, &mut gfx);
    let m = find(&e, "Marker:sphere").expect("spawned + renamed by the C behaviour");
    assert_eq!(e.world.get::<&Transform>(m).unwrap().position.y, 7.0);
    // OnDestroy + free when the entity goes
    e.world.despawn(bob).unwrap();
    e.frame(1.0 / 60.0, &mut gfx);
}
