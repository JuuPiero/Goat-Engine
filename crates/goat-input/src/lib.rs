//! Input actions — Unity's Input System on top of the portable input (`goat_runtime::Input`).
//!
//! An `.inputactions.json` asset holds action maps; each action has bindings to controls:
//!
//! ```json
//! { "maps": [ { "name": "Player", "actions": [
//!     { "name": "Move", "type": "value", "bindings": [
//!         { "path": "<Gamepad>/leftStick" },
//!         { "composite": "2DVector", "up": "<Gamepad>/dpad/up", "down": "<Gamepad>/dpad/down",
//!           "left": "<Gamepad>/dpad/left", "right": "<Gamepad>/dpad/right" } ] },
//!     { "name": "Jump", "type": "button", "bindings": [ { "path": "<Gamepad>/buttonSouth" }, { "path": "<Touchscreen>/press" } ] },
//!     { "name": "Charge", "type": "button", "interactions": "hold(duration=0.5)", "bindings": [ { "path": "<Gamepad>/buttonWest" } ] }
//! ] } ] }
//! ```
//!
//! - Control paths: `<Gamepad>/buttonSouth|buttonEast|buttonNorth|buttonWest` (Nintendo layout:
//!   B / A / X / Y — or the letters `a` `b` `x` `y`), `leftShoulder` `rightShoulder` (L / R),
//!   `leftTrigger` `rightTrigger` (ZL / ZR), `start` `select`, `dpad` (a 2D control),
//!   `dpad/up…`, `leftStick` / `rightStick` (+ `/x` `/y` `/up` `/down` `/left` `/right`),
//!   `<Touchscreen>/press` / `<Pointer>/press`, `<Pointer>/position`. The device prefix is optional.
//! - Composites: `2DVector` (up / down / left / right, normalized), `1DAxis` (negative /
//!   positive).
//! - Action types: `button` (pressed past 0.5), `value` (continuous; performed while non-zero),
//!   `passthrough` (performed on every change).
//! - Interactions (buttons): `press` (default), `hold(duration=0.4)`, `tap(duration=0.2)`.
//! - Processors (action or binding, comma separated): `invert`, `invertVector2`,
//!   `scale(factor=2)`, `scaleVector2(x=1,y=1)`, `normalizeVector2`, `clamp(min=-1,max=1)`,
//!   `axisDeadzone(min=0.125,max=0.925)`, `stickDeadzone(min=0.125,max=0.925)`.
//! - Rebinding: [`InputActions::override_binding`] + [`InputActions::overrides_json`] /
//!   [`InputActions::apply_overrides`] (Unity's binding overrides).
//!
//! [`InputActionsPlugin`] loads the asset and updates every action at the start of each frame
//! (`early_update`), so behaviours, fixed steps and plugins all see the same state.

use std::collections::BTreeMap;

use goat_core::math::Vec2;
use goat_runtime::{Axis, Button, Context, Input, Plugin};
use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------ asset

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionType {
    #[default]
    Button,
    Value,
    PassThrough,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BindingData {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// `2DVector` / `1DAxis`: the parts are the other keys (up / down / left / right, negative / positive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composite: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub processors: String,
    #[serde(flatten)]
    pub parts: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionData {
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: ActionType,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub interactions: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub processors: String,
    #[serde(default)]
    pub bindings: Vec<BindingData>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionMapData {
    pub name: String,
    #[serde(default)]
    pub actions: Vec<ActionData>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InputActionAsset {
    #[serde(default)]
    pub maps: Vec<ActionMapData>,
}

impl InputActionAsset {
    pub fn parse(text: &str) -> Result<Self, String> {
        let a: InputActionAsset = serde_json::from_str(text).map_err(|e| e.to_string())?;
        // every path must name a control
        for m in &a.maps {
            for act in &m.actions {
                for b in &act.bindings {
                    let paths: Vec<&String> = if b.composite.is_some() { b.parts.values().collect() } else { vec![&b.path] };
                    for p in paths {
                        Control::parse(p).ok_or_else(|| format!("{}/{}: unknown control {p}", m.name, act.name))?;
                    }
                    parse_processors(&b.processors).map_err(|e| format!("{}/{}: {e}", m.name, act.name))?;
                }
                parse_processors(&act.processors).map_err(|e| format!("{}/{}: {e}", m.name, act.name))?;
                parse_interaction(&act.interactions).map_err(|e| format!("{}/{}: {e}", m.name, act.name))?;
            }
        }
        Ok(a)
    }
}

// ------------------------------------------------------------------ controls

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Control {
    Button(Button),
    Axis(Axis),
    /// A part of an axis (0..1): (axis, positive side).
    HalfAxis(Axis, bool),
    LeftStick,
    RightStick,
    Dpad,
    PointerPress,
    PointerPosition,
}

impl Control {
    /// `<Gamepad>/buttonSouth`, `leftStick/x`, `<Touchscreen>/press`...
    pub fn parse(path: &str) -> Option<Control> {
        let p = path.trim();
        let p = if p.starts_with('<') { p.split_once('>').map(|(_, r)| r.trim_start_matches('/')).unwrap_or(p) } else { p };
        let p = p.to_ascii_lowercase();
        use Control::*;
        Some(match p.as_str() {
            "a" | "buttoneast" => Button(goat_runtime::Button::A),
            "b" | "buttonsouth" => Button(goat_runtime::Button::B),
            "x" | "buttonnorth" => Button(goat_runtime::Button::X),
            "y" | "buttonwest" => Button(goat_runtime::Button::Y),
            "l" | "leftshoulder" => Button(goat_runtime::Button::L),
            "r" | "rightshoulder" => Button(goat_runtime::Button::R),
            "zl" | "lefttrigger" => Button(goat_runtime::Button::ZL),
            "zr" | "righttrigger" => Button(goat_runtime::Button::ZR),
            "start" => Button(goat_runtime::Button::Start),
            "select" => Button(goat_runtime::Button::Select),
            "dpad/up" | "up" => Button(goat_runtime::Button::Up),
            "dpad/down" | "down" => Button(goat_runtime::Button::Down),
            "dpad/left" | "left" => Button(goat_runtime::Button::Left),
            "dpad/right" | "right" => Button(goat_runtime::Button::Right),
            "dpad" => Dpad,
            "leftstick" => LeftStick,
            "rightstick" => RightStick,
            "leftstick/x" => Axis(goat_runtime::Axis::LeftX),
            "leftstick/y" => Axis(goat_runtime::Axis::LeftY),
            "rightstick/x" => Axis(goat_runtime::Axis::RightX),
            "rightstick/y" => Axis(goat_runtime::Axis::RightY),
            "leftstick/up" => HalfAxis(goat_runtime::Axis::LeftY, true),
            "leftstick/down" => HalfAxis(goat_runtime::Axis::LeftY, false),
            "leftstick/right" => HalfAxis(goat_runtime::Axis::LeftX, true),
            "leftstick/left" => HalfAxis(goat_runtime::Axis::LeftX, false),
            "rightstick/up" => HalfAxis(goat_runtime::Axis::RightY, true),
            "rightstick/down" => HalfAxis(goat_runtime::Axis::RightY, false),
            "rightstick/right" => HalfAxis(goat_runtime::Axis::RightX, true),
            "rightstick/left" => HalfAxis(goat_runtime::Axis::RightX, false),
            "press" | "primarytouch/press" | "leftbutton" => PointerPress,
            "position" | "primarytouch/position" => PointerPosition,
            _ => return None,
        })
    }

    /// The control's value (scalars in `x`).
    pub fn read(&self, input: &Input) -> Vec2 {
        let b = |b: Button| if input.held(b) { 1.0 } else { 0.0 };
        match *self {
            Control::Button(btn) => Vec2::new(b(btn), 0.0),
            Control::Axis(a) => Vec2::new(input.axis(a), 0.0),
            Control::HalfAxis(a, pos) => Vec2::new(if pos { input.axis(a).max(0.0) } else { (-input.axis(a)).max(0.0) }, 0.0),
            Control::LeftStick => input.stick(),
            Control::RightStick => Vec2::new(input.axis(Axis::RightX), input.axis(Axis::RightY)),
            Control::Dpad => Vec2::new(b(Button::Right) - b(Button::Left), b(Button::Up) - b(Button::Down)),
            Control::PointerPress => Vec2::new(if input.pointer_down { 1.0 } else { 0.0 }, 0.0),
            Control::PointerPosition => input.pointer.unwrap_or(Vec2::ZERO),
        }
    }

    fn is_2d(&self) -> bool {
        matches!(self, Control::LeftStick | Control::RightStick | Control::Dpad | Control::PointerPosition)
    }
}

// ------------------------------------------------------------------ processors / interactions

#[derive(Clone, Debug, PartialEq)]
pub enum Processor {
    Invert,
    InvertVector2,
    Scale(f32),
    ScaleVector2(Vec2),
    NormalizeVector2,
    Clamp(f32, f32),
    AxisDeadzone(f32, f32),
    StickDeadzone(f32, f32),
}

/// `name(k=v,k=v)` items separated by commas outside parentheses.
fn items(s: &str) -> Vec<(String, BTreeMap<String, f32>)> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for ch in s.chars().chain(std::iter::once(',')) {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch)
            }
            ')' => {
                depth -= 1;
                cur.push(ch)
            }
            ',' if depth == 0 => {
                let t = cur.trim().to_string();
                cur.clear();
                if t.is_empty() {
                    continue;
                }
                let (name, args) = match t.split_once('(') {
                    Some((n, a)) => (n.trim().to_string(), a.trim_end_matches(')').to_string()),
                    None => (t, String::new()),
                };
                let args = args.split(',').filter_map(|kv| kv.split_once('=')).filter_map(|(k, v)| v.trim().parse().ok().map(|v| (k.trim().to_ascii_lowercase(), v))).collect();
                out.push((name, args));
            }
            _ => cur.push(ch),
        }
    }
    out
}

pub fn parse_processors(s: &str) -> Result<Vec<Processor>, String> {
    items(s)
        .into_iter()
        .map(|(name, a)| {
            let g = |k: &str, d: f32| a.get(k).copied().unwrap_or(d);
            Ok(match name.to_ascii_lowercase().as_str() {
                "invert" => Processor::Invert,
                "invertvector2" => Processor::InvertVector2,
                "scale" => Processor::Scale(g("factor", 1.0)),
                "scalevector2" => Processor::ScaleVector2(Vec2::new(g("x", 1.0), g("y", 1.0))),
                "normalize" | "normalizevector2" => Processor::NormalizeVector2,
                "clamp" => Processor::Clamp(g("min", -1.0), g("max", 1.0)),
                "axisdeadzone" => Processor::AxisDeadzone(g("min", 0.125), g("max", 0.925)),
                "stickdeadzone" => Processor::StickDeadzone(g("min", 0.125), g("max", 0.925)),
                other => return Err(format!("unknown processor {other}")),
            })
        })
        .collect()
}

fn process(v: Vec2, ps: &[Processor]) -> Vec2 {
    let dz = |x: f32, min: f32, max: f32| {
        let a = x.abs();
        if a < min {
            0.0
        } else {
            x.signum() * ((a - min) / (max - min).max(1e-6)).min(1.0)
        }
    };
    ps.iter().fold(v, |v, p| match *p {
        Processor::Invert => Vec2::new(-v.x, v.y),
        Processor::InvertVector2 => -v,
        Processor::Scale(f) => v * f,
        Processor::ScaleVector2(s) => v * s,
        Processor::NormalizeVector2 => v.normalize_or_zero(),
        Processor::Clamp(a, b) => Vec2::new(v.x.clamp(a, b), v.y.clamp(a, b)),
        Processor::AxisDeadzone(min, max) => Vec2::new(dz(v.x, min, max), v.y),
        Processor::StickDeadzone(min, max) => {
            let m = v.length();
            if m < min {
                Vec2::ZERO
            } else {
                v / m * ((m - min) / (max - min).max(1e-6)).min(1.0)
            }
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Interaction {
    Press,
    Hold(f32),
    Tap(f32),
}

pub fn parse_interaction(s: &str) -> Result<Interaction, String> {
    let it = items(s);
    let Some((name, a)) = it.first() else { return Ok(Interaction::Press) };
    Ok(match name.to_ascii_lowercase().as_str() {
        "press" => Interaction::Press,
        "hold" => Interaction::Hold(a.get("duration").copied().unwrap_or(0.4)),
        "tap" => Interaction::Tap(a.get("duration").copied().unwrap_or(0.2)),
        other => return Err(format!("unknown interaction {other}")),
    })
}

// ------------------------------------------------------------------ runtime

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Waiting,
    Started,
    Performed,
    Canceled,
}

#[derive(Clone, Debug)]
enum Source {
    Single(Control),
    Vector2 { up: Option<Control>, down: Option<Control>, left: Option<Control>, right: Option<Control> },
    Axis1D { negative: Option<Control>, positive: Option<Control> },
}

#[derive(Clone, Debug)]
struct Binding {
    source: Source,
    processors: Vec<Processor>,
}

impl Binding {
    fn build(b: &BindingData) -> Binding {
        let part = |k: &str| b.parts.get(k).and_then(|p| Control::parse(p));
        let source = match b.composite.as_deref().map(|c| c.to_ascii_lowercase()) {
            Some(c) if c == "2dvector" || c == "dpad" => Source::Vector2 { up: part("up"), down: part("down"), left: part("left"), right: part("right") },
            Some(c) if c == "1daxis" || c == "axis" => Source::Axis1D { negative: part("negative"), positive: part("positive") },
            _ => Source::Single(Control::parse(&b.path).unwrap_or(Control::Button(Button::A))),
        };
        Binding { source, processors: parse_processors(&b.processors).unwrap_or_default() }
    }
    fn read(&self, input: &Input) -> Vec2 {
        let r = |c: &Option<Control>| c.map(|c| c.read(input).x).unwrap_or(0.0);
        let v = match &self.source {
            Source::Single(c) => c.read(input),
            Source::Vector2 { up, down, left, right } => Vec2::new(r(right) - r(left), r(up) - r(down)).normalize_or_zero(),
            Source::Axis1D { negative, positive } => Vec2::new(r(positive) - r(negative), 0.0),
        };
        process(v, &self.processors)
    }
    fn is_2d(&self) -> bool {
        match &self.source {
            Source::Single(c) => c.is_2d(),
            Source::Vector2 { .. } => true,
            Source::Axis1D { .. } => false,
        }
    }
}

/// One action at runtime.
#[derive(Clone, Debug)]
pub struct InputAction {
    pub name: String,
    pub map: String,
    pub kind: ActionType,
    bindings: Vec<Binding>,
    processors: Vec<Processor>,
    interaction: Interaction,
    pub enabled: bool,
    value: Vec2,
    phase: Phase,
    /// Performed this frame (Unity's `triggered`).
    triggered: bool,
    pressed_now: bool,
    released_now: bool,
    actuated: bool,
    /// Seconds since the current actuation started.
    held_for: f32,
    hold_done: bool,
}

const PRESS_POINT: f32 = 0.5;

impl InputAction {
    fn build(map: &str, a: &ActionData) -> Self {
        InputAction {
            name: a.name.clone(),
            map: map.into(),
            kind: a.kind,
            bindings: a.bindings.iter().map(Binding::build).collect(),
            processors: parse_processors(&a.processors).unwrap_or_default(),
            interaction: parse_interaction(&a.interactions).unwrap_or(Interaction::Press),
            enabled: true,
            value: Vec2::ZERO,
            phase: Phase::Waiting,
            triggered: false,
            pressed_now: false,
            released_now: false,
            actuated: false,
            held_for: 0.0,
            hold_done: false,
        }
    }

    /// `ReadValue<float>()`.
    pub fn read_f32(&self) -> f32 {
        self.value.x
    }
    /// `ReadValue<Vector2>()`.
    pub fn read_vec2(&self) -> Vec2 {
        self.value
    }
    pub fn triggered(&self) -> bool {
        self.triggered
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    /// The control is past the press point (`IsPressed`).
    pub fn is_pressed(&self) -> bool {
        self.actuated
    }
    pub fn was_pressed_this_frame(&self) -> bool {
        self.pressed_now
    }
    pub fn was_released_this_frame(&self) -> bool {
        self.released_now
    }

    fn update(&mut self, input: &Input, dt: f32) {
        self.triggered = false;
        self.pressed_now = false;
        self.released_now = false;
        if !self.enabled {
            self.value = Vec2::ZERO;
            self.phase = Phase::Waiting;
            self.actuated = false;
            return;
        }
        // the most actuated binding wins (Unity's conflict resolution)
        let mut best = Vec2::ZERO;
        let mut two_d = false;
        for b in &self.bindings {
            let v = b.read(input);
            two_d |= b.is_2d();
            if v.length_squared() > best.length_squared() || (b.is_2d() && best == Vec2::ZERO && v != Vec2::ZERO) {
                best = v;
            }
        }
        let v = process(best, &self.processors);
        let prev_value = self.value;
        self.value = v;
        let magnitude = if two_d { v.length() } else { v.x.abs() };
        let was = self.actuated;
        self.actuated = magnitude >= PRESS_POINT || (self.kind != ActionType::Button && magnitude > 0.0);
        self.pressed_now = self.actuated && !was;
        self.released_now = !self.actuated && was;
        if self.actuated {
            self.held_for += dt;
        }
        match self.kind {
            ActionType::Button => match self.interaction {
                Interaction::Press => {
                    if self.pressed_now {
                        self.phase = Phase::Performed;
                        self.triggered = true;
                    } else if self.released_now {
                        self.phase = Phase::Canceled;
                    } else if !self.actuated {
                        self.phase = Phase::Waiting;
                    }
                }
                Interaction::Hold(d) => {
                    if self.pressed_now {
                        self.phase = Phase::Started;
                        self.hold_done = false;
                    }
                    if self.actuated && !self.hold_done && self.held_for >= d {
                        self.hold_done = true;
                        self.phase = Phase::Performed;
                        self.triggered = true;
                    }
                    if self.released_now {
                        self.phase = Phase::Canceled;
                    } else if !self.actuated && !self.released_now {
                        self.phase = Phase::Waiting;
                    }
                }
                Interaction::Tap(d) => {
                    if self.pressed_now {
                        self.phase = Phase::Started;
                    }
                    if self.actuated && self.held_for > d && self.phase == Phase::Started {
                        self.phase = Phase::Canceled;
                    }
                    if self.released_now {
                        if self.phase == Phase::Started {
                            self.phase = Phase::Performed;
                            self.triggered = true;
                        } else {
                            self.phase = Phase::Waiting;
                        }
                    } else if !self.actuated && self.phase != Phase::Started {
                        self.phase = Phase::Waiting;
                    }
                }
            },
            ActionType::Value => {
                if self.actuated {
                    if self.pressed_now {
                        self.phase = Phase::Started;
                    }
                    if v != prev_value || self.pressed_now {
                        self.phase = Phase::Performed;
                        self.triggered = true;
                    }
                } else if self.released_now {
                    self.phase = Phase::Canceled;
                } else {
                    self.phase = Phase::Waiting;
                }
            }
            ActionType::PassThrough => {
                if v != prev_value {
                    self.phase = Phase::Performed;
                    self.triggered = true;
                }
            }
        }
        if !self.actuated {
            self.held_for = 0.0;
        }
    }
}

/// Every action of an asset (a resource): `actions.find("Player/Jump")`.
#[derive(Clone, Debug, Default)]
pub struct InputActions {
    pub asset: InputActionAsset,
    pub key: Option<String>,
    actions: Vec<InputAction>,
    /// `"Map/Action#binding"` → control path.
    overrides: BTreeMap<String, String>,
}

impl InputActions {
    pub fn new(asset: InputActionAsset) -> Self {
        let actions = asset.maps.iter().flat_map(|m| m.actions.iter().map(move |a| InputAction::build(&m.name, a))).collect();
        InputActions { asset, key: None, actions, overrides: BTreeMap::new() }
    }
    pub fn parse(text: &str) -> Result<Self, String> {
        InputActionAsset::parse(text).map(Self::new)
    }

    /// `"Map/Action"` or just `"Action"` (first match).
    pub fn find(&self, name: &str) -> Option<&InputAction> {
        match name.split_once('/') {
            Some((m, a)) => self.actions.iter().find(|x| x.map == m && x.name == a),
            None => self.actions.iter().find(|x| x.name == name),
        }
    }
    pub fn find_mut(&mut self, name: &str) -> Option<&mut InputAction> {
        match name.split_once('/') {
            Some((m, a)) => self.actions.iter_mut().find(|x| x.map == m && x.name == a),
            None => self.actions.iter_mut().find(|x| x.name == name),
        }
    }
    pub fn actions(&self) -> &[InputAction] {
        &self.actions
    }

    pub fn triggered(&self, name: &str) -> bool {
        self.find(name).is_some_and(|a| a.triggered())
    }
    pub fn is_pressed(&self, name: &str) -> bool {
        self.find(name).is_some_and(|a| a.is_pressed())
    }
    pub fn read_f32(&self, name: &str) -> f32 {
        self.find(name).map(|a| a.read_f32()).unwrap_or(0.0)
    }
    pub fn read_vec2(&self, name: &str) -> Vec2 {
        self.find(name).map(|a| a.read_vec2()).unwrap_or(Vec2::ZERO)
    }

    /// Enables / disables every action of a map (`map.Enable()` / `Disable()`).
    pub fn set_map_enabled(&mut self, map: &str, on: bool) {
        for a in self.actions.iter_mut().filter(|a| a.map == map) {
            a.enabled = on;
        }
    }

    /// Replaces binding `index` of an action with another control path (interactive rebinding).
    pub fn override_binding(&mut self, action: &str, index: usize, path: &str) -> Result<(), String> {
        let control = Control::parse(path).ok_or_else(|| format!("unknown control {path}"))?;
        let a = self.find_mut(action).ok_or_else(|| format!("no action {action}"))?;
        let b = a.bindings.get_mut(index).ok_or_else(|| format!("{action} has no binding {index}"))?;
        b.source = Source::Single(control);
        let (map, name) = (a.map.clone(), a.name.clone());
        // keep the data in step so overrides can be saved
        if let Some(d) = self.asset.maps.iter_mut().find(|m| m.name == map).and_then(|m| m.actions.iter_mut().find(|x| x.name == name)) {
            let bd = &mut d.bindings[index];
            bd.composite = None;
            bd.parts.clear();
            bd.path = path.into();
        }
        self.overrides.insert(format!("{map}/{name}#{index}"), path.into());
        Ok(())
    }

    /// The overrides made so far, as JSON (`SaveBindingOverridesAsJson`).
    pub fn overrides_json(&self) -> String {
        serde_json::to_string(&self.overrides).unwrap_or_else(|_| "{}".into())
    }
    /// Re-applies saved overrides (`LoadBindingOverridesFromJson`).
    pub fn apply_overrides(&mut self, json: &str) -> Result<(), String> {
        let o: BTreeMap<String, String> = serde_json::from_str(json).map_err(|e| e.to_string())?;
        for (k, path) in o {
            let (action, index) = k.rsplit_once('#').ok_or_else(|| format!("bad override key {k}"))?;
            self.override_binding(action, index.parse().map_err(|_| format!("bad override key {k}"))?, &path)?;
        }
        Ok(())
    }

    /// Advances every action by one frame.
    pub fn update(&mut self, input: &Input, dt: f32) {
        for a in &mut self.actions {
            a.update(input, dt);
        }
    }
}

/// Loads an `.inputactions.json` and updates it every frame (`InputActions` resource).
pub struct InputActionsPlugin {
    key: String,
}

impl InputActionsPlugin {
    pub fn new(key: &str) -> Self {
        InputActionsPlugin { key: key.into() }
    }
}

impl Plugin for InputActionsPlugin {
    fn early_update(&mut self, ctx: &mut Context) {
        if ctx.resources.get::<InputActions>().is_none() {
            let loaded = ctx
                .resources
                .get::<goat_assets::AssetServer>()
                .ok_or_else(|| "no asset server".to_string())
                .and_then(|a| a.io.read_string(&self.key).map_err(|e| e.to_string()))
                .and_then(|t| InputActions::parse(&t));
            match loaded {
                Ok(mut a) => {
                    a.key = Some(self.key.clone());
                    ctx.resources.insert(a);
                }
                Err(e) => {
                    goat_runtime::warn!("{}: {e}", self.key);
                    ctx.resources.insert(InputActions::default());
                }
            }
        }
        let dt = ctx.time.delta;
        if let Some(a) = ctx.resources.get_mut::<InputActions>() {
            a.update(ctx.input, dt);
        }
    }
}

/// `goat_input::actions(ctx)` — the loaded actions.
pub fn actions<'a>(ctx: &'a Context) -> Option<&'a InputActions> {
    ctx.resources.get::<InputActions>()
}

#[cfg(test)]
mod tests;
