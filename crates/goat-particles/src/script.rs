//! ParticleSystem calls for scripts (feature `script`): Unity's `Play` / `Stop` / `Pause` /
//! `Clear` / `Emit(n)` / `particleCount` / `time` / `isPlaying` on the object's ParticleSystem.

use goat_runtime::Entity;
use goat_script::Api;
use serde_json::{json, Value};

use crate::ParticleSystem;

fn with<R>(api: &mut Api, e: Entity, f: impl FnOnce(&mut ParticleSystem) -> R) -> Result<R, String> {
    let mut p = api.ctx.world.get::<&mut ParticleSystem>(e).map_err(|_| "no ParticleSystem on this object".to_string())?;
    Ok(f(&mut p))
}

/// `[restart]`: emits again (from the start of the cycle when `restart` or stopped).
fn play(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let restart = a.first().and_then(Value::as_bool).unwrap_or(false);
    with(api, e, |p| {
        if restart || !p.playing {
            let keep = std::mem::take(&mut p.particles);
            p.restart();
            if !restart {
                p.particles = keep;
            }
        }
        p.playing = true;
    })
    .map(|_| Value::Null)
}
/// `[clear]`: stops emitting (alive particles finish, or go with `clear`).
fn stop(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let clear = a.first().and_then(Value::as_bool).unwrap_or(false);
    with(api, e, |p| {
        p.playing = false;
        if clear {
            p.particles.clear();
        }
    })
    .map(|_| Value::Null)
}
fn clear(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with(api, e, |p| p.particles.clear()).map(|_| Value::Null)
}
fn emit(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let n = a.first().and_then(Value::as_u64).unwrap_or(1) as u32;
    with(api, e, |p| p.emit(n)).map(|_| Value::Null)
}
/// `{ playing, count, time }`
fn state(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with(api, e, |p| json!({ "playing": p.playing, "count": p.particles.len(), "time": p.time() }))
}

pub fn register(x: &mut goat_script::Extensions) {
    x.add("ParticleSystem.play", play)
        .add("ParticleSystem.stop", stop)
        .add("ParticleSystem.clear", clear)
        .add("ParticleSystem.emit", emit)
        .add("ParticleSystem.state", state);
}