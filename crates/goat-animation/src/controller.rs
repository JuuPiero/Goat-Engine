//! Animator Controller (Unity's Mecanim state machine) — `.controller.json` assets:
//!
//! ```json
//! {
//!   "parameters": [ { "name": "speed", "type": "float" }, { "name": "jump", "type": "trigger" } ],
//!   "layers": [ {
//!     "name": "Base Layer",
//!     "default_state": "Locomotion",
//!     "states": [
//!       { "name": "Locomotion", "blend_tree": { "parameter": "speed",
//!           "children": [ { "clip": "Idle", "threshold": 0 }, { "clip": "Run", "threshold": 1 } ] } },
//!       { "name": "Jump", "clip": "Hop", "loop": false }
//!     ],
//!     "transitions": [
//!       { "from": "Any State", "to": "Jump", "conditions": [ { "parameter": "jump", "mode": "if" } ], "duration": 0.1 },
//!       { "from": "Jump", "to": "Locomotion", "has_exit_time": true, "exit_time": 0.9, "duration": 0.2 }
//!     ]
//!   } ]
//! }
//! ```
//!
//! - Parameters: `float`, `int`, `bool`, `trigger` (a trigger stays set until a transition uses it).
//! - States play a clip of the animator's model, or a 1D blend tree (children weighted by a
//!   float parameter, played in sync by normalized time). `speed` × an optional
//!   `speed_parameter`; `loop` (default true).
//! - Transitions from a state or from `"Any State"`: every condition must hold (`if` / `ifnot`
//!   for bools and triggers, `greater` / `less` / `equals` / `notequal` against `threshold`),
//!   plus the exit time (normalized) when `has_exit_time`. `duration` = crossfade seconds.
//!   Any State transitions are checked first and never re-enter the current state unless
//!   `can_transition_to_self`.
//! - Layers: layer 0 poses the model; each next layer overrides it by its `weight` — or adds to
//!   it (`"blending": "additive"`: the clip's change from its first frame). `"mask"` (Unity's
//!   Avatar Mask) limits a layer to the listed nodes and their descendants.
//! - 2D blend trees (`"type": "2d"`, Unity's Freeform Cartesian): children at `position`
//!   [x, y] weighted by (`parameter`, `parameter_y`) with gradient band interpolation.
//! - Clip events (Unity's Animation Events): `"events": { "Walk": [ { "time": 0.3, "name": "footstep" } ] }`
//!   (seconds into the clip) — reported by the Animator when playback crosses them.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub const ANY_STATE: &str = "Any State";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    #[default]
    Float,
    Int,
    Bool,
    Trigger,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: ParamKind,
    #[serde(default)]
    pub default: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConditionMode {
    If,
    IfNot,
    Greater,
    Less,
    Equals,
    NotEqual,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub parameter: String,
    pub mode: ConditionMode,
    #[serde(default)]
    pub threshold: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlendChild {
    pub clip: String,
    #[serde(default)]
    pub threshold: f32,
    /// 2D trees: where the child sits in (parameter, parameter_y) space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 2]>,
    #[serde(default = "one")]
    pub speed: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendKind {
    /// One float parameter, children at thresholds.
    #[default]
    #[serde(rename = "1d")]
    OneD,
    /// Two parameters, children at 2D positions (Freeform Cartesian).
    #[serde(rename = "2d")]
    TwoD,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlendTree {
    #[serde(rename = "type", default, skip_serializing_if = "is_1d")]
    pub kind: BlendKind,
    pub parameter: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_y: Option<String>,
    pub children: Vec<BlendChild>,
}

fn is_1d(k: &BlendKind) -> bool {
    *k == BlendKind::OneD
}

/// Gradient band interpolation (Unity's Freeform Cartesian): weights of `points` at `p`, summing to 1.
pub fn gradient_band_weights(points: &[[f32; 2]], p: [f32; 2]) -> Vec<f32> {
    let n = points.len();
    let mut w = vec![0.0f32; n];
    if n == 0 {
        return w;
    }
    if n == 1 {
        w[0] = 1.0;
        return w;
    }
    for i in 0..n {
        let pi = points[i];
        let mut h: f32 = 1.0;
        for (j, pj) in points.iter().enumerate() {
            if i == j {
                continue;
            }
            let (ix, iy) = (pj[0] - pi[0], pj[1] - pi[1]);
            let len2 = ix * ix + iy * iy;
            if len2 < 1e-12 {
                continue;
            }
            let (px, py) = (p[0] - pi[0], p[1] - pi[1]);
            h = h.min(1.0 - (px * ix + py * iy) / len2);
        }
        w[i] = h.max(0.0);
    }
    let sum: f32 = w.iter().sum();
    if sum > 0.0 {
        for x in w.iter_mut() {
            *x /= sum;
        }
    } else {
        // outside every band: the nearest point
        let k = (0..n).min_by(|&a, &b| dist2(points[a], p).total_cmp(&dist2(points[b], p))).unwrap_or(0);
        w[k] = 1.0;
    }
    w
}

fn dist2(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

/// A named moment of a clip (Unity's AnimationEvent).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipEvent {
    /// Seconds from the clip's start.
    pub time: f32,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub parameter: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayerBlending {
    #[default]
    Override,
    Additive,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend_tree: Option<BlendTree>,
    #[serde(default = "one")]
    pub speed: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_parameter: Option<String>,
    #[serde(rename = "loop", default = "yes")]
    pub looping: bool,
    /// Where the node sits in the editor's graph.
    #[serde(default)]
    pub position: [f32; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub has_exit_time: bool,
    #[serde(default = "exit")]
    pub exit_time: f32,
    #[serde(default = "fade")]
    pub duration: f32,
    #[serde(default)]
    pub can_transition_to_self: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    #[serde(default = "one")]
    pub weight: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_state: Option<String>,
    pub states: Vec<State>,
    #[serde(default)]
    pub transitions: Vec<Transition>,
    /// Where the Any State node sits in the editor's graph.
    #[serde(default)]
    pub any_state_position: [f32; 2],
    #[serde(default, skip_serializing_if = "is_override")]
    pub blending: LayerBlending,
    /// Avatar mask: node names (each with its descendants); empty = every node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mask: Vec<String>,
}

fn is_override(b: &LayerBlending) -> bool {
    *b == LayerBlending::Override
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ControllerData {
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    pub layers: Vec<Layer>,
    /// Events by clip name.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub events: std::collections::BTreeMap<String, Vec<ClipEvent>>,
}

fn one() -> f32 {
    1.0
}
fn yes() -> bool {
    true
}
fn exit() -> f32 {
    0.9
}
fn fade() -> f32 {
    0.25
}

impl ControllerData {
    pub fn parse(text: &str) -> Result<Self, String> {
        let d: ControllerData = serde_json::from_str(text).map_err(|e| e.to_string())?;
        d.validate()?;
        Ok(d)
    }
    /// Names used by transitions / conditions / blend trees must exist.
    pub fn validate(&self) -> Result<(), String> {
        let param = |n: &str| self.parameters.iter().any(|p| p.name == n);
        for l in &self.layers {
            if l.states.is_empty() {
                return Err(format!("layer {}: no states", l.name));
            }
            let state = |n: &str| l.states.iter().any(|s| s.name == n);
            if let Some(d) = &l.default_state {
                if !state(d) {
                    return Err(format!("layer {}: default state {d} doesn't exist", l.name));
                }
            }
            for s in &l.states {
                if let Some(b) = &s.blend_tree {
                    if !param(&b.parameter) {
                        return Err(format!("state {}: unknown parameter {}", s.name, b.parameter));
                    }
                    if b.kind == BlendKind::TwoD {
                        match &b.parameter_y {
                            Some(p) if param(p) => {}
                            Some(p) => return Err(format!("state {}: unknown parameter {p}", s.name)),
                            None => return Err(format!("state {}: a 2D blend tree needs parameter_y", s.name)),
                        }
                    }
                }
                if let Some(p) = &s.speed_parameter {
                    if !param(p) {
                        return Err(format!("state {}: unknown parameter {p}", s.name));
                    }
                }
            }
            for t in &l.transitions {
                if t.from != ANY_STATE && !state(&t.from) {
                    return Err(format!("transition from unknown state {}", t.from));
                }
                if !state(&t.to) {
                    return Err(format!("transition to unknown state {}", t.to));
                }
                for c in &t.conditions {
                    if !param(&c.parameter) {
                        return Err(format!("transition {} → {}: unknown parameter {}", t.from, t.to, c.parameter));
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
struct LayerRun {
    state: usize,
    /// Normalized time of the current state (1 = one cycle).
    time: f32,
    /// (target state, its normalized time, elapsed seconds, duration)
    next: Option<(usize, f32, f32, f32)>,
}

/// One clip to sample: (clip name, time in seconds, weight).
pub type ClipSample = (String, f32, f32);

/// A controller instance (parameters + where each layer is).
#[derive(Clone, Debug)]
pub struct ControllerState {
    pub data: Arc<ControllerData>,
    params: Vec<f32>,
    layers: Vec<LayerRun>,
}

impl ControllerState {
    pub fn new(data: Arc<ControllerData>) -> Self {
        let params = data.parameters.iter().map(|p| if p.kind == ParamKind::Trigger { 0.0 } else { p.default }).collect();
        let layers = data
            .layers
            .iter()
            .map(|l| LayerRun { state: l.default_state.as_deref().and_then(|d| l.states.iter().position(|s| s.name == d)).unwrap_or(0), ..Default::default() })
            .collect();
        ControllerState { data, params, layers }
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.data.parameters.iter().position(|p| p.name == name)
    }
    pub fn set(&mut self, name: &str, v: f32) -> bool {
        match self.index(name) {
            Some(i) => {
                self.params[i] = match self.data.parameters[i].kind {
                    ParamKind::Int => v.round(),
                    ParamKind::Bool | ParamKind::Trigger => (v != 0.0) as u8 as f32,
                    ParamKind::Float => v,
                };
                true
            }
            None => false,
        }
    }
    pub fn get(&self, name: &str) -> Option<f32> {
        self.index(name).map(|i| self.params[i])
    }

    pub fn current_state(&self, layer: usize) -> Option<&str> {
        let l = self.layers.get(layer)?;
        Some(&self.data.layers[layer].states[l.state].name)
    }
    pub fn next_state(&self, layer: usize) -> Option<&str> {
        let (n, ..) = self.layers.get(layer)?.next?;
        Some(&self.data.layers[layer].states[n].name)
    }
    pub fn normalized_time(&self, layer: usize) -> f32 {
        self.layers.get(layer).map(|l| l.time).unwrap_or(0.0)
    }
    pub fn is_in_transition(&self, layer: usize) -> bool {
        self.layers.get(layer).is_some_and(|l| l.next.is_some())
    }

    /// Jumps to a state (Animator.Play) — or crossfades to it over `fade` seconds.
    pub fn play(&mut self, state: &str, layer: usize, fade: f32) -> bool {
        let Some(l) = self.data.layers.get(layer) else { return false };
        let Some(i) = l.states.iter().position(|s| s.name == state) else { return false };
        let run = &mut self.layers[layer];
        if fade > 0.0 {
            run.next = Some((i, 0.0, 0.0, fade));
        } else {
            *run = LayerRun { state: i, ..Default::default() };
        }
        true
    }

    fn param_value(&self, name: &str) -> f32 {
        self.get(name).unwrap_or(0.0)
    }

    fn holds(&self, c: &Condition) -> bool {
        let v = self.param_value(&c.parameter);
        match c.mode {
            ConditionMode::If => v != 0.0,
            ConditionMode::IfNot => v == 0.0,
            ConditionMode::Greater => v > c.threshold,
            ConditionMode::Less => v < c.threshold,
            ConditionMode::Equals => v == c.threshold,
            ConditionMode::NotEqual => v != c.threshold,
        }
    }

    /// Blend weights of a state's motion: (clip, weight, clip speed).
    fn motions(&self, s: &State) -> Vec<(String, f32, f32)> {
        if let Some(b) = &s.blend_tree {
            let p = self.param_value(&b.parameter);
            if b.kind == BlendKind::TwoD {
                let py = b.parameter_y.as_deref().map(|n| self.param_value(n)).unwrap_or(0.0);
                let pts: Vec<[f32; 2]> = b.children.iter().map(|c| c.position.unwrap_or([c.threshold, 0.0])).collect();
                let w = gradient_band_weights(&pts, [p, py]);
                return b.children.iter().zip(w).filter(|(_, w)| *w > 1e-4).map(|(c, w)| (c.clip.clone(), w, c.speed)).collect();
            }
            let mut kids: Vec<&BlendChild> = b.children.iter().collect();
            kids.sort_by(|a, b| a.threshold.total_cmp(&b.threshold));
            if kids.is_empty() {
                return Vec::new();
            }
            if p <= kids[0].threshold {
                return vec![(kids[0].clip.clone(), 1.0, kids[0].speed)];
            }
            let last = kids[kids.len() - 1];
            if p >= last.threshold {
                return vec![(last.clip.clone(), 1.0, last.speed)];
            }
            let i = kids.iter().position(|k| k.threshold > p).unwrap_or(kids.len() - 1);
            let (a, b) = (kids[i - 1], kids[i]);
            let f = (p - a.threshold) / (b.threshold - a.threshold).max(1e-6);
            return vec![(a.clip.clone(), 1.0 - f, a.speed), (b.clip.clone(), f, b.speed)];
        }
        s.clip.iter().map(|c| (c.clone(), 1.0, 1.0)).collect()
    }

    /// Seconds per normalized cycle of a state (blend trees: weighted child lengths).
    fn cycle(&self, s: &State, durations: &dyn Fn(&str) -> Option<f32>) -> f32 {
        let d: f32 = self.motions(s).iter().map(|(c, w, speed)| durations(c).unwrap_or(1.0) / speed.abs().max(1e-3) * w).sum();
        d.max(1e-3)
    }

    fn speed(&self, s: &State) -> f32 {
        s.speed * s.speed_parameter.as_deref().map(|p| self.param_value(p)).unwrap_or(1.0)
    }

    fn sample_state(&self, s: &State, norm: f32, weight: f32, durations: &dyn Fn(&str) -> Option<f32>, out: &mut Vec<ClipSample>) {
        let norm = if s.looping { norm.rem_euclid(1.0) } else { norm.clamp(0.0, 1.0) };
        for (clip, w, _) in self.motions(s) {
            let d = durations(&clip).unwrap_or(0.0);
            if w * weight > 0.0 {
                out.push((clip, norm * d, w * weight));
            }
        }
    }

    /// Advances every layer by `dt` and returns, per layer, its weight and the clips to sample.
    /// `durations` gives a clip's length in seconds.
    pub fn update(&mut self, dt: f32, durations: &dyn Fn(&str) -> Option<f32>) -> Vec<(f32, Vec<ClipSample>)> {
        let data = self.data.clone();
        let mut result = Vec::with_capacity(data.layers.len());
        for (li, layer) in data.layers.iter().enumerate() {
            // advance the current (and the incoming) state
            let mut run = self.layers[li].clone();
            let cur = &layer.states[run.state];
            let prev = run.time;
            run.time += dt * self.speed(cur) / self.cycle(cur, durations);
            if let Some((n, t, e, d)) = run.next {
                let ns = &layer.states[n];
                let t = t + dt * self.speed(ns) / self.cycle(ns, durations);
                let e = e + dt;
                run.next = if e >= d { None } else { Some((n, t, e, d)) };
                if e >= d {
                    run.state = n;
                    run.time = t;
                }
            } else {
                // look for a transition: Any State first, then the current state's own
                let fired = layer
                    .transitions
                    .iter()
                    .filter(|t| t.from == ANY_STATE)
                    .chain(layer.transitions.iter().filter(|t| t.from == cur.name))
                    .find(|t| {
                        let to = layer.states.iter().position(|s| s.name == t.to);
                        if t.from == ANY_STATE && to == Some(run.state) && !t.can_transition_to_self {
                            return false;
                        }
                        if !t.has_exit_time && t.conditions.is_empty() {
                            return false;
                        }
                        let exit_ok = !t.has_exit_time || {
                            // loops: the exit time of the current cycle; ≥ 1: once reached
                            let target = if t.exit_time < 1.0 && cur.looping {
                                let k = prev.floor();
                                let mut target = k + t.exit_time;
                                if target < prev {
                                    target += 1.0;
                                }
                                target
                            } else {
                                t.exit_time
                            };
                            run.time >= target
                        };
                        exit_ok && t.conditions.iter().all(|c| self.holds(c))
                    })
                    .cloned();
                if let Some(t) = fired {
                    for c in &t.conditions {
                        if let Some(i) = self.index(&c.parameter) {
                            if data.parameters[i].kind == ParamKind::Trigger {
                                self.params[i] = 0.0;
                            }
                        }
                    }
                    let to = layer.states.iter().position(|s| s.name == t.to).unwrap_or(0);
                    if t.duration > 0.0 {
                        run.next = Some((to, 0.0, 0.0, t.duration));
                    } else {
                        run = LayerRun { state: to, ..Default::default() };
                    }
                }
            }
            // what this layer shows
            let mut clips = Vec::new();
            let cur = &layer.states[run.state];
            match run.next {
                Some((n, t, e, d)) => {
                    let f = (e / d.max(1e-6)).clamp(0.0, 1.0);
                    self.sample_state(cur, run.time, 1.0 - f, durations, &mut clips);
                    self.sample_state(&layer.states[n], t, f, durations, &mut clips);
                }
                None => self.sample_state(cur, run.time, 1.0, durations, &mut clips),
            }
            self.layers[li] = run;
            result.push((if li == 0 { 1.0 } else { layer.weight }, clips));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "parameters": [ { "name": "speed", "type": "float" }, { "name": "jump", "type": "trigger" }, { "name": "grounded", "type": "bool", "default": 1 } ],
      "layers": [ {
        "name": "Base Layer", "default_state": "Move",
        "states": [
          { "name": "Move", "blend_tree": { "parameter": "speed", "children": [ { "clip": "Idle", "threshold": 0 }, { "clip": "Run", "threshold": 1 } ] } },
          { "name": "Jump", "clip": "Hop", "loop": false },
          { "name": "Fall", "clip": "Idle" }
        ],
        "transitions": [
          { "from": "Any State", "to": "Jump", "conditions": [ { "parameter": "jump", "mode": "if" } ], "duration": 0.1 },
          { "from": "Jump", "to": "Move", "has_exit_time": true, "exit_time": 1.0, "duration": 0 },
          { "from": "Move", "to": "Fall", "conditions": [ { "parameter": "grounded", "mode": "ifnot" } ], "duration": 0 }
        ]
      } ]
    }"#;

    fn len(c: &str) -> Option<f32> {
        Some(match c {
            "Idle" => 2.0,
            "Run" => 1.0,
            "Hop" => 0.5,
            _ => return None,
        })
    }

    #[test]
    fn blend_tree_weights_follow_the_parameter() {
        let mut c = ControllerState::new(Arc::new(ControllerData::parse(SAMPLE).unwrap()));
        let out = c.update(0.0, &len);
        assert_eq!(out[0].1, vec![("Idle".to_string(), 0.0, 1.0)]);
        c.set("speed", 0.25);
        let out = c.update(0.0, &len);
        assert_eq!(out[0].1.iter().map(|(n, _, w)| (n.as_str(), *w)).collect::<Vec<_>>(), vec![("Idle", 0.75), ("Run", 0.25)]);
        // synced: a cycle lasts the weighted length (0.75·2 + 0.25·1 = 1.75 s)
        let out = c.update(0.875, &len);
        assert!((out[0].1[0].1 - 1.0).abs() < 1e-4, "Idle at half its length: {:?}", out);
        assert!((out[0].1[1].1 - 0.5).abs() < 1e-4);
    }

    #[test]
    fn triggers_any_state_exit_time_and_bools() {
        let mut c = ControllerState::new(Arc::new(ControllerData::parse(SAMPLE).unwrap()));
        c.update(0.1, &len);
        assert_eq!(c.current_state(0), Some("Move"));
        c.set("jump", 1.0);
        c.update(0.016, &len);
        assert!(c.is_in_transition(0));
        assert_eq!(c.next_state(0), Some("Jump"));
        assert_eq!(c.get("jump"), Some(0.0), "the trigger was consumed");
        c.update(0.1, &len);
        assert_eq!(c.current_state(0), Some("Jump"));
        // Any State doesn't re-enter Jump; exit time 1.0 of a 0.5 s clip returns to Move
        c.update(0.3, &len);
        assert_eq!(c.current_state(0), Some("Jump"));
        c.update(0.3, &len);
        assert_eq!(c.current_state(0), Some("Move"));
        c.set("grounded", 0.0);
        c.update(0.016, &len);
        assert_eq!(c.current_state(0), Some("Fall"));
        assert!(c.play("Jump", 0, 0.0));
        assert_eq!(c.current_state(0), Some("Jump"));
    }

    #[test]
    fn every_sample_controller_parses() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets/animators");
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.to_string_lossy().ends_with(".controller.json") {
                ControllerData::parse(&std::fs::read_to_string(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                n += 1;
            }
        }
        assert!(n > 0);
    }

    #[test]
    fn validation_names_the_broken_reference() {
        let bad = SAMPLE.replace(r#""to": "Fall""#, r#""to": "Nope""#);
        assert!(ControllerData::parse(&bad).unwrap_err().contains("Nope"));
    }

    #[test]
    fn gradient_bands_and_2d_trees() {
        let pts = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]];
        let w = gradient_band_weights(&pts, [0.0, 0.0]);
        assert!((w[0] - 1.0).abs() < 1e-5, "at a point: only it {w:?}");
        let w = gradient_band_weights(&pts, [0.5, 0.0]);
        assert!((w[0] - 0.5).abs() < 1e-4 && (w[1] - 0.5).abs() < 1e-4, "halfway: {w:?}");
        let w = gradient_band_weights(&pts, [3.0, 0.0]);
        assert!((w[1] - 1.0).abs() < 1e-5, "outside: the nearest edge point {w:?}");
        let json = r#"{ "parameters": [ { "name": "x" }, { "name": "y" } ], "layers": [ { "name": "Base", "states": [
            { "name": "Move", "blend_tree": { "type": "2d", "parameter": "x", "parameter_y": "y", "children": [
              { "clip": "Idle", "position": [0, 0] }, { "clip": "Right", "position": [1, 0] }, { "clip": "Fwd", "position": [0, 1] } ] } } ] } ] }"#;
        let mut c = ControllerState::new(Arc::new(ControllerData::parse(json).unwrap()));
        c.set("y", 0.5);
        let out = c.update(0.0, &|_| Some(1.0));
        let names: Vec<(&str, f32)> = out[0].1.iter().map(|(n, _, w)| (n.as_str(), *w)).collect();
        assert_eq!(names.len(), 2);
        assert!(names.iter().all(|(n, w)| *n != "Right" && (w - 0.5).abs() < 1e-4), "{names:?}");
        assert!(ControllerData::parse(&json.replace(r#""parameter_y": "y", "#, "")).unwrap_err().contains("parameter_y"));
    }
}