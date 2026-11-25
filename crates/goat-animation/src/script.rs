//! Animator calls for scripts (feature `script`), registered as [`goat_script::Extensions`]:
//! `gameObject.call("Animator.setTrigger", "poke")` (TS / C#), `goat_call(g, "Animator.setFloat", e, "[\"speed\", 2]", ...)` (C).

use goat_runtime::Entity;
use goat_script::Api;
use serde_json::Value;

use crate::Animator;

fn with<R>(api: &mut Api, e: Entity, f: impl FnOnce(&mut Animator) -> R) -> Result<R, String> {
    let mut a = api.ctx.world.get::<&mut Animator>(e).map_err(|_| "no Animator on this object".to_string())?;
    Ok(f(&mut a))
}
fn name(args: &[Value]) -> Result<String, String> {
    args.first().and_then(|v| v.as_str()).map(str::to_string).ok_or_else(|| "expected a parameter / state name".to_string())
}
fn number(args: &[Value]) -> f32 {
    args.get(1).and_then(|v| v.as_f64().or_else(|| v.as_bool().map(|b| b as u8 as f64))).unwrap_or(0.0) as f32
}

fn set_float(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let (n, v) = (name(a)?, number(a));
    with(api, e, |an| an.set_float(&n, v)).map(|_| Value::Null)
}
fn set_integer(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let (n, v) = (name(a)?, number(a));
    with(api, e, |an| an.set_integer(&n, v as i32)).map(|_| Value::Null)
}
fn set_bool(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let (n, v) = (name(a)?, number(a));
    with(api, e, |an| an.set_bool(&n, v != 0.0)).map(|_| Value::Null)
}
fn set_trigger(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let n = name(a)?;
    with(api, e, |an| an.set_trigger(&n)).map(|_| Value::Null)
}
fn reset_trigger(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let n = name(a)?;
    with(api, e, |an| an.reset_trigger(&n)).map(|_| Value::Null)
}
fn get_float(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let n = name(a)?;
    with(api, e, |an| Value::from(an.get_float(&n) as f64))
}
fn get_bool(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let n = name(a)?;
    with(api, e, |an| Value::from(an.get_bool(&n)))
}
fn current_state(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let layer = a.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as usize;
    with(api, e, |an| an.current_state(layer).map(Value::from).unwrap_or(Value::Null))
}
fn play(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let n = name(a)?;
    with(api, e, |an| an.play(&n)).map(|_| Value::Null)
}
fn cross_fade(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let (n, s) = (name(a)?, number(a));
    with(api, e, |an| an.crossfade(&n, s.max(0.0))).map(|_| Value::Null)
}

fn events(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with(api, e, |an| Value::Array(an.fired.iter().map(|f| serde_json::json!({ "name": f.name, "parameter": f.parameter, "clip": f.clip })).collect()))
}
fn normalized_time(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let layer = a.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as usize;
    with(api, e, |an| Value::from(an.controller.as_ref().map(|c| c.normalized_time(layer)).unwrap_or(0.0) as f64))
}
fn set_speed(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let s = a.first().and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
    with(api, e, |an| an.speed = s).map(|_| Value::Null)
}

fn with_anim<R>(api: &mut Api, e: Entity, f: impl FnOnce(&mut crate::Animation) -> R) -> Result<R, String> {
    let mut a = api.ctx.world.get::<&mut crate::Animation>(e).map_err(|_| "no Animation on this object".to_string())?;
    Ok(f(&mut a))
}

/// `Animation.play(clip?)`: the clip (an .anim.json key, or the component's clip) from the start.
fn anim_play(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let key = a.first().and_then(Value::as_str).map(str::to_string);
    with_anim(api, e, |an| {
        let k = key.unwrap_or_else(|| an.clip.clone());
        an.play(&k);
    })
    .map(|_| Value::Null)
}
fn anim_stop(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with_anim(api, e, |an| an.stop()).map(|_| Value::Null)
}
fn anim_is_playing(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with_anim(api, e, |an| Value::Bool(an.playing.is_some()))
}
fn anim_time(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let set = a.first().and_then(Value::as_f64);
    with_anim(api, e, |an| {
        if let Some(t) = set {
            an.time = t as f32;
        }
        Value::from(an.time as f64)
    })
}
fn anim_speed(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let s = a.first().and_then(Value::as_f64).unwrap_or(1.0) as f32;
    with_anim(api, e, |an| an.speed = s).map(|_| Value::Null)
}

pub fn register(x: &mut goat_script::Extensions) {
    x.add("Animation.play", anim_play).add("Animation.stop", anim_stop).add("Animation.isPlaying", anim_is_playing).add("Animation.time", anim_time).add("Animation.setSpeed", anim_speed);
    x.add("Animator.events", events).add("Animator.normalizedTime", normalized_time).add("Animator.setSpeed", set_speed);
    x.add("Animator.setFloat", set_float)
        .add("Animator.setInteger", set_integer)
        .add("Animator.setBool", set_bool)
        .add("Animator.setTrigger", set_trigger)
        .add("Animator.resetTrigger", reset_trigger)
        .add("Animator.getFloat", get_float)
        .add("Animator.getBool", get_bool)
        .add("Animator.currentState", current_state)
        .add("Animator.play", play)
        .add("Animator.crossFade", cross_fade);
}
