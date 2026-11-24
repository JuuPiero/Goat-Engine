//! Tween calls for scripts (feature `script`), registered as [`goat_script::Extensions`]. goat.js
//! wraps them as DOTween's API (`transform.doMove(...)`, `DOTween.sequence()`...); C code calls
//! them with `goat_call(g, "Tween.to", e, "[\"position\", [0, 2, 0], 1]", ...)`.
//!
//! Callbacks: a script registers a key with `Tween.on(id, "complete", key)`; fired keys are read
//! once per frame with `Tween.events` (goat.js runs the matching functions).

use goat_core::math::{Vec3, Vec4};
use goat_runtime::Entity;
use goat_script::Api;
use serde_json::{json, Value};

use crate::{tweens, Ease, LoopType, Prop, SeqAdd, Tween, TweenId, Tweens};

fn num(v: Option<&Value>, d: f32) -> f32 {
    v.and_then(|v| v.as_f64()).map(|x| x as f32).unwrap_or(d)
}
fn vec4(v: Option<&Value>) -> Vec4 {
    match v {
        Some(Value::Array(a)) => Vec4::new(num(a.first(), 0.0), num(a.get(1), 0.0), num(a.get(2), 0.0), num(a.get(3), 0.0)),
        Some(Value::Number(n)) => Vec4::new(n.as_f64().unwrap_or(0.0) as f32, 0.0, 0.0, 0.0),
        _ => Vec4::ZERO,
    }
}
fn vec3(v: Option<&Value>) -> Vec3 {
    match v {
        Some(Value::Number(n)) => Vec3::splat(n.as_f64().unwrap_or(0.0) as f32),
        other => vec4(other).truncate(),
    }
}
fn prop(v: Option<&Value>) -> Result<Prop, String> {
    match v.and_then(|v| v.as_str()).unwrap_or("") {
        "position" | "localPosition" => Ok(Prop::Position),
        "scale" | "localScale" => Ok(Prop::Scale),
        "euler" | "eulerAngles" | "rotate" => Ok(Prop::Euler),
        "rotation" => Ok(Prop::Rotation),
        other => Err(format!("Tween: unknown property {other:?} (position, scale, euler, rotation)")),
    }
}
fn id(v: Option<&Value>) -> Result<TweenId, String> {
    v.and_then(|v| v.as_f64()).filter(|x| *x >= 0.0).map(|x| TweenId(x as u32)).ok_or_else(|| "expected a tween id".to_string())
}
fn add(api: &mut Api, t: Tween) -> Value {
    json!(tweens(api.ctx).add(t).0)
}
fn t<'a>(api: &'a mut Api) -> &'a mut Tweens {
    tweens(api.ctx)
}

fn to(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let p = prop(a.first())?;
    let v = vec4(a.get(1));
    let v = if matches!(p, Prop::Rotation) && a.get(1).and_then(|x| x.as_array()).is_some_and(|x| x.len() == 3) {
        // [x, y, z] degrees for a rotation: to a quaternion
        Vec4::from(goat_core::math::Quat::from_euler(goat_core::math::EulerRot::YXZ, v.y.to_radians(), v.x.to_radians(), v.z.to_radians()))
    } else {
        v
    };
    Ok(add(api, Tween::to(e, p, v, num(a.get(2), 1.0))))
}
fn value(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    Ok(add(api, Tween::value(vec4(a.first()), vec4(a.get(1)), num(a.get(2), 1.0))))
}
fn punch(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let p = prop(a.first())?;
    Ok(add(api, Tween::punch(e, p, vec3(a.get(1)), num(a.get(2), 1.0), num(a.get(3), 10.0) as u32, num(a.get(4), 1.0))))
}
fn shake(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let p = prop(a.first())?;
    Ok(add(api, Tween::shake(e, p, num(a.get(1), 1.0), vec3(a.get(2).or(Some(&json!(1.0)))), num(a.get(3), 10.0) as u32, num(a.get(4), 90.0))))
}
fn path(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let pts: Vec<Vec3> = a.first().and_then(|v| v.as_array()).map(|l| l.iter().map(|p| vec3(Some(p))).collect()).unwrap_or_default();
    if pts.is_empty() {
        return Err("Tween.path: expected a list of points".into());
    }
    let closed = a.get(2).and_then(|v| v.as_bool()).unwrap_or(false);
    Ok(add(api, Tween::path(e, pts, num(a.get(1), 1.0), closed)))
}

/// Changes a tween through its by-value builder methods.
fn modify(tw: &mut Tween, f: impl FnOnce(Tween) -> Tween) {
    let old = std::mem::replace(tw, Tween::value(Vec4::ZERO, Vec4::ZERO, 0.0));
    *tw = f(old);
}

fn set(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let i = id(a.first())?;
    let Some(Value::Object(o)) = a.get(1) else { return Err("Tween.set: expected { ease, loops, ... }".into()) };
    let Some(tw) = t(api).get_mut(i) else { return Ok(Value::Bool(false)) };
    for (k, v) in o {
        match k.as_str() {
            "ease" => {
                let e = v.as_str().and_then(Ease::parse).ok_or_else(|| format!("unknown ease {v}"))?;
                modify(tw, |t| t.ease(e));
            }
            "loops" => {
                let n = v.as_f64().unwrap_or(1.0) as i32;
                let lt = o.get("loopType").and_then(|x| x.as_str()).and_then(LoopType::parse).unwrap_or(tw.loop_type);
                modify(tw, |t| t.loops(n, lt));
            }
            "loopType" => {
                if !o.contains_key("loops") {
                    let lt = v.as_str().and_then(LoopType::parse).unwrap_or_default();
                    let n = tw.loops;
                    modify(tw, |t| t.loops(n, lt));
                }
            }
            "delay" => {
                let d = num(Some(v), 0.0);
                modify(tw, |t| t.delay(d));
            }
            "relative" => {
                let r = v.as_bool().unwrap_or(true);
                modify(tw, |t| t.relative(r));
            }
            "from" => {
                if v.as_bool() == Some(true) {
                    modify(tw, |t| t.reversed());
                } else if v.is_array() {
                    let f = vec4(Some(v));
                    modify(tw, |t| t.from(f));
                }
            }
            "speed" => {
                let s = num(Some(v), 1.0);
                modify(tw, |t| t.speed(s));
            }
            "unscaled" => {
                if v.as_bool() == Some(true) {
                    modify(tw, |t| t.unscaled());
                }
            }
            "id" => tw.tag = v.as_str().map(str::to_string),
            "autoKill" => tw.autokill = v.as_bool().unwrap_or(true),
            "lookAhead" => {
                if v.as_bool() == Some(true) {
                    modify(tw, |t| t.look_ahead());
                }
            }
            "paused" => tw.paused = v.as_bool().unwrap_or(false),
            other => return Err(format!("Tween.set: unknown setting {other}")),
        }
    }
    Ok(Value::Bool(true))
}

fn on(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let i = id(a.first())?;
    let kind = a.get(1).and_then(|v| v.as_str()).unwrap_or("complete").to_string();
    let key = a.get(2).and_then(|v| v.as_f64()).ok_or("Tween.on: expected a callback key")? as u64;
    Ok(Value::Bool(t(api).add_event(i, &kind, key)))
}
fn events(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    Ok(json!(t(api).take_events()))
}
fn get(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let i = id(a.first())?;
    Ok(t(api).value(i).map(|v| json!([v.x, v.y, v.z, v.w])).unwrap_or(Value::Null))
}
fn kill(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let i = id(a.first())?;
    let c = a.get(1).and_then(|v| v.as_bool()).unwrap_or(false);
    t(api).kill(i, c);
    Ok(Value::Null)
}
fn kill_target(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let c = a.first().and_then(|v| v.as_bool()).unwrap_or(false);
    t(api).kill_target(e, c);
    Ok(Value::Null)
}
fn kill_tag(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let tag = a.first().and_then(|v| v.as_str()).ok_or("Tween.killTag: expected an id string")?.to_string();
    let c = a.get(1).and_then(|v| v.as_bool()).unwrap_or(false);
    t(api).kill_tag(&tag, c);
    Ok(Value::Null)
}
fn kill_all(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    t(api).kill_all();
    Ok(Value::Null)
}
fn pause(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let i = id(a.first())?;
    t(api).pause(i);
    Ok(Value::Null)
}
fn play(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let i = id(a.first())?;
    t(api).play(i);
    Ok(Value::Null)
}
fn toggle(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let i = id(a.first())?;
    t(api).toggle_pause(i);
    Ok(Value::Null)
}
fn restart(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let i = id(a.first())?;
    t(api).restart(i);
    Ok(Value::Null)
}
fn is_active(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let i = id(a.first())?;
    Ok(Value::Bool(t(api).is_active(i)))
}
fn is_playing(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let i = id(a.first())?;
    Ok(Value::Bool(t(api).is_playing(i)))
}
fn info(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let i = id(a.first())?;
    Ok(t(api).get(i).map(|tw| json!({ "duration": tw.duration(), "elapsed": tw.elapsed(), "complete": tw.is_complete() })).unwrap_or(Value::Null))
}
fn sequence(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    Ok(json!(t(api).new_sequence().0))
}
fn seq_add(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let (s, c) = (id(a.first())?, id(a.get(1))?);
    let mode = match a.get(2) {
        Some(Value::Number(n)) => SeqAdd::Insert(n.as_f64().unwrap_or(0.0) as f32),
        Some(Value::String(m)) if m == "join" => SeqAdd::Join,
        _ => SeqAdd::Append,
    };
    t(api).sequence_add(s, c, mode)?;
    Ok(Value::Null)
}
fn seq_interval(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let s = id(a.first())?;
    t(api).sequence_interval(s, num(a.get(1), 0.0));
    Ok(Value::Null)
}
fn seq_callback(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let s = id(a.first())?;
    let key = a.get(1).and_then(|v| v.as_f64()).ok_or("Tween.seqCallback: expected a callback key")? as u64;
    let at = a.get(2).and_then(|v| v.as_f64()).map(|x| x as f32);
    t(api).sequence_callback(s, at, key);
    Ok(Value::Null)
}
fn time_scale(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    if let Some(s) = a.first().and_then(|v| v.as_f64()) {
        t(api).time_scale = s.max(0.0) as f32;
    }
    Ok(json!(t(api).time_scale))
}
/// Plays the entity's TweenAnimation(s) that aren't playing (DOTweenAnimation.DOPlay).
#[cfg(feature = "scene")]
fn play_animation(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let which = a.first().and_then(|v| v.as_str()).map(str::to_string);
    let built: Vec<(usize, Tween)> = {
        let mut anims = api.ctx.world.get::<&mut crate::scene::TweenAnimations>(e).map_err(|_| "no TweenAnimation on this object".to_string())?;
        anims
            .0
            .iter_mut()
            .enumerate()
            .filter(|(_, an)| which.as_deref().is_none_or(|w| w == an.id))
            .filter_map(|(i, an)| {
                an.started = true;
                an.build(e).ok().map(|t| (i, t))
            })
            .collect()
    };
    for (i, tw) in built {
        let prev = api.ctx.world.get::<&crate::scene::TweenAnimations>(e).ok().and_then(|a| a.0[i].tween);
        if let Some(p) = prev {
            if t(api).is_active(p) {
                t(api).restart(p);
                continue;
            }
        }
        let new = t(api).add(tw);
        if let Ok(mut anims) = api.ctx.world.get::<&mut crate::scene::TweenAnimations>(e) {
            anims.0[i].tween = Some(new);
        }
    }
    Ok(Value::Null)
}

pub fn register(x: &mut goat_script::Extensions) {
    x.add("Tween.to", to)
        .add("Tween.value", value)
        .add("Tween.punch", punch)
        .add("Tween.shake", shake)
        .add("Tween.path", path)
        .add("Tween.set", set)
        .add("Tween.on", on)
        .add("Tween.events", events)
        .add("Tween.get", get)
        .add("Tween.kill", kill)
        .add("Tween.killTarget", kill_target)
        .add("Tween.killTag", kill_tag)
        .add("Tween.killAll", kill_all)
        .add("Tween.pause", pause)
        .add("Tween.play", play)
        .add("Tween.togglePause", toggle)
        .add("Tween.restart", restart)
        .add("Tween.isActive", is_active)
        .add("Tween.isPlaying", is_playing)
        .add("Tween.info", info)
        .add("Tween.sequence", sequence)
        .add("Tween.seqAdd", seq_add)
        .add("Tween.seqInterval", seq_interval)
        .add("Tween.seqCallback", seq_callback)
        .add("Tween.timeScale", time_scale);
    #[cfg(feature = "scene")]
    x.add("Tween.playAnimation", play_animation);
}
