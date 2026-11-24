//! Particles (the core of Unity's Shuriken).
//!
//! [`ParticleSystem`] = settings (emission, shape, start values as min..max ranges, color / size
//! over lifetime, gravity, drag, space, blend) + its live particles. [`ParticlePlugin`] simulates
//! them in `update` and draws them in `late_update` as billboards facing the main camera, through
//! a [`Batcher`](goat_sprites::Batcher): one draw per texture. Without a texture a soft round
//! dot is used.
//!
//! Optional modules ([`modules`]): color gradient / size and speed curves over lifetime, random
//! start colors, velocity over lifetime, texture sheet animation, collision (planes; 3D colliders
//! with feature `physics`), sub-emitters (birth / death / collision), trails.

pub mod modules;

pub use modules::{Collision, Curve, Gradient, SubEmitter, SubEmitterEvent, TextureSheet, Trail};

use goat_assets::AssetServer;
use goat_core::math::{Quat, Vec3};
use goat_core::Color;
use goat_gfx::{BlendMode, Display, Filter, TextureData, TextureId, Wrap};
use goat_runtime::{Camera, Context, GlobalTransform, Plugin};
use goat_sprites::{Batcher, Quad};

/// A min..max range sampled uniformly.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Range(pub f32, pub f32);

impl Range {
    pub fn constant(v: f32) -> Self {
        Range(v, v)
    }
    fn sample(&self, r: f32) -> f32 {
        self.0 + (self.1 - self.0) * r
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    Point,
    Sphere { radius: f32 },
    /// Opening up along the entity's +Y, half angle in degrees.
    Cone { angle: f32, radius: f32 },
    Box { half: Vec3 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Space {
    /// Particles stay where they were emitted when the emitter moves (smoke trails).
    World,
    /// Particles move with the emitter.
    Local,
}

#[derive(Clone, Debug)]
pub struct Particle {
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub lifetime: f32,
    pub size: f32,
    pub rotation: f32,
    pub spin: f32,
    pub seed: f32,
    /// Start color (random between `start_colors`), multiplies the color over lifetime.
    pub tint: Color,
    /// Trail points, newest last (world or local, like the particle).
    pub trail: Vec<(Vec3, f32)>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ParticleSystem {
    pub playing: bool,
    pub looping: bool,
    /// Seconds of one cycle (emission stops after it when not looping).
    pub duration: f32,
    pub max_particles: usize,
    /// Particles per second.
    pub rate: f32,
    /// (time in the cycle, count).
    pub bursts: Vec<(f32, u32)>,
    pub shape: Shape,
    pub lifetime: Range,
    pub speed: Range,
    pub size: Range,
    /// Degrees.
    pub rotation: Range,
    /// Degrees per second.
    pub spin: Range,
    pub color_start: Color,
    pub color_end: Color,
    /// Size multiplier at birth / death.
    pub size_over_life: (f32, f32),
    /// × gravity (9.81 down).
    pub gravity: f32,
    /// Velocity damping per second (0 = none).
    pub drag: f32,
    pub space: Space,
    /// Texture asset key (None = soft dot).
    pub texture: Option<String>,
    pub blend: BlendMode,
    pub layer: u8,
    pub sorting_order: i32,
    /// Color over lifetime (replaces color_start → color_end).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_over_life: Option<Gradient>,
    /// Start color picked between two (Unity's Random Between Two Colors).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_colors: Option<[Color; 2]>,
    /// Size multiplier over lifetime (replaces size_over_life).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_curve: Option<Curve>,
    /// Speed multiplier over lifetime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_curve: Option<Curve>,
    /// Added velocity (units / s, in the system's space).
    pub velocity_over_life: Vec3,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture_sheet: Option<TextureSheet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collision: Option<Collision>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sub_emitters: Vec<SubEmitter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trail: Option<Trail>,
    // state
    #[serde(skip)]
    pub particles: Vec<Particle>,
    /// Emission bookkeeping (opaque: `..Default::default()` fills it).
    #[serde(skip)]
    pub state: EmitterState,
}

#[derive(Clone, Debug)]
pub struct EmitterState {
    time: f32,
    emit_acc: f32,
    rng: u32,
    /// Births / deaths / collisions of the last step (world position, velocity), for sub-emitters.
    pub events: Vec<(SubEmitterEvent, Vec3, Vec3)>,
}

impl Default for EmitterState {
    fn default() -> Self {
        EmitterState { time: 0.0, emit_acc: 0.0, rng: 0x9e37_79b9, events: Vec::new() }
    }
}

impl Default for ParticleSystem {
    fn default() -> Self {
        ParticleSystem {
            playing: true,
            looping: true,
            duration: 5.0,
            max_particles: 500,
            rate: 20.0,
            bursts: Vec::new(),
            shape: Shape::Cone { angle: 25.0, radius: 0.2 },
            lifetime: Range(1.0, 2.0),
            speed: Range(1.5, 3.0),
            size: Range(0.15, 0.3),
            rotation: Range(0.0, 360.0),
            spin: Range(-90.0, 90.0),
            color_start: Color::WHITE,
            color_end: Color::rgba(1.0, 1.0, 1.0, 0.0),
            size_over_life: (1.0, 0.4),
            gravity: 0.0,
            drag: 0.0,
            space: Space::World,
            texture: None,
            blend: BlendMode::Additive,
            layer: 0,
            sorting_order: 0,
            color_over_life: None,
            start_colors: None,
            size_curve: None,
            speed_curve: None,
            velocity_over_life: Vec3::ZERO,
            texture_sheet: None,
            collision: None,
            sub_emitters: Vec::new(),
            trail: None,
            particles: Vec::new(),
            state: EmitterState::default(),
        }
    }
}

impl ParticleSystem {
    fn rand(&mut self) -> f32 {
        // xorshift32
        let r = &mut self.state.rng;
        *r ^= *r << 13;
        *r ^= *r >> 17;
        *r ^= *r << 5;
        (*r >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn with_seed(mut self, seed: u32) -> Self {
        self.state.rng = seed.max(1);
        self
    }

    /// Restarts the cycle (and clears the particles).
    pub fn restart(&mut self) {
        self.particles.clear();
        self.state.time = 0.0;
        self.state.emit_acc = 0.0;
        self.playing = true;
    }

    /// Emits `n` particles now (Unity's Emit), in the system's space.
    pub fn emit(&mut self, n: u32) {
        for _ in 0..n {
            if self.particles.len() >= self.max_particles {
                return;
            }
            let (pos, dir) = self.shape_point();
            let p = self.new_particle(pos, dir);
            self.particles.push(p);
        }
    }

    fn new_particle(&mut self, pos: Vec3, dir: Vec3) -> Particle {
        let r = [self.rand(), self.rand(), self.rand(), self.rand(), self.rand(), self.rand(), self.rand()];
        let tint = match self.start_colors {
            Some([a, b]) => Color::rgba(a.r + (b.r - a.r) * r[6], a.g + (b.g - a.g) * r[6], a.b + (b.b - a.b) * r[6], a.a + (b.a - a.a) * r[6]),
            None => Color::WHITE,
        };
        Particle {
            position: pos,
            velocity: dir * self.speed.sample(r[0]),
            age: 0.0,
            lifetime: self.lifetime.sample(r[1]).max(0.01),
            size: self.size.sample(r[2]),
            rotation: self.rotation.sample(r[3]).to_radians(),
            spin: self.spin.sample(r[4]).to_radians(),
            seed: r[5],
            tint,
            trail: Vec::new(),
        }
    }

    /// Emits `n` particles at a world position (sub-emitters, scripts), shape around it, plus
    /// `velocity`. Local-space systems convert through `to_world`.
    pub fn emit_at(&mut self, n: u32, world_pos: Vec3, velocity: Vec3, to_world: &goat_core::math::Mat4) {
        let inv = to_world.inverse();
        for _ in 0..n {
            if self.particles.len() >= self.max_particles {
                return;
            }
            let (off, dir) = self.shape_point();
            let mut p = self.new_particle(Vec3::ZERO, dir);
            match self.space {
                Space::World => {
                    let rot = to_world.to_scale_rotation_translation().1;
                    p.position = world_pos + rot * off;
                    p.velocity = rot * p.velocity + velocity;
                }
                Space::Local => {
                    p.position = inv.transform_point3(world_pos) + off;
                    p.velocity += inv.transform_vector3(velocity);
                }
            }
            self.particles.push(p);
        }
    }

    /// Life fraction values used by drawing: (color, size multiplier).
    pub fn look(&self, p: &Particle) -> (Color, f32) {
        let t = (p.age / p.lifetime).clamp(0.0, 1.0);
        let c = match &self.color_over_life {
            Some(g) => g.eval(t),
            None => {
                let (a, b) = (self.color_start, self.color_end);
                Color::rgba(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t)
            }
        };
        let c = Color::rgba(c.r * p.tint.r, c.g * p.tint.g, c.b * p.tint.b, c.a * p.tint.a);
        let size = match &self.size_curve {
            Some(k) => k.eval(t),
            None => self.size_over_life.0 + (self.size_over_life.1 - self.size_over_life.0) * t,
        };
        (c, size)
    }

    /// A start position and direction in the emitter's local space.
    fn shape_point(&mut self) -> (Vec3, Vec3) {
        let unit = |s: &mut Self| loop {
            let v = Vec3::new(s.rand() * 2.0 - 1.0, s.rand() * 2.0 - 1.0, s.rand() * 2.0 - 1.0);
            if v.length_squared() > 1e-4 && v.length_squared() <= 1.0 {
                return v.normalize();
            }
        };
        match self.shape {
            Shape::Point => (Vec3::ZERO, unit(self)),
            Shape::Sphere { radius } => {
                let d = unit(self);
                (d * radius * self.rand().cbrt(), d)
            }
            Shape::Cone { angle, radius } => {
                let a = self.rand() * std::f32::consts::TAU;
                let r = self.rand().sqrt();
                let base = Vec3::new(a.cos() * r * radius, 0.0, a.sin() * r * radius);
                let tilt = angle.to_radians() * r;
                let dir = Vec3::new(a.cos() * tilt.sin(), tilt.cos(), a.sin() * tilt.sin());
                (base, dir.normalize())
            }
            Shape::Box { half } => {
                let p = Vec3::new(self.rand() * 2.0 - 1.0, self.rand() * 2.0 - 1.0, self.rand() * 2.0 - 1.0) * half;
                (p, Vec3::Y)
            }
        }
    }

    /// One step: emission (rate + bursts) and motion. `to_world` places new particles when the
    /// system simulates in world space.
    pub fn simulate(&mut self, dt: f32, to_world: &goat_core::math::Mat4) {
        self.simulate_with(dt, to_world, &|_, _| None);
    }

    /// [`simulate`](Self::simulate) with a world hit test for collisions: `hit(from, to)` →
    /// (point, normal) of the first surface on the segment (world space).
    pub fn simulate_with(&mut self, dt: f32, to_world: &goat_core::math::Mat4, hit: &dyn Fn(Vec3, Vec3) -> Option<(Vec3, Vec3)>) {
        self.state.events.clear();
        let track = !self.sub_emitters.is_empty();
        let born_from = self.particles.len();
        if self.playing {
            let before = self.particles.len();
            let prev_time = self.state.time;
            self.state.time += dt;
            let now = self.state.time;
            let in_cycle = self.looping || now <= self.duration;
            if in_cycle {
                self.state.emit_acc += self.rate * dt;
                let n = self.state.emit_acc as u32;
                self.state.emit_acc -= n as f32;
                self.emit(n);
                // bursts crossing their time in this step
                let cycle = |t: f32, d: f32| if d > 0.0 { t % d } else { t };
                let (t0, t1) = (cycle(prev_time, self.duration), cycle(now, self.duration));
                let bursts = self.bursts.clone();
                for (bt, count) in bursts {
                    let crossed = if t1 >= t0 { bt >= t0 && bt < t1 } else { bt >= t0 || bt < t1 };
                    if crossed || (prev_time == 0.0 && bt == 0.0) {
                        self.emit(count);
                    }
                }
            } else if self.particles.is_empty() {
                self.playing = false;
            }
            if self.space == Space::World {
                let rot = to_world.to_scale_rotation_translation().1;
                for p in &mut self.particles[before..] {
                    p.position = to_world.transform_point3(p.position);
                    p.velocity = rot * p.velocity;
                }
            }
        }
        let world_of = |p: Vec3, space: Space| if space == Space::Local { to_world.transform_point3(p) } else { p };
        if track {
            for i in born_from..self.particles.len() {
                let p = &self.particles[i];
                let wv = if self.space == Space::Local { to_world.transform_vector3(p.velocity) } else { p.velocity };
                self.state.events.push((SubEmitterEvent::Birth, world_of(p.position, self.space), wv));
            }
        }
        let g = Vec3::new(0.0, -9.81 * self.gravity, 0.0);
        let damp = (1.0 - self.drag * dt).clamp(0.0, 1.0);
        let inv = to_world.inverse();
        let space = self.space;
        let (speed_curve, extra) = (self.speed_curve.clone(), self.velocity_over_life);
        let collision = self.collision.clone();
        let trail = self.trail.clone();
        let mut events = std::mem::take(&mut self.state.events);
        for p in &mut self.particles {
            p.age += dt;
            p.velocity = (p.velocity + g * dt) * damp;
            let mult = speed_curve.as_ref().map(|c| c.eval((p.age / p.lifetime).clamp(0.0, 1.0))).unwrap_or(1.0);
            let before = p.position;
            p.position += (p.velocity * mult + extra) * dt;
            p.rotation += p.spin * dt;
            if let Some(c) = &collision {
                // world-space segment of this step
                let (w0, w1) = (world_of(before, space), world_of(p.position, space));
                let mut hit_at: Option<(Vec3, Vec3)> = None;
                let radius = p.size * c.radius_scale;
                for (point, normal) in &c.planes {
                    let n = normal.normalize_or(Vec3::Y);
                    let d1 = (w1 - *point).dot(n) - radius;
                    if d1 < 0.0 && (w0 - *point).dot(n) - radius >= -1e-4 {
                        hit_at = Some((w1 - n * d1, n));
                    }
                }
                if hit_at.is_none() && c.world && (w1 - w0).length_squared() > 1e-10 {
                    hit_at = hit(w0, w1);
                }
                if let Some((point, n)) = hit_at {
                    // reflect in world space
                    let wv = if space == Space::Local { to_world.transform_vector3(p.velocity) } else { p.velocity };
                    let vn = n * wv.dot(n);
                    let vt = wv - vn;
                    let out = vt * c.dampen - vn * c.bounce;
                    let place = point + n * (radius + 1e-3);
                    p.velocity = if space == Space::Local { inv.transform_vector3(out) } else { out };
                    p.position = if space == Space::Local { inv.transform_point3(place) } else { place };
                    p.age += p.lifetime * c.lifetime_loss;
                    if track {
                        events.push((SubEmitterEvent::Collision, point, out));
                    }
                }
            }
            if let Some(t) = &trail {
                let far = p.trail.last().is_none_or(|(q, _)| (*q - p.position).length() >= t.min_vertex_distance);
                if far {
                    p.trail.push((p.position, p.age));
                }
                let age = p.age;
                p.trail.retain(|(_, a)| age - *a <= t.lifetime);
                if p.trail.len() > t.max_points.max(2) {
                    let extra = p.trail.len() - t.max_points.max(2);
                    p.trail.drain(..extra);
                }
            }
        }
        if track {
            for p in self.particles.iter().filter(|p| p.age >= p.lifetime) {
                let wv = if space == Space::Local { to_world.transform_vector3(p.velocity) } else { p.velocity };
                events.push((SubEmitterEvent::Death, world_of(p.position, space), wv));
            }
        }
        self.state.events = events;
        self.particles.retain(|p| p.age < p.lifetime);
    }
}

/// A 64×64 soft white dot (the default particle texture).
fn soft_dot() -> TextureData {
    let n = 64u32;
    let mut rgba = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = ((x as f32 + 0.5) / n as f32 * 2.0 - 1.0, (y as f32 + 0.5) / n as f32 * 2.0 - 1.0);
            let a = (1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            rgba.extend_from_slice(&[255, 255, 255, (a * a * 255.0) as u8]);
        }
    }
    TextureData { width: n, height: n, rgba, filter: Filter::Linear, wrap: Wrap::Clamp, mipmaps: true, format: Default::default(), compressed: None }
}

/// Simulates and draws every [`ParticleSystem`].
pub struct ParticlePlugin {
    batcher: Batcher,
    dot: Option<TextureId>,
}

impl Default for ParticlePlugin {
    fn default() -> Self {
        ParticlePlugin { batcher: Batcher::new("Particles"), dot: None }
    }
}

impl Plugin for ParticlePlugin {
    fn build(&mut self, _ctx: &mut Context) {
        #[cfg(feature = "scene")]
        goat_scene::registry(_ctx).component(goat_scene::serde_codec::<ParticleSystem>("ParticleSystem"));
    }

    fn update(&mut self, ctx: &mut Context) {
        let dt = ctx.time.delta;
        #[cfg(feature = "physics")]
        let physics = ctx.resources.get::<goat_physics::Physics>();
        #[cfg(feature = "physics")]
        let hit = |a: Vec3, b: Vec3| -> Option<(Vec3, Vec3)> {
            let d = b - a;
            let h = physics?.raycast(a, d, d.length())?;
            Some((h.point, h.normal))
        };
        #[cfg(not(feature = "physics"))]
        let hit = |_: Vec3, _: Vec3| -> Option<(Vec3, Vec3)> { None };
        let mut subs: Vec<(String, u32, f32, Vec<(Vec3, Vec3)>)> = Vec::new();
        for (_, (ps, g)) in ctx.world.query_mut::<(&mut ParticleSystem, &GlobalTransform)>() {
            ps.simulate_with(dt, &g.0, &hit);
            for se in &ps.sub_emitters {
                let at: Vec<(Vec3, Vec3)> = ps.state.events.iter().filter(|(k, _, _)| *k == se.on).map(|(_, p, v)| (*p, *v)).collect();
                if !at.is_empty() && se.count > 0 {
                    subs.push((se.system.clone(), se.count, se.inherit_velocity, at));
                }
            }
        }
        // sub-emitters: emit from the named systems
        for (name, count, inherit, at) in subs {
            let target = ctx.world.query::<(&goat_runtime::Name, &ParticleSystem)>().iter().find(|(_, (n, _))| n.0 == name).map(|(e, _)| e);
            let Some(t) = target else { continue };
            let m = ctx.world.get::<&GlobalTransform>(t).map(|g| g.0).unwrap_or(goat_core::math::Mat4::IDENTITY);
            if let Ok(mut ps) = ctx.world.get::<&mut ParticleSystem>(t) {
                for (p, v) in at {
                    ps.emit_at(count, p, v * inherit, &m);
                }
            }
        }
    }

    fn late_update(&mut self, ctx: &mut Context) {
        if self.dot.is_none() {
            self.dot = ctx.gfx.create_texture(&soft_dot()).ok();
        }
        let Some(dot) = self.dot else { return };
        // billboards face the main screen's first camera
        let cam = ctx
            .world
            .query::<(&Camera, &GlobalTransform)>()
            .iter()
            .filter(|(_, (c, _))| c.display == Display::Main)
            .min_by_key(|(_, (c, _))| c.depth)
            .map(|(_, (_, g))| g.0.to_scale_rotation_translation().1)
            .unwrap_or(Quat::IDENTITY);
        let (right, up, fwd) = (cam * Vec3::X, cam * Vec3::Y, cam * Vec3::Z);
        // textures first (needs the GPU), then the systems are read in place (no copies)
        let keys: Vec<(goat_runtime::Entity, Option<String>)> = ctx.world.query::<&ParticleSystem>().without::<&goat_runtime::InactiveInHierarchy>().iter().filter(|(_, ps)| !ps.particles.is_empty()).map(|(e, ps)| (e, ps.texture.clone())).collect();
        let mut tex_of = std::collections::HashMap::with_capacity(keys.len());
        for (e, k) in keys {
            let t = k.as_deref().and_then(|k| ctx.resources.get_mut::<AssetServer>().and_then(|a| a.texture(ctx.gfx, k).ok())).unwrap_or(dot);
            tex_of.insert(e, t);
        }
        let mut quads = Vec::new();
        for (e, (ps, g)) in ctx.world.query::<(&ParticleSystem, &GlobalTransform)>().without::<&goat_runtime::InactiveInHierarchy>().iter() {
            let Some(&texture) = tex_of.get(&e) else { continue };
            let m = &g.0;
            let to_w = |q: Vec3| if ps.space == Space::Local { m.transform_point3(q) } else { q };
            for p in &ps.particles {
                let t = (p.age / p.lifetime).clamp(0.0, 1.0);
                let pos = to_w(p.position);
                let (col, mult) = ps.look(p);
                let size = p.size * mult * 0.5;
                let (s, c) = p.rotation.sin_cos();
                let (r, u) = ((right * c + up * s) * size, (up * c - right * s) * size);
                let [u0, v0, u1, v1] = ps.texture_sheet.map(|sh| sh.uv(t, p.seed)).unwrap_or([0.0, 0.0, 1.0, 1.0]);
                quads.push(Quad {
                    sorting_order: ps.sorting_order,
                    layer: ps.layer,
                    texture,
                    blend: ps.blend,
                    // back to front along the camera's view
                    z: -pos.dot(fwd),
                    corners: [pos - r - u, pos + r - u, pos + r + u, pos - r + u],
                    normal: fwd,
                    uv: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
                    color: [col.r, col.g, col.b, col.a],
                });
                // trail: a camera-facing ribbon through the history, head = the particle
                if let (Some(tr), true) = (&ps.trail, p.trail.len() >= 1) {
                    let mut pts: Vec<Vec3> = p.trail.iter().map(|(q, _)| to_w(*q)).collect();
                    pts.push(pos);
                    let n = pts.len();
                    let half = p.size * mult * 0.5 * tr.width;
                    for k in 0..n - 1 {
                        let (a, b) = (pts[k], pts[k + 1]);
                        let along = (b - a).normalize_or_zero();
                        if along == Vec3::ZERO {
                            continue;
                        }
                        let side = along.cross(fwd).normalize_or_zero();
                        // 0 at the tail, 1 at the head
                        let (fa, fb) = (k as f32 / (n - 1) as f32, (k + 1) as f32 / (n - 1) as f32);
                        let w = |f: f32| half * (tr.width_tail + (1.0 - tr.width_tail) * f);
                        let alpha = |f: f32| col.a * (tr.tail_alpha + (1.0 - tr.tail_alpha) * f);
                        let a_alpha = (alpha(fa) + alpha(fb)) * 0.5;
                        quads.push(Quad {
                            sorting_order: ps.sorting_order,
                            layer: ps.layer,
                            texture,
                            blend: ps.blend,
                            z: -((a + b) * 0.5).dot(fwd),
                            corners: [a - side * w(fa), b - side * w(fb), b + side * w(fb), a + side * w(fa)],
                            normal: fwd,
                            uv: [[0.5, 1.0], [0.5, 1.0], [0.5, 0.0], [0.5, 0.0]],
                            color: [col.r, col.g, col.b, a_alpha],
                        });
                    }
                }
            }
        }
        self.batcher.submit(ctx, quads);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goat_core::math::Mat4;

    #[test]
    fn emits_at_rate_with_bursts_and_dies_with_age() {
        let mut ps = ParticleSystem { rate: 10.0, bursts: vec![(0.0, 5)], lifetime: Range::constant(1.0), max_particles: 1000, ..Default::default() };
        let m = Mat4::from_translation(Vec3::new(0.0, 10.0, 0.0));
        ps.simulate(0.5, &m);
        assert_eq!(ps.particles.len(), 5 + 5, "burst at t = 0 + 0.5 s × 10/s");
        assert!(ps.particles.iter().all(|p| p.position.y >= 9.9), "world space: emitted at the emitter");
        for _ in 0..10 {
            ps.simulate(0.1, &m);
        }
        assert!(ps.particles.iter().all(|p| p.age < 1.0), "old ones died");
    }

    #[test]
    fn one_shot_systems_stop_and_gravity_pulls() {
        let mut ps = ParticleSystem { looping: false, duration: 0.2, rate: 0.0, bursts: vec![(0.0, 3)], gravity: 1.0, speed: Range::constant(0.0), lifetime: Range::constant(0.5), ..Default::default() };
        ps.simulate(0.1, &Mat4::IDENTITY);
        assert_eq!(ps.particles.len(), 3);
        ps.simulate(0.1, &Mat4::IDENTITY);
        assert!(ps.particles[0].velocity.y < -1.0, "falling");
        for _ in 0..10 {
            ps.simulate(0.1, &Mat4::IDENTITY);
        }
        assert!(ps.particles.is_empty() && !ps.playing);
    }

    #[test]
    fn collision_planes_bounce_and_sub_emitters_report() {
        let mut ps = ParticleSystem {
            rate: 0.0,
            bursts: vec![(0.0, 1)],
            shape: Shape::Point,
            speed: Range::constant(0.0),
            lifetime: Range::constant(5.0),
            gravity: 1.0,
            collision: Some(Collision { planes: vec![(Vec3::ZERO, Vec3::Y)], bounce: 0.5, radius_scale: 0.0, ..Default::default() }),
            sub_emitters: vec![SubEmitter { on: SubEmitterEvent::Collision, system: "sparks".into(), count: 3, inherit_velocity: 0.0 }],
            trail: Some(Trail::default()),
            ..Default::default()
        };
        let m = Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0));
        let mut hits = 0;
        let mut max_after = 0.0f32;
        let mut bounced = false;
        let mut trail = 0;
        for _ in 0..120 {
            ps.simulate(1.0 / 60.0, &m);
            trail = trail.max(ps.particles[0].trail.len());
            hits += ps.state.events.iter().filter(|e| e.0 == SubEmitterEvent::Collision).count();
            let y = ps.particles[0].position.y;
            assert!(y >= -1e-3, "kept above the plane: {y}");
            if hits > 0 {
                bounced |= ps.particles[0].velocity.y > 0.0;
                max_after = max_after.max(y);
            }
        }
        assert!(hits >= 1 && bounced);
        assert!(max_after > 0.1 && max_after < 0.5, "bounced to about a quarter of the drop: {max_after}");
        assert!(trail >= 3, "trail points recorded while falling: {trail}");
        // emit_at: world position for a world-space system
        let mut sparks = ParticleSystem { rate: 0.0, shape: Shape::Point, speed: Range::constant(0.0), ..Default::default() };
        sparks.emit_at(4, Vec3::new(5.0, 0.0, 0.0), Vec3::Y, &Mat4::IDENTITY);
        assert_eq!(sparks.particles.len(), 4);
        assert!(sparks.particles.iter().all(|p| p.position == Vec3::new(5.0, 0.0, 0.0) && p.velocity == Vec3::Y));
        // gradients and size curves
        let g = ParticleSystem { color_over_life: Some(Gradient(vec![(0.0, Color::WHITE), (1.0, Color::rgba(1.0, 0.0, 0.0, 0.0))])), size_curve: Some(Curve::linear(0.0, 2.0)), ..Default::default() };
        let p = Particle { age: 0.5, lifetime: 1.0, ..sparks.particles[0].clone() };
        let (c, s) = g.look(&p);
        assert!((c.g - 0.5).abs() < 1e-5 && (s - 1.0).abs() < 1e-5);
    }
}