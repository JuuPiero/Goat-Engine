//! GoatEngine tweens (DOTween-like): animate a Transform or any value over time with an ease,
//! loops (restart / yoyo / incremental), delays, punch / shake effects, paths, and sequences
//! (append / join / insert / intervals / callbacks).
//!
//! ```ignore
//! let tweens = goat_tween::tweens(ctx);
//! tweens.add(Tween::move_to(coin, vec3(0.0, 2.0, 0.0), 0.6).ease(Ease::OutBack).loops(-1, LoopType::Yoyo));
//! let seq = Sequence::new()
//!     .append(Tween::scale_to(e, Vec3::splat(1.3), 0.15))
//!     .append(Tween::scale_to(e, Vec3::ONE, 0.3).ease(Ease::OutBounce))
//!     .append_callback(|ctx| goat_core::info!("done"));
//! tweens.add(seq.build());
//! ```
//!
//! [`TweenPlugin`] runs them after the behaviours' `update`. A tween whose target entity is gone
//! is killed (DOTween's safe mode). Features: `scene` = the [`TweenAnimation`] component
//! (DOTweenAnimation) in scene files, `script` = `Tween.*` calls for TS / C# / C scripts.

pub mod ease;
#[cfg(feature = "scene")]
pub mod scene;
#[cfg(feature = "script")]
pub mod script;

use std::collections::HashMap;

use goat_core::math::{EulerRot, Quat, Vec3, Vec4};
use goat_runtime::{Context, Entity, Plugin, Transform, World};
use serde::{Deserialize, Serialize};

pub use ease::Ease;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TweenId(pub u32);

/// What happens when a loop ends (DOTween's LoopType).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopType {
    /// Starts over from the start value.
    #[default]
    Restart,
    /// Plays backwards every other loop.
    Yoyo,
    /// Each loop continues from where the previous one ended.
    Incremental,
}

impl LoopType {
    pub fn parse(s: &str) -> Option<LoopType> {
        match s.to_ascii_lowercase().as_str() {
            "restart" => Some(LoopType::Restart),
            "yoyo" => Some(LoopType::Yoyo),
            "incremental" => Some(LoopType::Incremental),
            _ => None,
        }
    }
}

/// Code run by a tween (start / step / complete, sequence callbacks).
pub enum Callback {
    Rust(Box<dyn FnMut(&mut Context)>),
    /// Reported through [`Tweens::take_events`] (how scripts get their callbacks).
    Event(u64),
}

impl Callback {
    pub fn new(f: impl FnMut(&mut Context) + 'static) -> Self {
        Callback::Rust(Box::new(f))
    }
}

// ------------------------------------------------------------------ properties

type Getter = Box<dyn Fn(&World, Entity) -> Option<Vec4>>;
type Setter = Box<dyn FnMut(&mut World, Entity, Vec4)>;

/// What a tween animates.
pub enum Prop {
    /// Transform position (local).
    Position,
    Scale,
    /// Euler angles in degrees, interpolated as numbers (so 0 → 360 spins once).
    Euler,
    /// Rotation, interpolated along the shortest arc (value = quaternion xyzw).
    Rotation,
    /// A value read back with [`Tweens::value`] (DOTween.To).
    Value,
    /// Anything: a getter and a setter (sprite color, UI alpha, audio volume...).
    Custom { get: Getter, set: Setter },
}

fn euler_of(q: Quat) -> Vec3 {
    let (y, x, z) = q.to_euler(EulerRot::YXZ);
    Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
}
fn quat_of(d: Vec3) -> Quat {
    Quat::from_euler(EulerRot::YXZ, d.y.to_radians(), d.x.to_radians(), d.z.to_radians())
}

impl Prop {
    fn get(&self, world: &World, e: Option<Entity>) -> Option<Vec4> {
        let t = || e.and_then(|e| world.get::<&Transform>(e).ok().map(|t| *t));
        match self {
            Prop::Position => t().map(|t| t.position.extend(0.0)),
            Prop::Scale => t().map(|t| t.scale.extend(0.0)),
            Prop::Euler => t().map(|t| euler_of(t.rotation).extend(0.0)),
            Prop::Rotation => t().map(|t| Vec4::from(t.rotation)),
            Prop::Value => Some(Vec4::ZERO),
            Prop::Custom { get, .. } => e.and_then(|e| get(world, e)),
        }
    }
    /// False when the target is gone.
    fn set(&mut self, world: &mut World, e: Option<Entity>, v: Vec4) -> bool {
        let Some(e) = e else { return matches!(self, Prop::Value) };
        if let Prop::Custom { set, .. } = self {
            if !world.contains(e) {
                return false;
            }
            set(world, e, v);
            return true;
        }
        let Ok(mut t) = world.get::<&mut Transform>(e) else { return false };
        match self {
            Prop::Position => t.position = v.truncate(),
            Prop::Scale => t.scale = v.truncate(),
            Prop::Euler => t.rotation = quat_of(v.truncate()),
            Prop::Rotation => t.rotation = Quat::from_vec4(v).normalize(),
            _ => {}
        }
        true
    }
}

/// How a leaf tween computes its value.
pub enum Kind {
    /// From the start value to `to`.
    To(Vec4),
    /// Springs out by `amount` and back (DOPunch*): `vibrato` oscillations, `elasticity` 0..1 =
    /// how far it swings past the start.
    Punch { amount: Vec4, vibrato: u32, elasticity: f32 },
    /// Random offsets up to `strength`, `vibrato` per second-ish, fading out (DOShake*).
    Shake { strength: Vec4, vibrato: u32, randomness: f32, fade_out: bool, seed: u32 },
    /// Along a Catmull-Rom curve through the points (DOPath); the start value is the first point.
    Path { points: Vec<Vec3>, closed: bool, look_ahead: bool, table: Vec<f32> },
}

struct Leaf {
    entity: Option<Entity>,
    prop: Prop,
    kind: Kind,
    /// Explicit start value (`from(...)`); otherwise the value when the tween starts.
    from: Option<Vec4>,
    /// DOTween's `.From()`: animate from `to` back to the current value.
    is_from: bool,
    relative: bool,
    /// (start, end), captured at the first apply.
    span: Option<(Vec4, Vec4)>,
    value: Vec4,
}

enum Node {
    Leaf(Box<Leaf>),
    Seq(Seq),
}

struct Child {
    start: f32,
    tween: Tween,
}

#[derive(Default)]
struct Seq {
    children: Vec<Child>,
    callbacks: Vec<(f32, Callback)>,
    last: Option<(i64, f32)>,
}

// ------------------------------------------------------------------ the tween

/// One tween (a value / Transform animation, or a sequence of tweens).
pub struct Tween {
    node: Node,
    duration: f32,
    delay: f32,
    /// -1 = forever.
    loops: i32,
    loop_type: LoopType,
    ease: Ease,
    time: f32,
    time_scale: f32,
    ignore_time_scale: bool,
    paused: bool,
    started: bool,
    completed: bool,
    autokill: bool,
    last_loop: i64,
    /// DOTween's SetId: kill / pause by tag.
    pub tag: Option<String>,
    on_start: Vec<Callback>,
    on_step_complete: Vec<Callback>,
    on_complete: Vec<Callback>,
    on_kill: Vec<Callback>,
}

impl Tween {
    fn leaf(entity: Option<Entity>, prop: Prop, kind: Kind, duration: f32) -> Self {
        let node = Node::Leaf(Box::new(Leaf { entity, prop, kind, from: None, is_from: false, relative: false, span: None, value: Vec4::ZERO }));
        Tween {
            node,
            duration: duration.max(0.0),
            delay: 0.0,
            loops: 1,
            loop_type: LoopType::Restart,
            ease: Ease::OutQuad,
            time: 0.0,
            time_scale: 1.0,
            ignore_time_scale: false,
            paused: false,
            started: false,
            completed: false,
            autokill: true,
            last_loop: 0,
            tag: None,
            on_start: Vec::new(),
            on_step_complete: Vec::new(),
            on_complete: Vec::new(),
            on_kill: Vec::new(),
        }
    }

    /// Any property to `to`.
    pub fn to(entity: Entity, prop: Prop, to: Vec4, duration: f32) -> Self {
        Self::leaf(Some(entity), prop, Kind::To(to), duration)
    }
    /// DOMove.
    pub fn move_to(e: Entity, to: Vec3, duration: f32) -> Self {
        Self::to(e, Prop::Position, to.extend(0.0), duration)
    }
    /// DOScale.
    pub fn scale_to(e: Entity, to: Vec3, duration: f32) -> Self {
        Self::to(e, Prop::Scale, to.extend(0.0), duration)
    }
    /// DORotate (Euler degrees; 0 → 360 turns once).
    pub fn rotate_to(e: Entity, euler: Vec3, duration: f32) -> Self {
        Self::to(e, Prop::Euler, euler.extend(0.0), duration)
    }
    /// DORotateQuaternion (shortest arc).
    pub fn rotate_quat(e: Entity, to: Quat, duration: f32) -> Self {
        Self::to(e, Prop::Rotation, Vec4::from(to), duration)
    }
    /// DOTween.To: a value from `from` to `to`, read with [`Tweens::value`].
    pub fn value(from: Vec4, to: Vec4, duration: f32) -> Self {
        Self::leaf(None, Prop::Value, Kind::To(to), duration).from(from)
    }
    /// DOPunchPosition / Scale / Rotation (prop = Position / Scale / Euler).
    pub fn punch(e: Entity, prop: Prop, amount: Vec3, duration: f32, vibrato: u32, elasticity: f32) -> Self {
        Self::leaf(Some(e), prop, Kind::Punch { amount: amount.extend(0.0), vibrato: vibrato.max(1), elasticity: elasticity.clamp(0.0, 1.0) }, duration).ease(Ease::Linear)
    }
    /// DOShakePosition / Scale / Rotation.
    pub fn shake(e: Entity, prop: Prop, duration: f32, strength: Vec3, vibrato: u32, randomness: f32) -> Self {
        let seed = e.id().wrapping_mul(2654435761) ^ (duration.to_bits());
        Self::leaf(Some(e), prop, Kind::Shake { strength: strength.extend(0.0), vibrato: vibrato.max(1), randomness, fade_out: true, seed }, duration).ease(Ease::Linear)
    }
    /// DOPath: through `points` (Catmull-Rom), from the current position.
    pub fn path(e: Entity, points: Vec<Vec3>, duration: f32, closed: bool) -> Self {
        Self::leaf(Some(e), Prop::Position, Kind::Path { points, closed, look_ahead: false, table: Vec::new() }, duration).ease(Ease::Linear)
    }
    /// Any property through a getter / setter (sprite colors, volumes...).
    pub fn custom(e: Entity, get: impl Fn(&World, Entity) -> Option<Vec4> + 'static, set: impl FnMut(&mut World, Entity, Vec4) + 'static, to: Vec4, duration: f32) -> Self {
        Self::to(e, Prop::Custom { get: Box::new(get), set: Box::new(set) }, to, duration)
    }

    // settings (chainable)
    pub fn ease(mut self, e: Ease) -> Self {
        self.ease = e;
        self
    }
    pub fn delay(mut self, s: f32) -> Self {
        self.delay = s.max(0.0);
        self
    }
    /// `-1` = forever.
    pub fn loops(mut self, n: i32, kind: LoopType) -> Self {
        self.loops = if n < 0 { -1 } else { n.max(1) };
        self.loop_type = kind;
        self
    }
    /// `to` is an offset from the start value (DOTween's SetRelative).
    pub fn relative(mut self, yes: bool) -> Self {
        if let Node::Leaf(l) = &mut self.node {
            l.relative = yes;
        }
        self
    }
    /// An explicit start value.
    pub fn from(mut self, v: Vec4) -> Self {
        if let Node::Leaf(l) = &mut self.node {
            l.from = Some(v);
        }
        self
    }
    /// DOTween's `.From()`: animates from the target value to the current one.
    pub fn reversed(mut self) -> Self {
        if let Node::Leaf(l) = &mut self.node {
            l.is_from = true;
        }
        self
    }
    /// The object turns to face along the path (DOPath's SetLookAt).
    pub fn look_ahead(mut self) -> Self {
        if let Node::Leaf(l) = &mut self.node {
            if let Kind::Path { look_ahead, .. } = &mut l.kind {
                *look_ahead = true;
            }
        }
        self
    }
    pub fn speed(mut self, scale: f32) -> Self {
        self.time_scale = scale.max(0.0);
        self
    }
    /// Runs on real time (unaffected by `Time::time_scale`).
    pub fn unscaled(mut self) -> Self {
        self.ignore_time_scale = true;
        self
    }
    pub fn tag(mut self, tag: &str) -> Self {
        self.tag = Some(tag.into());
        self
    }
    /// Keep it after completion (to restart / rewind it later).
    pub fn keep(mut self) -> Self {
        self.autokill = false;
        self
    }
    pub fn paused(mut self) -> Self {
        self.paused = true;
        self
    }
    pub fn on_start(mut self, f: impl FnMut(&mut Context) + 'static) -> Self {
        self.on_start.push(Callback::new(f));
        self
    }
    pub fn on_step_complete(mut self, f: impl FnMut(&mut Context) + 'static) -> Self {
        self.on_step_complete.push(Callback::new(f));
        self
    }
    pub fn on_complete(mut self, f: impl FnMut(&mut Context) + 'static) -> Self {
        self.on_complete.push(Callback::new(f));
        self
    }
    pub fn on_kill(mut self, f: impl FnMut(&mut Context) + 'static) -> Self {
        self.on_kill.push(Callback::new(f));
        self
    }

    /// Length of one loop (seconds).
    pub fn duration(&self) -> f32 {
        self.duration
    }
    /// Delay + every loop; infinite for endless loops.
    pub fn full_duration(&self) -> f32 {
        if self.loops < 0 {
            f32::INFINITY
        } else {
            self.delay + self.duration * self.loops as f32
        }
    }
    pub fn is_complete(&self) -> bool {
        self.completed
    }
    pub fn elapsed(&self) -> f32 {
        self.time
    }

    fn target(&self) -> Option<Entity> {
        match &self.node {
            Node::Leaf(l) => l.entity,
            Node::Seq(_) => None,
        }
    }
    fn targets(&self, e: Entity) -> bool {
        match &self.node {
            Node::Leaf(l) => l.entity == Some(e),
            Node::Seq(s) => s.children.iter().any(|c| c.tween.targets(e)),
        }
    }

    /// Moves to `t` seconds (delay included) and applies. Returns false when a target is gone.
    fn goto(&mut self, t: f32, run: &mut Run) -> bool {
        self.time = t;
        let mut lt = t - self.delay;
        if lt < 0.0 {
            if !self.started {
                return true;
            }
            lt = 0.0;
        }
        if !self.started {
            self.started = true;
            fire(&mut self.on_start, run);
        }
        let d = self.duration.max(1e-6);
        let (lp, mut local, done) = if self.loops > 0 && lt >= d * self.loops as f32 {
            ((self.loops - 1) as i64, d, true)
        } else {
            let lp = (lt / d).floor() as i64;
            (lp, lt - lp as f32 * d, false)
        };
        let back = self.loop_type == LoopType::Yoyo && lp % 2 == 1;
        if back {
            local = d - local;
        }
        let p = if self.duration <= 0.0 { 1.0 } else { self.ease.eval(local / d) };
        let incremental = if self.loop_type == LoopType::Incremental { lp as f32 } else { 0.0 };
        let alive = match &mut self.node {
            Node::Leaf(l) => apply_leaf(l, p, incremental, run.ctx),
            Node::Seq(s) => apply_seq(s, p * d, lp, run),
        };
        if lp > self.last_loop {
            for _ in self.last_loop..lp {
                fire(&mut self.on_step_complete, run);
            }
            self.last_loop = lp;
        }
        if done && !self.completed {
            self.completed = true;
            fire(&mut self.on_step_complete, run);
            fire(&mut self.on_complete, run);
        } else if !done {
            self.completed = false;
        }
        alive
    }
}

fn fire(list: &mut [Callback], run: &mut Run) {
    for c in list.iter_mut() {
        match c {
            Callback::Rust(f) => f(run.ctx),
            Callback::Event(k) => run.events.push(*k),
        }
    }
}

struct Run<'a, 'b, 'c> {
    ctx: &'a mut Context<'b>,
    events: &'c mut Vec<u64>,
}

fn hash(mut x: u32) -> f32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^= x >> 16;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

fn shake_offset(strength: Vec4, vibrato: u32, randomness: f32, fade: bool, seed: u32, p: f32) -> Vec4 {
    if p >= 1.0 {
        return Vec4::ZERO;
    }
    let steps = vibrato.max(1) as f32;
    let x = p * steps;
    let k = x.floor() as u32;
    let f = x - k as f32;
    let dir = |k: u32| {
        let r = randomness.clamp(0.0, 180.0) / 180.0;
        // alternate sides, randomised by `randomness`
        let side = if k % 2 == 0 { 1.0 } else { -1.0 };
        Vec4::new(side * (1.0 - r) + hash(seed ^ k.wrapping_mul(3)) * r, -side * (1.0 - r) * 0.5 + hash(seed ^ k.wrapping_mul(5) ^ 77) * r, hash(seed ^ k.wrapping_mul(7) ^ 991) * r.max(0.3), 0.0)
    };
    let a = if k == 0 { Vec4::ZERO } else { dir(k) };
    let b = dir(k + 1);
    let s = f * f * (3.0 - 2.0 * f);
    let o = (a + (b - a) * s) * strength;
    if fade {
        o * (1.0 - p)
    } else {
        o
    }
}

fn punch_offset(amount: Vec4, vibrato: u32, elasticity: f32, p: f32) -> Vec4 {
    if p <= 0.0 || p >= 1.0 {
        return Vec4::ZERO;
    }
    // first quarter-swing out to `amount`, then damped oscillation; swings past the start
    // are scaled by elasticity
    let w = std::f32::consts::PI * (vibrato as f32 + 0.5);
    let s = (p * w).sin();
    let s = if s < 0.0 { s * elasticity } else { s };
    amount * s * (1.0 - p)
}

fn catmull(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1) + (-p0 + p2) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2 + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// A point on the path at segment-parameter `u` (0..segments).
fn path_point(pts: &[Vec3], closed: bool, u: f32) -> Vec3 {
    let n = pts.len();
    if n == 1 {
        return pts[0];
    }
    let segs = if closed { n } else { n - 1 };
    let u = u.clamp(0.0, segs as f32);
    let i = (u.floor() as usize).min(segs - 1);
    let t = u - i as f32;
    let at = |k: isize| -> Vec3 {
        if closed {
            pts[k.rem_euclid(n as isize) as usize]
        } else if k < 0 {
            pts[0] * 2.0 - pts[1]
        } else if k as usize >= n {
            pts[n - 1] * 2.0 - pts[n - 2]
        } else {
            pts[k as usize]
        }
    };
    let i = i as isize;
    catmull(at(i - 1), at(i), at(i + 1), at(i + 2), t)
}

const PATH_SAMPLES: usize = 64;

/// Arc-length table: cumulative lengths at `segs * PATH_SAMPLES` steps.
fn path_table(pts: &[Vec3], closed: bool) -> Vec<f32> {
    let segs = if closed { pts.len() } else { pts.len().saturating_sub(1) }.max(1);
    let n = segs * PATH_SAMPLES;
    let mut t = Vec::with_capacity(n + 1);
    t.push(0.0);
    let mut prev = path_point(pts, closed, 0.0);
    let mut acc = 0.0;
    for i in 1..=n {
        let p = path_point(pts, closed, i as f32 / PATH_SAMPLES as f32);
        acc += (p - prev).length();
        t.push(acc);
        prev = p;
    }
    t
}

/// The point at fraction `p` of the path's length (constant speed).
fn path_at(pts: &[Vec3], closed: bool, table: &[f32], p: f32) -> Vec3 {
    let total = *table.last().unwrap_or(&0.0);
    if total <= 0.0 {
        return pts[0];
    }
    let want = p.clamp(0.0, 1.0) * total;
    let i = table.partition_point(|&l| l < want).clamp(1, table.len() - 1);
    let (a, b) = (table[i - 1], table[i]);
    let f = if b > a { (want - a) / (b - a) } else { 0.0 };
    path_point(pts, closed, (i as f32 - 1.0 + f) / PATH_SAMPLES as f32)
}

fn apply_leaf(l: &mut Leaf, p: f32, incremental: f32, ctx: &mut Context) -> bool {
    if l.span.is_none() {
        let Some(cur) = l.prop.get(ctx.world, l.entity) else { return false };
        let start = l.from.unwrap_or(cur);
        let span = match &mut l.kind {
            Kind::To(to) => {
                let mut end = if l.relative { start + *to } else { *to };
                if matches!(l.prop, Prop::Rotation) && Quat::from_vec4(start).dot(Quat::from_vec4(end)) < 0.0 {
                    end = -end; // shortest arc
                }
                if l.is_from {
                    (end, start)
                } else {
                    (start, end)
                }
            }
            Kind::Path { points, closed, table, .. } => {
                points.insert(0, start.truncate());
                *table = path_table(points, *closed);
                (start, start)
            }
            _ => (start, start),
        };
        l.span = Some(span);
    }
    let (a, b) = l.span.unwrap();
    let mut look: Option<Vec3> = None;
    let v = match &l.kind {
        Kind::To(_) => {
            if matches!(l.prop, Prop::Rotation) {
                Vec4::from(Quat::from_vec4(a).slerp(Quat::from_vec4(b), p))
            } else {
                a + (b - a) * (p + incremental)
            }
        }
        Kind::Punch { amount, vibrato, elasticity } => a + punch_offset(*amount, *vibrato, *elasticity, p),
        Kind::Shake { strength, vibrato, randomness, fade_out, seed } => a + shake_offset(*strength, *vibrato, *randomness, *fade_out, *seed, p),
        Kind::Path { points, closed, look_ahead, table } => {
            let pos = path_at(points, *closed, table, p);
            if *look_ahead {
                let ahead = path_at(points, *closed, table, (p + 0.01).min(1.0));
                let behind = path_at(points, *closed, table, (p - 0.01).max(0.0));
                look = Some(ahead - behind);
            }
            pos.extend(0.0)
        }
    };
    l.value = v;
    let ok = l.prop.set(ctx.world, l.entity, v);
    if let (Some(dir), Some(e)) = (look, l.entity) {
        if dir.length_squared() > 1e-8 {
            if let Ok(mut t) = ctx.world.get::<&mut Transform>(e) {
                let target = t.position + dir;
                t.look_at(target, Vec3::Y);
            }
        }
    }
    ok
}

fn apply_seq(s: &mut Seq, tt: f32, lp: i64, run: &mut Run) -> bool {
    let mut alive = true;
    for c in s.children.iter_mut() {
        if tt >= c.start || c.tween.started {
            alive &= c.tween.goto((tt - c.start).max(0.0), run);
        }
    }
    // callbacks crossed since the last apply
    let (last_lp, last_tt) = s.last.unwrap_or((lp, -1.0));
    for (at, cb) in s.callbacks.iter_mut() {
        let crossed = if lp == last_lp { (*at > last_tt && *at <= tt) || (*at < last_tt && *at >= tt) } else { *at > last_tt || *at <= tt };
        if crossed {
            fire(std::slice::from_mut(cb), run);
        }
    }
    s.last = Some((lp, tt));
    alive
}

// ------------------------------------------------------------------ sequences

/// Builds a sequence tween (DOTween.Sequence).
#[derive(Default)]
pub struct Sequence {
    seq: Seq,
    cursor: f32,
    last_start: f32,
}

impl Sequence {
    pub fn new() -> Self {
        Self::default()
    }
    fn put(&mut self, start: f32, mut t: Tween) {
        t.autokill = false;
        let end = start + t.full_duration().min(1e6);
        self.cursor = self.cursor.max(end);
        self.last_start = start;
        self.seq.children.push(Child { start, tween: t });
    }
    /// After everything added so far.
    pub fn append(mut self, t: Tween) -> Self {
        let at = self.cursor;
        self.put(at, t);
        self
    }
    /// At the same time as the previous append / insert.
    pub fn join(mut self, t: Tween) -> Self {
        let at = self.last_start;
        self.put(at, t);
        self
    }
    /// At `time` seconds.
    pub fn insert(mut self, time: f32, t: Tween) -> Self {
        self.put(time.max(0.0), t);
        self
    }
    pub fn append_interval(mut self, seconds: f32) -> Self {
        self.cursor += seconds.max(0.0);
        self
    }
    pub fn append_callback(mut self, f: impl FnMut(&mut Context) + 'static) -> Self {
        self.seq.callbacks.push((self.cursor, Callback::new(f)));
        self
    }
    pub fn insert_callback(mut self, time: f32, f: impl FnMut(&mut Context) + 'static) -> Self {
        self.seq.callbacks.push((time.max(0.0), Callback::new(f)));
        self
    }

    /// The sequence as a tween (Linear ease over its timeline; chain loops / delay / callbacks).
    pub fn build(self) -> Tween {
        let mut t = Tween::leaf(None, Prop::Value, Kind::To(Vec4::ZERO), self.cursor);
        t.node = Node::Seq(self.seq);
        t.ease = Ease::Linear;
        t
    }
}

// ------------------------------------------------------------------ the manager

/// Every running tween (a resource; DOTween's static API).
pub struct Tweens {
    list: Vec<(TweenId, Tween)>,
    next: u32,
    /// Global speed (DOTween.timeScale).
    pub time_scale: f32,
    events: Vec<u64>,
    /// Kill requests (id, complete first).
    kills: Vec<(TweenId, bool)>,
    values: HashMap<TweenId, Vec4>,
}

impl Default for Tweens {
    fn default() -> Self {
        Tweens { list: Vec::new(), next: 0, time_scale: 1.0, events: Vec::new(), kills: Vec::new(), values: HashMap::new() }
    }
}

impl Tweens {
    pub fn add(&mut self, t: Tween) -> TweenId {
        self.next += 1;
        let id = TweenId(self.next);
        self.list.push((id, t));
        id
    }
    pub fn get(&self, id: TweenId) -> Option<&Tween> {
        self.list.iter().find(|(i, _)| *i == id).map(|(_, t)| t)
    }
    /// Change settings of a running tween (scripts chain `.setEase(...)` after creating it).
    pub fn get_mut(&mut self, id: TweenId) -> Option<&mut Tween> {
        self.list.iter_mut().find(|(i, _)| *i == id).map(|(_, t)| t)
    }
    /// Takes a tween out (to nest it in a sequence).
    pub fn take(&mut self, id: TweenId) -> Option<Tween> {
        let i = self.list.iter().position(|(x, _)| *x == id)?;
        Some(self.list.remove(i).1)
    }
    pub fn is_active(&self, id: TweenId) -> bool {
        self.list.iter().any(|(i, _)| *i == id) && !self.kills.iter().any(|(k, _)| *k == id)
    }
    pub fn is_playing(&self, id: TweenId) -> bool {
        self.get(id).is_some_and(|t| !t.paused && !t.completed)
    }
    /// Kills a tween (after jumping to its end with `complete`).
    pub fn kill(&mut self, id: TweenId, complete: bool) {
        self.kills.push((id, complete));
    }
    /// Kills every tween animating this entity (DOKill).
    pub fn kill_target(&mut self, e: Entity, complete: bool) {
        let ids: Vec<TweenId> = self.list.iter().filter(|(_, t)| t.targets(e)).map(|(i, _)| *i).collect();
        self.kills.extend(ids.into_iter().map(|i| (i, complete)));
    }
    pub fn kill_tag(&mut self, tag: &str, complete: bool) {
        let ids: Vec<TweenId> = self.list.iter().filter(|(_, t)| t.tag.as_deref() == Some(tag)).map(|(i, _)| *i).collect();
        self.kills.extend(ids.into_iter().map(|i| (i, complete)));
    }
    pub fn kill_all(&mut self) {
        let ids: Vec<TweenId> = self.list.iter().map(|(i, _)| *i).collect();
        self.kills.extend(ids.into_iter().map(|i| (i, false)));
    }
    pub fn pause(&mut self, id: TweenId) {
        if let Some(t) = self.get_mut(id) {
            t.paused = true;
        }
    }
    pub fn play(&mut self, id: TweenId) {
        if let Some(t) = self.get_mut(id) {
            t.paused = false;
        }
    }
    pub fn toggle_pause(&mut self, id: TweenId) {
        if let Some(t) = self.get_mut(id) {
            t.paused = !t.paused;
        }
    }
    /// Starts over (needs `keep()` once it completed: completed tweens are killed otherwise).
    pub fn restart(&mut self, id: TweenId) {
        if let Some(t) = self.get_mut(id) {
            t.time = 0.0;
            t.completed = false;
            t.paused = false;
            t.last_loop = 0;
        }
    }
    /// The current value of a value tween (or the last value set by a property tween).
    pub fn value(&self, id: TweenId) -> Option<Vec4> {
        self.values.get(&id).copied()
    }
    /// Script callbacks fired since the last call.
    pub fn take_events(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.events)
    }
    pub fn len(&self) -> usize {
        self.list.len()
    }
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
    /// Adds a script callback to a tween: `kind` = "start" | "stepComplete" | "complete" | "kill".
    pub fn add_event(&mut self, id: TweenId, kind: &str, key: u64) -> bool {
        let Some(t) = self.get_mut(id) else { return false };
        let list = match kind {
            "start" => &mut t.on_start,
            "stepComplete" => &mut t.on_step_complete,
            "complete" => &mut t.on_complete,
            "kill" => &mut t.on_kill,
            _ => return false,
        };
        list.push(Callback::Event(key));
        true
    }
    /// Nests a running tween in a sequence (`append`: after its end, `join`: with the last one,
    /// or at `insert` seconds). Scripts build sequences this way.
    pub fn sequence_add(&mut self, seq: TweenId, child: TweenId, mode: SeqAdd) -> Result<(), String> {
        if seq == child {
            return Err("a sequence can't contain itself".into());
        }
        let Some(mut c) = self.take(child) else { return Err("no such tween".into()) };
        let Some(s) = self.get_mut(seq) else { return Err("no such sequence".into()) };
        let Node::Seq(q) = &mut s.node else { return Err("not a sequence".into()) };
        c.autokill = false;
        let cursor = s.duration;
        let start = match mode {
            SeqAdd::Append => cursor,
            SeqAdd::Join => q.children.last().map(|c| c.start).unwrap_or(0.0),
            SeqAdd::Insert(t) => t.max(0.0),
        };
        let end = start + c.full_duration().min(1e6);
        q.children.push(Child { start, tween: c });
        s.duration = s.duration.max(end);
        Ok(())
    }
    /// An empty sequence (scripts).
    pub fn new_sequence(&mut self) -> TweenId {
        self.add(Sequence::new().build())
    }
    pub fn sequence_interval(&mut self, seq: TweenId, seconds: f32) {
        if let Some(s) = self.get_mut(seq) {
            s.duration += seconds.max(0.0);
        }
    }
    pub fn sequence_callback(&mut self, seq: TweenId, at: Option<f32>, key: u64) {
        if let Some(s) = self.get_mut(seq) {
            let at = at.unwrap_or(s.duration);
            if let Node::Seq(q) = &mut s.node {
                q.callbacks.push((at, Callback::Event(key)));
            }
        }
    }

    /// Advances every tween by `dt` (scaled) / `real` (unscaled) seconds.
    fn step(&mut self, ctx: &mut Context, dt: f32, real: f32) {
        let mut list = std::mem::take(&mut self.list);
        let kills = std::mem::take(&mut self.kills);
        let mut events = std::mem::take(&mut self.events);
        let mut keep = Vec::with_capacity(list.len());
        for (id, mut t) in list.drain(..) {
            let mut run = Run { ctx: &mut *ctx, events: &mut events };
            if let Some((_, complete)) = kills.iter().find(|(k, _)| *k == id) {
                if *complete && t.loops > 0 {
                    let end = t.full_duration();
                    t.goto(end, &mut run);
                }
                fire(&mut t.on_kill, &mut run);
                continue;
            }
            if t.paused || (t.completed && !t.autokill) {
                keep.push((id, t));
                continue;
            }
            let d = if t.ignore_time_scale { real } else { dt } * self.time_scale * t.time_scale;
            let alive = t.goto(t.time + d, &mut run);
            if let Node::Leaf(l) = &t.node {
                self.values.insert(id, l.value);
            }
            if !alive {
                // target destroyed (safe mode)
                fire(&mut t.on_kill, &mut run);
                self.values.remove(&id);
                continue;
            }
            if t.completed && t.autokill {
                fire(&mut t.on_kill, &mut run);
                continue;
            }
            keep.push((id, t));
        }
        self.list = keep;
        self.events = events;
    }
}

/// Where [`Tweens::sequence_add`] puts a tween.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SeqAdd {
    Append,
    Join,
    Insert(f32),
}

/// `goat_tween::tweens(ctx).add(...)`.
pub fn tweens<'a>(ctx: &'a mut Context) -> &'a mut Tweens {
    ctx.resources.get_or_default::<Tweens>()
}

/// Runs the tweens every frame (after the behaviours), plus the [`TweenAnimation`] components
/// (feature `scene`) and the `Tween.*` script calls (feature `script`).
#[derive(Default)]
pub struct TweenPlugin;

impl Plugin for TweenPlugin {
    fn build(&mut self, ctx: &mut Context) {
        ctx.resources.get_or_default::<Tweens>();
        #[cfg(feature = "scene")]
        goat_scene::registry(ctx).component(scene::CODEC);
        #[cfg(feature = "script")]
        script::register(goat_script::extensions(ctx));
    }
    fn update(&mut self, ctx: &mut Context) {
        #[cfg(feature = "scene")]
        scene::start_animations(ctx);
        let Some(mut tweens) = ctx.resources.take::<Tweens>() else { return };
        // callbacks may create / kill tweens meanwhile: they go to a stand-in that continues the ids
        ctx.resources.insert(Tweens { next: tweens.next, ..Default::default() });
        let (dt, real) = (ctx.time.delta, ctx.time.unscaled_delta);
        tweens.step(ctx, dt, real);
        if let Some(mut added) = ctx.resources.take::<Tweens>() {
            tweens.list.append(&mut added.list);
            tweens.next = tweens.next.max(added.next);
            tweens.kills.append(&mut added.kills);
            tweens.events.append(&mut added.events);
        }
        let live: Vec<TweenId> = tweens.list.iter().map(|(i, _)| *i).collect();
        tweens.values.retain(|k, _| live.contains(k));
        ctx.resources.put_back(tweens);
    }
}

#[doc(hidden)]
pub fn _target_of(t: &Tween) -> Option<Entity> {
    t.target()
}

#[cfg(test)]
mod tests;
