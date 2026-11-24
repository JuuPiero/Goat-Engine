//! Shuriken modules beyond the core: curves / gradients over lifetime, texture sheet animation,
//! collision, sub-emitters, trails. All optional in scene files (missing = off).

use goat_core::math::Vec3;
use goat_core::Color;
use serde::{Deserialize, Serialize};

/// A curve of (time 0..1, value) keys, linear between keys (Unity's AnimationCurve, sampled).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Curve(pub Vec<(f32, f32)>);

impl Curve {
    pub fn linear(a: f32, b: f32) -> Self {
        Curve(vec![(0.0, a), (1.0, b)])
    }
    pub fn eval(&self, t: f32) -> f32 {
        let k = &self.0;
        match k.len() {
            0 => 1.0,
            1 => k[0].1,
            _ => {
                if t <= k[0].0 {
                    return k[0].1;
                }
                for w in k.windows(2) {
                    let ((t0, v0), (t1, v1)) = (w[0], w[1]);
                    if t <= t1 {
                        let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
                        return v0 + (v1 - v0) * f;
                    }
                }
                k[k.len() - 1].1
            }
        }
    }
}

/// Color keys over 0..1 (Unity's Gradient, alpha included in each key).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Gradient(pub Vec<(f32, Color)>);

impl Gradient {
    pub fn eval(&self, t: f32) -> Color {
        let k = &self.0;
        match k.len() {
            0 => Color::WHITE,
            1 => k[0].1,
            _ => {
                if t <= k[0].0 {
                    return k[0].1;
                }
                for w in k.windows(2) {
                    let ((t0, a), (t1, b)) = (w[0], w[1]);
                    if t <= t1 {
                        let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
                        return Color::rgba(a.r + (b.r - a.r) * f, a.g + (b.g - a.g) * f, a.b + (b.b - a.b) * f, a.a + (b.a - a.a) * f);
                    }
                }
                k[k.len() - 1].1
            }
        }
    }
}

/// Flipbook frames from a texture grid (Texture Sheet Animation module).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextureSheet {
    /// Columns × rows.
    pub tiles: [u32; 2],
    /// Times the whole sheet plays over a particle's life.
    pub cycles: f32,
    /// Each particle picks a random row and plays it (Single Row mode).
    pub random_row: bool,
    /// First frame random per particle.
    pub random_start: bool,
}

impl Default for TextureSheet {
    fn default() -> Self {
        TextureSheet { tiles: [1, 1], cycles: 1.0, random_row: false, random_start: false }
    }
}

impl TextureSheet {
    /// uv (u0, v0, u1, v1) of the frame at life fraction `t` (v down) for a particle.
    pub fn uv(&self, t: f32, seed: f32) -> [f32; 4] {
        let (cols, rows) = (self.tiles[0].max(1), self.tiles[1].max(1));
        let frames = if self.random_row { cols } else { cols * rows };
        let start = if self.random_start { (seed * frames as f32) as u32 } else { 0 };
        let f = ((t * self.cycles.max(0.0) * frames as f32) as u32 + start) % frames;
        let (col, row) = if self.random_row { (f, ((seed * 7919.0).fract() * rows as f32) as u32 % rows) } else { (f % cols, f / cols) };
        let (w, h) = (1.0 / cols as f32, 1.0 / rows as f32);
        [col as f32 * w, row as f32 * h, (col + 1) as f32 * w, (row + 1) as f32 * h]
    }
}

/// Bounces / kills particles on planes and (feature `physics`) on 3D colliders.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Collision {
    /// World planes (point, normal): particles stay on the normal's side.
    pub planes: Vec<(Vec3, Vec3)>,
    /// Collide with goat-physics colliders (raycasts; feature `physics`).
    pub world: bool,
    /// Kept fraction of the speed along the normal (0 = stop, 1 = perfect bounce).
    pub bounce: f32,
    /// Kept fraction of the speed along the surface.
    pub dampen: f32,
    /// Fraction of its lifetime a particle loses per hit (1 = dies).
    pub lifetime_loss: f32,
    /// Particle radius for collisions (× its size).
    pub radius_scale: f32,
}

impl Default for Collision {
    fn default() -> Self {
        Collision { planes: Vec::new(), world: false, bounce: 0.5, dampen: 0.8, lifetime_loss: 0.0, radius_scale: 0.5 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubEmitterEvent {
    #[default]
    Birth,
    Death,
    Collision,
}

/// Emits particles of another system where something happens to this one's particles
/// (fireworks: sparks on death).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubEmitter {
    pub on: SubEmitterEvent,
    /// Name of the entity with the system to emit from (usually a child).
    pub system: String,
    pub count: u32,
    /// Fraction of the parent particle's velocity the new ones start with.
    pub inherit_velocity: f32,
}

/// A ribbon behind each particle (Trails module).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Trail {
    /// Seconds of history kept.
    pub lifetime: f32,
    /// New point when the particle moved this far.
    pub min_vertex_distance: f32,
    /// Width at the head (× particle size) and at the tail (× head).
    pub width: f32,
    pub width_tail: f32,
    /// Alpha at the tail (× particle color).
    pub tail_alpha: f32,
    /// Most points per trail (3DS: keep it small).
    pub max_points: usize,
}

impl Default for Trail {
    fn default() -> Self {
        Trail { lifetime: 0.3, min_vertex_distance: 0.05, width: 1.0, width_tail: 0.0, tail_alpha: 0.0, max_points: 16 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_gradients_and_sheets() {
        let c = Curve(vec![(0.0, 0.0), (0.5, 2.0), (1.0, 1.0)]);
        assert_eq!((c.eval(0.25), c.eval(0.75), c.eval(2.0)), (1.0, 1.5, 1.0));
        let g = Gradient(vec![(0.0, Color::rgba(1.0, 0.0, 0.0, 1.0)), (1.0, Color::rgba(0.0, 0.0, 1.0, 0.0))]);
        let m = g.eval(0.5);
        assert!((m.r - 0.5).abs() < 1e-6 && (m.a - 0.5).abs() < 1e-6);
        let s = TextureSheet { tiles: [4, 2], ..Default::default() };
        assert_eq!(s.uv(0.0, 0.0), [0.0, 0.0, 0.25, 0.5]);
        assert_eq!(s.uv(0.5, 0.0), [0.0, 0.5, 0.25, 1.0], "frame 4 = second row");
    }
}
