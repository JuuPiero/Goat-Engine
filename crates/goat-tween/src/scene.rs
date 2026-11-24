//! `TweenAnimation` in scene files (DOTweenAnimation): tweens set up in the editor, started when
//! the game plays.
//!
//! ```json
//! "TweenAnimation": { "animation": "Move", "end": [0, 1, 0], "duration": 0.8, "ease": "InOutSine",
//!                     "loops": -1, "loopType": "Yoyo", "relative": true }
//! ```
//! Several on one object: a list. Animations: `Move`, `Scale`, `Rotate`, `PunchPosition`,
//! `PunchScale`, `PunchRotation`, `ShakePosition`, `ShakeScale`, `ShakeRotation`, `Path`
//! (`points`, `closed`, `lookAhead`).

use goat_core::math::Vec3;
use goat_runtime::{Context, Entity};
use goat_scene::{ComponentCodec, LoadContext, SaveContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Ease, LoopType, Prop, Tween, TweenId};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TweenAnimation {
    pub animation: String,
    #[serde(default)]
    pub end: [f32; 3],
    #[serde(default = "one")]
    pub duration: f32,
    #[serde(default)]
    pub delay: f32,
    #[serde(default = "out_quad")]
    pub ease: String,
    #[serde(default = "one_i")]
    pub loops: i32,
    #[serde(default = "restart")]
    pub loop_type: String,
    #[serde(default)]
    pub relative: bool,
    /// `.From()`: from `end` to where the object is.
    #[serde(default)]
    pub from: bool,
    #[serde(default = "yes")]
    pub autoplay: bool,
    /// DOTween id (kill / pause by tag from scripts).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default = "ten")]
    pub vibrato: u32,
    #[serde(default = "one")]
    pub elasticity: f32,
    #[serde(default = "ninety")]
    pub randomness: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<[f32; 3]>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub look_ahead: bool,
    #[serde(skip)]
    pub started: bool,
    #[serde(skip)]
    pub tween: Option<TweenId>,
}

fn one() -> f32 {
    1.0
}
fn one_i() -> i32 {
    1
}
fn ten() -> u32 {
    10
}
fn ninety() -> f32 {
    90.0
}
fn yes() -> bool {
    true
}
fn out_quad() -> String {
    "OutQuad".into()
}
fn restart() -> String {
    "Restart".into()
}

impl TweenAnimation {
    /// The tween this animation plays on `e`.
    pub fn build(&self, e: Entity) -> Result<Tween, String> {
        let end = Vec3::from(self.end);
        let d = self.duration.max(0.0);
        let t = match self.animation.as_str() {
            "Move" | "LocalMove" => Tween::move_to(e, end, d),
            "Scale" => Tween::scale_to(e, end, d),
            "Rotate" | "LocalRotate" => Tween::rotate_to(e, end, d),
            "PunchPosition" => Tween::punch(e, Prop::Position, end, d, self.vibrato, self.elasticity),
            "PunchScale" => Tween::punch(e, Prop::Scale, end, d, self.vibrato, self.elasticity),
            "PunchRotation" => Tween::punch(e, Prop::Euler, end, d, self.vibrato, self.elasticity),
            "ShakePosition" => Tween::shake(e, Prop::Position, d, end, self.vibrato, self.randomness),
            "ShakeScale" => Tween::shake(e, Prop::Scale, d, end, self.vibrato, self.randomness),
            "ShakeRotation" => Tween::shake(e, Prop::Euler, d, end, self.vibrato, self.randomness),
            "Path" => {
                let t = Tween::path(e, self.points.iter().map(|p| Vec3::from(*p)).collect(), d, self.closed);
                if self.look_ahead {
                    t.look_ahead()
                } else {
                    t
                }
            }
            other => return Err(format!("TweenAnimation: unknown animation {other}")),
        };
        let mut t = t.delay(self.delay).loops(self.loops, LoopType::parse(&self.loop_type).unwrap_or_default()).relative(self.relative);
        if !matches!(self.animation.as_str(), "Path") && !self.animation.starts_with("Punch") && !self.animation.starts_with("Shake") {
            t = t.ease(Ease::parse(&self.ease).unwrap_or_default());
        }
        if self.from {
            t = t.reversed();
        }
        if !self.id.is_empty() {
            t = t.tag(&self.id);
        }
        Ok(t)
    }
}

/// The TweenAnimations of an entity.
#[derive(Clone, Debug, Default)]
pub struct TweenAnimations(pub Vec<TweenAnimation>);

/// Starts the autoplay animations not started yet (TweenPlugin, every frame in Play).
pub fn start_animations(ctx: &mut Context) {
    let mut todo: Vec<(Entity, usize, Tween)> = Vec::new();
    for (e, a) in ctx.world.query::<&mut TweenAnimations>().iter() {
        for (i, anim) in a.0.iter_mut().enumerate() {
            if anim.autoplay && !anim.started {
                anim.started = true;
                match anim.build(e) {
                    Ok(t) => todo.push((e, i, t)),
                    Err(err) => goat_core::warn!("{err}"),
                }
            }
        }
    }
    for (e, i, t) in todo {
        let id = crate::tweens(ctx).add(t);
        if let Ok(mut a) = ctx.world.get::<&mut TweenAnimations>(e) {
            if let Some(anim) = a.0.get_mut(i) {
                anim.tween = Some(id);
            }
        }
    }
}

fn save(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let a = ctx.world.get::<&TweenAnimations>(e).ok()?;
    if a.0.len() == 1 {
        serde_json::to_value(&a.0[0]).ok()
    } else {
        serde_json::to_value(&a.0).ok()
    }
}

fn load(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let list: Vec<TweenAnimation> = match v {
        Value::Array(_) => serde_json::from_value(v.clone()).map_err(|e| e.to_string())?,
        _ => vec![serde_json::from_value(v.clone()).map_err(|e| e.to_string())?],
    };
    ctx.world.insert_one(e, TweenAnimations(list)).map_err(|_| "entity vanished".to_string())
}

pub const CODEC: ComponentCodec = ComponentCodec { name: "TweenAnimation", save, load, remove: goat_scene::remove_typed::<TweenAnimations> };
