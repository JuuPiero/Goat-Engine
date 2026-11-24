use super::*;
use goat_gfx::NullBackend;
use goat_runtime::{Engine, Game};

/// A host whose "scripts" are named Rust closures: checks the plugin's lifecycle and the Api.
#[derive(Default)]
struct Fake {
    log: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    next: u32,
}

impl ScriptHost for Fake {
    fn name(&self) -> &str {
        "fake"
    }
    fn handles(&self, src: &str) -> bool {
        src.starts_with("fake:")
    }
    fn instantiate(&mut self, api: &mut Api, entity: Entity, src: &str, props: &Value) -> Result<u32, String> {
        if src == "fake:broken" {
            return Err("does not compile".into());
        }
        self.next += 1;
        self.log.borrow_mut().push(format!("new {} {} speed={}", src, api.name(entity).unwrap_or_default(), props["speed"]));
        Ok(self.next)
    }
    fn call(&mut self, api: &mut Api, instance: u32, entity: Entity, method: Method) -> Result<(), String> {
        if method == Method::Update {
            // move along X, read input, touch a component through JSON
            let p = api.position(entity);
            api.set_position(entity, p + Vec3::X);
            if api.button("A").1 {
                self.log.borrow_mut().push("A pressed".into());
            }
            if api.get_component(entity, "Camera").is_some() {
                return Err("cameras don't move".into());
            }
        }
        if method != Method::Update && method != Method::LateUpdate && method != Method::FixedUpdate {
            self.log.borrow_mut().push(format!("{} #{instance}", method.js_name()));
        }
        Ok(())
    }
    fn destroy(&mut self, instance: u32) {
        self.log.borrow_mut().push(format!("destroy #{instance}"));
    }
}

struct G(std::rc::Rc<std::cell::RefCell<Vec<String>>>);
impl Game for G {
    fn start(&mut self, ctx: &mut Context) {
        ctx.resources.insert(goat_assets::AssetServer::new(goat_assets::AssetIo::new(".")));
        ctx.world.spawn((Name("Mover".into()), Transform::default(), GlobalTransform::default(), Script(vec![ScriptRef::new("fake:mover").with_props(serde_json::json!({ "speed": 3 }))])));
        ctx.world.spawn((Name("Bad".into()), Transform::default(), Script::one("fake:broken")));
        ctx.world.spawn((Name("Nobody".into()), Transform::default(), Script::one("lua:nope")));
    }
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        let fake = Fake { log: self.0.clone(), next: 0 };
        vec![Box::new(goat_scene::ScenePlugin), Box::new(ScriptPlugin::new().with_host(fake))]
    }
}

#[test]
fn lifecycle_api_and_failures() {
    let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(G(log.clone()));
    e.frame(1.0 / 60.0, &mut gfx);
    e.frame(1.0 / 60.0, &mut gfx);
    let mover = e.world.query::<&Name>().iter().find(|(_, n)| n.0 == "Mover").unwrap().0;
    assert_eq!(e.world.get::<&Transform>(mover).unwrap().position.x, 2.0, "Update ran on both frames");
    e.input.begin_frame();
    e.input.set(Button::A, true);
    e.frame(1.0 / 60.0, &mut gfx);
    // a camera (seen through the JSON component API) makes the script throw → disabled, logged
    e.world.insert_one(mover, goat_runtime::Camera::default()).unwrap();
    e.frame(1.0 / 60.0, &mut gfx);
    let x = e.world.get::<&Transform>(mover).unwrap().position.x;
    e.frame(1.0 / 60.0, &mut gfx);
    assert_eq!(e.world.get::<&Transform>(mover).unwrap().position.x, x, "disabled after throwing");
    // despawn → destroy
    e.world.despawn(mover).unwrap();
    e.frame(1.0 / 60.0, &mut gfx);
    let log = log.borrow().clone();
    assert_eq!(log[0], "new fake:mover Mover speed=3");
    assert!(log.contains(&"start #1".to_string()));
    assert!(log.contains(&"A pressed".to_string()));
    assert!(log.contains(&"destroy #1".to_string()), "{log:?}");
}

#[test]
fn scene_codec_accepts_one_or_many() {
    let one: Value = serde_json::json!({ "src": "scripts/A.ts", "props": { "x": 1 } });
    let many: Value = serde_json::json!([{ "src": "scripts/A.ts" }, { "src": "native:B", "enabled": false }]);
    let a: ScriptRef = serde_json::from_value(one).unwrap();
    assert_eq!(a.props["x"], 1);
    let b: Vec<ScriptRef> = serde_json::from_value(many).unwrap();
    assert!(!b[1].enabled);
    assert_eq!(serde_json::to_value(&b[0]).unwrap(), serde_json::json!({ "src": "scripts/A.ts" }));
}
