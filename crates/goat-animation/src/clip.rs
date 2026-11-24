//! Authored animation clips (`.anim.json`, Unity's .anim made in the Animation window) and the
//! [`Animation`] component playing them (Unity's legacy Animation).
//!
//! ```json
//! { "length": 2, "loop": true, "sample_rate": 60,
//!   "curves": [
//!     { "path": "", "property": "position.y", "keys": [ { "t": 0, "v": 0 }, { "t": 1, "v": 2 } ] },
//!     { "path": "Arm/Hand", "property": "rotation.z", "interp": "step", "keys": [...] },
//!     { "path": "", "property": "PointLight.intensity", "keys": [...] } ],
//!   "events": [ { "time": 0.5, "name": "step" } ] }
//! ```
//!
//! `path` = names of children from the animated object ("" = itself); `property` =
//! `position.x|y|z`, `rotation.x|y|z` (Euler degrees, like the Inspector), `scale.x|y|z`, or
//! `<Component>.<field>[.<index or field>...]` (a number inside the component's scene JSON).

use std::collections::HashMap;
use std::sync::Arc;

use goat_core::math::{EulerRot, Quat};
use goat_runtime::{Entity, Name, Parent, Transform, World};
use serde::{Deserialize, Serialize};

use crate::ClipEvent;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Interp {
    /// Catmull-Rom through the keys (Unity's auto tangents).
    #[default]
    Smooth,
    Linear,
    /// Holds each key's value until the next one (sprite swaps, switches).
    Step,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub t: f32,
    pub v: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    #[serde(default)]
    pub path: String,
    pub property: String,
    #[serde(default, skip_serializing_if = "is_smooth")]
    pub interp: Interp,
    pub keys: Vec<Key>,
}

fn is_smooth(i: &Interp) -> bool {
    *i == Interp::Smooth
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimClip {
    /// Seconds (the last key when 0).
    #[serde(default)]
    pub length: f32,
    #[serde(default = "yes", rename = "loop")]
    pub looping: bool,
    #[serde(default = "sixty")]
    pub sample_rate: f32,
    #[serde(default)]
    pub curves: Vec<Curve>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<ClipEvent>,
}

fn yes() -> bool {
    true
}
fn sixty() -> f32 {
    60.0
}

impl Default for AnimClip {
    fn default() -> Self {
        AnimClip { length: 1.0, looping: true, sample_rate: 60.0, curves: Vec::new(), events: Vec::new() }
    }
}

impl Curve {
    /// The curve's value at `t` (clamped to its keys).
    pub fn sample(&self, t: f32) -> Option<f32> {
        let k = &self.keys;
        let (first, last) = (k.first()?, k.last()?);
        if t <= first.t || k.len() == 1 {
            return Some(first.v);
        }
        if t >= last.t {
            return Some(last.v);
        }
        let i = k.iter().position(|x| x.t > t)?.max(1);
        let (a, b) = (k[i - 1], k[i]);
        let u = ((t - a.t) / (b.t - a.t).max(1e-6)).clamp(0.0, 1.0);
        Some(match self.interp {
            Interp::Step => a.v,
            Interp::Linear => a.v + (b.v - a.v) * u,
            Interp::Smooth => {
                // Catmull-Rom tangents from the neighbours (flat at the ends), in value / second
                let slope = |j: usize| -> f32 {
                    if j == 0 || j + 1 >= k.len() {
                        0.0
                    } else {
                        (k[j + 1].v - k[j - 1].v) / (k[j + 1].t - k[j - 1].t).max(1e-6)
                    }
                };
                let (m0, m1) = (slope(i - 1) * (b.t - a.t), slope(i) * (b.t - a.t));
                let (u2, u3) = (u * u, u * u * u);
                (2.0 * u3 - 3.0 * u2 + 1.0) * a.v + (u3 - 2.0 * u2 + u) * m0 + (-2.0 * u3 + 3.0 * u2) * b.v + (u3 - u2) * m1
            }
        })
    }
}

impl AnimClip {
    pub fn parse(text: &str) -> Result<AnimClip, String> {
        serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(text)).map_err(|e| e.to_string())
    }
    /// Seconds: `length`, or the last key / event.
    pub fn duration(&self) -> f32 {
        if self.length > 0.0 {
            return self.length;
        }
        self.curves.iter().filter_map(|c| c.keys.last().map(|k| k.t)).chain(self.events.iter().map(|e| e.time)).fold(0.0, f32::max)
    }
    /// The time inside the clip for a play time (wrapped when looping, clamped otherwise).
    pub fn local_time(&self, t: f32) -> f32 {
        let d = self.duration().max(1e-6);
        if self.looping {
            t.rem_euclid(d)
        } else {
            t.clamp(0.0, d)
        }
    }
}

/// The entity at `path` (child names separated by `/`) under `root`.
pub fn find_path(world: &World, root: Entity, path: &str) -> Option<Entity> {
    let mut cur = root;
    for part in path.split('/').filter(|p| !p.is_empty()) {
        cur = world.query::<(&Parent, &Name)>().iter().find(|(_, (p, n))| p.0 == cur && n.0 == part).map(|(e, _)| e)?;
    }
    Some(cur)
}

/// Writes a transform property (`position.x`, `rotation.y` in degrees, `scale.z`).
/// Returns false for other properties (components).
pub fn set_transform_property(t: &mut Transform, property: &str, v: f32) -> bool {
    let Some((field, axis)) = property.split_once('.') else { return false };
    let i = match axis {
        "x" => 0,
        "y" => 1,
        "z" => 2,
        _ => return false,
    };
    match field {
        "position" => t.position[i] = v,
        "scale" => t.scale[i] = v,
        "rotation" => {
            let (y, x, z) = t.rotation.to_euler(EulerRot::YXZ);
            let mut e = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
            e[i] = v;
            t.rotation = Quat::from_euler(EulerRot::YXZ, e[1].to_radians(), e[0].to_radians(), e[2].to_radians());
        }
        _ => return false,
    }
    true
}

/// A transform property's current value.
pub fn get_transform_property(t: &Transform, property: &str) -> Option<f32> {
    let (field, axis) = property.split_once('.')?;
    let i = ["x", "y", "z"].iter().position(|a| *a == axis)?;
    Some(match field {
        "position" => t.position[i],
        "scale" => t.scale[i],
        "rotation" => {
            let (y, x, z) = t.rotation.to_euler(EulerRot::YXZ);
            [x.to_degrees(), y.to_degrees(), z.to_degrees()][i]
        }
        _ => return None,
    })
}

/// Sets the number at `field.a.0...` inside a component's JSON.
pub fn set_json_number(v: &mut serde_json::Value, path: &[&str], x: f32) -> bool {
    let Some((first, rest)) = path.split_first() else {
        *v = serde_json::Value::from(x);
        return true;
    };
    let slot = match v {
        serde_json::Value::Object(m) => m.get_mut(*first),
        serde_json::Value::Array(a) => first.parse::<usize>().ok().and_then(|i| a.get_mut(i)),
        _ => None,
    };
    slot.is_some_and(|s| set_json_number(s, rest, x))
}

/// Plays authored clips (Unity's legacy Animation component): `clip` from the start (looping as
/// the clip says) when `play_automatically`; `clips` are the others scripts can play by key.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Animation {
    pub clip: String,
    pub clips: Vec<String>,
    pub play_automatically: bool,
    pub speed: f32,
    /// Seconds into the playing clip (runtime).
    #[serde(skip)]
    pub time: f32,
    #[serde(skip)]
    pub playing: Option<String>,
}

impl Default for Animation {
    fn default() -> Self {
        Animation { clip: String::new(), clips: Vec::new(), play_automatically: true, speed: 1.0, time: 0.0, playing: None }
    }
}

impl Animation {
    pub fn play(&mut self, key: &str) {
        self.playing = Some(key.to_string());
        self.time = 0.0;
    }
    pub fn stop(&mut self) {
        self.playing = None;
    }
}

/// Loaded `.anim.json` clips by key (a resource).
#[derive(Default)]
pub struct AnimClips(pub HashMap<String, Arc<AnimClip>>);

/// One sampled value: target entity, property, value.
pub fn sample_clip(world: &World, root: Entity, clip: &AnimClip, t: f32) -> Vec<(Entity, String, f32)> {
    let lt = clip.local_time(t);
    clip.curves
        .iter()
        .filter_map(|c| Some((find_path(world, root, &c.path)?, c.property.clone(), c.sample(lt)?)))
        .collect()
}

/// Applies sampled values: transforms directly; component numbers through `set_component`
/// (component name, field path, value) — the scene registry in the plugin, a stub in tests.
pub fn apply_samples(world: &mut World, samples: &[(Entity, String, f32)], mut set_component: impl FnMut(&mut World, Entity, &str, &[&str], f32)) {
    for (e, property, v) in samples {
        if let Ok(mut t) = world.get::<&mut Transform>(*e) {
            if set_transform_property(&mut t, property, *v) {
                continue;
            }
        }
        let parts: Vec<&str> = property.split('.').collect();
        if parts.len() >= 2 {
            set_component(world, *e, parts[0], &parts[1..], *v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goat_core::math::Vec3;

    #[test]
    fn curves_interpolate_and_clips_drive_children() {
        let clip = AnimClip::parse(r#"{ "length": 2, "curves": [
            { "path": "", "property": "position.y", "interp": "linear", "keys": [ { "t": 0, "v": 0 }, { "t": 1, "v": 2 } ] },
            { "path": "Arm", "property": "rotation.z", "interp": "step", "keys": [ { "t": 0, "v": 0 }, { "t": 1, "v": 90 } ] },
            { "path": "", "property": "scale.x", "keys": [ { "t": 0, "v": 1 }, { "t": 1, "v": 3 }, { "t": 2, "v": 1 } ] } ] }"#)
        .unwrap();
        let mut w = World::new();
        let root = w.spawn((Name("Root".into()), Transform::default()));
        let arm = w.spawn((Name("Arm".into()), Transform::default(), Parent(root)));
        let s = sample_clip(&w, root, &clip, 0.5);
        apply_samples(&mut w, &s, |_, _, _, _, _| {});
        assert!((w.get::<&Transform>(root).unwrap().position.y - 1.0).abs() < 1e-5, "linear half-way");
        assert_eq!(get_transform_property(&w.get::<&Transform>(arm).unwrap(), "rotation.z").unwrap().round(), 0.0, "step holds");
        let s = sample_clip(&w, root, &clip, 1.0);
        apply_samples(&mut w, &s, |_, _, _, _, _| {});
        assert!((get_transform_property(&w.get::<&Transform>(arm).unwrap(), "rotation.z").unwrap() - 90.0).abs() < 1e-3);
        assert!((w.get::<&Transform>(root).unwrap().scale.x - 3.0).abs() < 1e-5, "smooth passes through its keys");
        // smooth keeps the peak (flat tangent at a local max in the middle: no overshoot beyond 3)
        let peak = clip.curves[2].sample(1.0).unwrap().max(clip.curves[2].sample(0.95).unwrap()).max(clip.curves[2].sample(1.05).unwrap());
        assert!(peak <= 3.0 + 1e-4, "{peak}");
        // looping wraps
        assert!((clip.local_time(2.5) - 0.5).abs() < 1e-5);
        let mut v = serde_json::json!({ "color": [1, 0.5, 0.2, 1], "intensity": 1 });
        assert!(set_json_number(&mut v, &["color", "3"], 0.25) && set_json_number(&mut v, &["intensity"], 4.0));
        assert_eq!(v["color"][3], 0.25);
        let _ = Vec3::ZERO;
    }
}
