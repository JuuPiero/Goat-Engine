//! `Animator` in scene files (feature `scene`): `"Animator": { "play": ["Sway", "Hop"], "speed": 1 }`
//! (every listed clip plays from the start, looping), or driven by an Animator Controller:
//! `"Animator": { "controller": "animators/tentacle.controller.json", "parameters": { "speed": 0.5 } }`.

use goat_runtime::Entity;
use goat_scene::{ComponentCodec, LoadContext, SaveContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Animator;

#[derive(Serialize, Deserialize)]
struct AnimatorJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    controller: Option<String>,
    /// Initial parameter values (controller).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    parameters: std::collections::BTreeMap<String, f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    play: Vec<String>,
    #[serde(default = "one")]
    speed: f32,
}

fn one() -> f32 {
    1.0
}

fn save(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let a = ctx.world.get::<&Animator>(e).ok()?;
    if let Some(key) = &a.controller_key {
        // parameters that differ from the controller's defaults
        let mut parameters = std::collections::BTreeMap::new();
        if let Some(c) = &a.controller {
            for p in &c.data.parameters {
                let v = c.get(&p.name).unwrap_or(0.0);
                if p.kind != crate::controller::ParamKind::Trigger && v != p.default {
                    parameters.insert(p.name.clone(), v);
                }
            }
        }
        return serde_json::to_value(AnimatorJson { controller: Some(key.clone()), parameters, play: Vec::new(), speed: a.speed }).ok();
    }
    let speed = a.states.first().map(|s| s.speed).unwrap_or(1.0);
    serde_json::to_value(AnimatorJson { controller: None, parameters: Default::default(), play: a.states.iter().map(|s| s.clip.clone()).collect(), speed }).ok()
}

fn load(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: AnimatorJson = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    if let Some(key) = &j.controller {
        let mut a = Animator::from_controller_asset(ctx.assets, &key)?;
        a.speed = j.speed;
        for (k, v) in &j.parameters {
            a.set_float(k, *v);
        }
        return ctx.world.insert_one(e, a).map_err(|_| "entity vanished".to_string());
    }
    let mut a = Animator::default();
    for c in &j.play {
        a.add(c, 1.0);
    }
    for s in &mut a.states {
        s.speed = j.speed;
    }
    ctx.world.insert_one(e, a).map_err(|_| "entity vanished".to_string())
}

pub const CODEC: ComponentCodec = ComponentCodec { name: "Animator", save, load, remove: goat_scene::remove_typed::<Animator> };
