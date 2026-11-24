//! Audio calls for scripts (feature `script`).
//!
//! - `Audio.playOneShot(clip, volume?, group?)`, `Audio.playAt(clip, [x, y, z], volume?)`
//! - on an object with an AudioSource: `AudioSource.play / stop / pause / unpause / isPlaying / time`
//! - `AudioMixer.set(group, { volume, mute, lowpass })`, `AudioMixer.get(group)`, `Audio.masterVolume(v)`

use goat_core::math::Vec3;
use goat_runtime::Entity;
use goat_script::Api;
use serde_json::{json, Value};

use crate::{Audio, AudioSource};

fn s(a: &[Value], i: usize) -> String {
    a.get(i).and_then(|v| v.as_str()).unwrap_or("").to_string()
}
fn f(a: &[Value], i: usize, d: f32) -> f32 {
    a.get(i).and_then(|v| v.as_f64()).map(|x| x as f32).unwrap_or(d)
}

fn one_shot(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let (clip, vol, group) = (s(a, 0), f(a, 1, 1.0), s(a, 2));
    crate::play_one_shot_in(api.ctx, &clip, vol, &group);
    Ok(Value::Null)
}
fn play_at(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let xyz = a.get(1).and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let p = Vec3::new(f(&xyz, 0, 0.0), f(&xyz, 1, 0.0), f(&xyz, 2, 0.0));
    crate::play_at(api.ctx, &s(a, 0), p, f(a, 2, 1.0), 1.0);
    Ok(Value::Null)
}
fn with_source<R>(api: &mut Api, e: Entity, f: impl FnOnce(&mut AudioSource) -> R) -> Result<R, String> {
    let mut src = api.ctx.world.get::<&mut AudioSource>(e).map_err(|_| "no AudioSource on this object".to_string())?;
    Ok(f(&mut src))
}
fn play(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with_source(api, e, |s| s.play()).map(|_| Value::Null)
}
fn stop(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with_source(api, e, |s| s.stop()).map(|_| Value::Null)
}
fn pause(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with_source(api, e, |s| s.pause()).map(|_| Value::Null)
}
fn unpause(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with_source(api, e, |s| s.unpause()).map(|_| Value::Null)
}
fn is_playing(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with_source(api, e, |s| s.is_playing()).map(Value::from)
}
fn time(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    let v = with_source(api, e, |s| s.voice())?;
    let t = v.and_then(|v| api.ctx.resources.get::<Audio>().and_then(|a| a.voice_time(v))).unwrap_or(0.0);
    Ok(json!(t))
}
fn mixer_set(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let name = s(a, 0);
    let o = a.get(1).cloned().unwrap_or(Value::Null);
    let num = |k: &str| o.get(k).and_then(|v| v.as_f64()).map(|x| x as f32);
    let audio = api.ctx.resources.get_or_default::<Audio>();
    // volumeDb: Unity's exposed mixer parameters are in decibels
    let volume = num("volume").or_else(|| num("volumeDb").map(|db| 10f32.powf(db / 20.0)));
    if !audio.set_group(&name, volume, o.get("mute").and_then(|v| v.as_bool()), num("lowpass")) {
        return Err(format!("no mixer group {name}"));
    }
    Ok(Value::Null)
}
fn mixer_get(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let audio = api.ctx.resources.get_or_default::<Audio>();
    Ok(audio.group_settings(&s(a, 0)).map(|(v, m, l)| json!({ "volume": v, "mute": m, "lowpass": l })).unwrap_or(Value::Null))
}
fn master(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let audio = api.ctx.resources.get_or_default::<Audio>();
    audio.set_master_volume(f(a, 0, 1.0));
    Ok(Value::Null)
}

pub fn register(x: &mut goat_script::Extensions) {
    x.add("Audio.playOneShot", one_shot)
        .add("Audio.playAt", play_at)
        .add("Audio.masterVolume", master)
        .add("AudioSource.play", play)
        .add("AudioSource.stop", stop)
        .add("AudioSource.pause", pause)
        .add("AudioSource.unpause", unpause)
        .add("AudioSource.isPlaying", is_playing)
        .add("AudioSource.time", time)
        .add("AudioMixer.set", mixer_set)
        .add("AudioMixer.get", mixer_get);
}
