//! SpriteAnimator calls for scripts (feature `script`): `SpriteAnimator.play(name)`,
//! `SpriteAnimator.restart(name)`, `SpriteAnimator.current()`, `SpriteAnimator.finished()`,
//! `SpriteAnimator.setSpeed(s)`, `SpriteAnimator.setPaused(b)`.

use goat_runtime::Entity;
use goat_script::Api;
use serde_json::{json, Value};

use crate::SpriteAnimator;

fn with<R>(api: &mut Api, e: Entity, f: impl FnOnce(&mut SpriteAnimator) -> R) -> Result<R, String> {
    let mut a = api.ctx.world.get::<&mut SpriteAnimator>(e).map_err(|_| "no SpriteAnimator on this object".to_string())?;
    Ok(f(&mut a))
}
fn name(a: &[Value]) -> String {
    a.first().and_then(|v| v.as_str()).unwrap_or("").to_string()
}
fn play(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let n = name(a);
    with(api, e, |s| s.play(&n)).map(|_| Value::Null)
}
fn restart(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let n = name(a);
    with(api, e, |s| s.restart(&n)).map(|_| Value::Null)
}
fn current(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with(api, e, |s| json!(s.play))
}
fn finished(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with(api, e, |s| json!(s.finished))
}
fn set_speed(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let v = a.first().and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
    with(api, e, |s| s.speed = v).map(|_| Value::Null)
}
fn set_paused(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let v = a.first().and_then(|v| v.as_bool()).unwrap_or(true);
    with(api, e, |s| s.paused = v).map(|_| Value::Null)
}

pub fn register(x: &mut goat_script::Extensions) {
    x.add("SpriteAnimator.play", play)
        .add("SpriteAnimator.restart", restart)
        .add("SpriteAnimator.current", current)
        .add("SpriteAnimator.finished", finished)
        .add("SpriteAnimator.setSpeed", set_speed)
        .add("SpriteAnimator.setPaused", set_paused);
}
