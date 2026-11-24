//! Particles (the core of Unity's Shuriken).
//!
//! [`ParticleSystem`] = settings (emission, shape, start values as min..max ranges, color / size
//! over lifetime, gravity, drag, space, blend) + its live particles. [`ParticlePlugin`] simulates
//! them in `update` and draws them in `late_update` as billboards facing the main camera, through
//! a [`Batcher`](goat_sprites::Batcher): one draw per texture. Without a texture a soft round
//! dot is used.

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

#[derive(Clone, Copy, Debug)]
pub struct Particle {
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub lifetime: f32,
    pub size: f32,
    pub rotation: f32,
    pub spin: f32,
    pub seed: f32,
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
}

impl Default for EmitterState {
    fn default() -> Self {
        EmitterState { time: 0.0, emit_acc: 0.0, rng: 0x9e37_79b9 }
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
            let r = [self.rand(), self.rand(), self.rand(), self.rand(), self.rand(), self.rand()];
            self.particles.push(Particle {
                position: pos,
                velocity: dir * self.speed.sample(r[0]),
                age: 0.0,
                lifetime: self.lifetime.sample(r[1]).max(0.01),
                size: self.size.sample(r[2]),
                rotation: self.rotation.sample(r[3]).to_radians(),
                spin: self.spin.sample(r[4]).to_radians(),
                seed: r[5],
            });
        }
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
        let g = Vec3::new(0.0, -9.81 * self.gravity, 0.0);
        let damp = (1.0 - self.drag * dt).clamp(0.0, 1.0);
        for p in &mut self.particles {
            p.age += dt;
            p.velocity = (p.velocity + g * dt) * damp;
            p.position += p.velocity * dt;
            p.rotation += p.spin * dt;
        }
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
    TextureData { width: n, height: n, rgba, filter: Filter::Linear, wrap: Wrap::Clamp, mipmaps: true, format: Default::default() }
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
        for (_, (ps, g)) in ctx.world.query_mut::<(&mut ParticleSystem, &GlobalTransform)>() {
            ps.simulate(dt, &g.0);
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
        let mut systems: Vec<(ParticleSystem, goat_core::math::Mat4)> = Vec::new();
        for (_, (ps, g)) in ctx.world.query::<(&ParticleSystem, &GlobalTransform)>().iter() {
            if !ps.particles.is_empty() {
                systems.push((ps.clone(), g.0));
            }
        }
        let mut quads = Vec::new();
        for (ps, m) in &systems {
            let texture = ps.texture.as_deref().and_then(|k| ctx.resources.get_mut::<AssetServer>().and_then(|a| a.texture(ctx.gfx, k).ok())).unwrap_or(dot);
            for p in &ps.particles {
                let t = (p.age / p.lifetime).clamp(0.0, 1.0);
                let pos = if ps.space == Space::Local { m.transform_point3(p.position) } else { p.position };
                let size = p.size * (ps.size_over_life.0 + (ps.size_over_life.1 - ps.size_over_life.0) * t) * 0.5;
                let (s, c) = p.rotation.sin_cos();
                let (r, u) = ((right * c + up * s) * size, (up * c - right * s) * size);
                let col = Color::rgba(
                    ps.color_start.r + (ps.color_end.r - ps.color_start.r) * t,
                    ps.color_start.g + (ps.color_end.g - ps.color_start.g) * t,
                    ps.color_start.b + (ps.color_end.b - ps.color_start.b) * t,
                    ps.color_start.a + (ps.color_end.a - ps.color_start.a) * t,
                );
                quads.push(Quad {
                    sorting_order: ps.sorting_order,
                    layer: ps.layer,
                    texture,
                    blend: ps.blend,
                    // back to front along the camera's view
                    z: -pos.dot(fwd),
                    corners: [pos - r - u, pos + r - u, pos + r + u, pos - r + u],
                    normal: fwd,
                    uv: [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                    color: [col.r, col.g, col.b, col.a],
                });
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
}
