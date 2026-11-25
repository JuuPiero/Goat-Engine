//! Light probes and reflection probes (Unity's): what [`extract_views`](crate::engine::extract_views)
//! attaches to each [`DrawItem`](goat_gfx::DrawItem).
//!
//! Light probes store the irradiance of the [`LightMode::Baked`] lights and the ambient light as
//! linear spherical harmonics per color channel: E(n) = a + d · n (a clamped cosine lobe projects
//! to ¼ + ½ n · l). An object blends its 4 nearest probes by inverse squared distance, and the
//! result becomes what every backend can draw: an ambient color plus one directional light along
//! the dominant direction. Occlusion isn't captured (no shadows in probes).

use goat_core::math::Vec3;
use goat_core::Color;
use goat_gfx::{Light, LightKind, LightProbeSample, TextureId};
use hecs::World;

use crate::{GlobalTransform, InactiveInHierarchy, LightProbeGroup, ReflectionProbe, RenderSettings};

/// Linear SH of one probe: `a` (rgb) + `d[c]` · n for channel c.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProbeSh {
    pub a: Vec3,
    pub d: [Vec3; 3],
}

impl ProbeSh {
    fn add_light(&mut self, color: Color, to_light: Vec3) {
        let c = Vec3::new(color.r, color.g, color.b);
        self.a += c * 0.25;
        for (k, v) in [c.x, c.y, c.z].into_iter().enumerate() {
            self.d[k] += to_light * (0.5 * v);
        }
    }
    fn scaled_add(&mut self, o: &ProbeSh, w: f32) {
        self.a += o.a * w;
        for k in 0..3 {
            self.d[k] += o.d[k] * w;
        }
    }
    /// The irradiance toward `n` (tests).
    pub fn eval(&self, n: Vec3) -> Vec3 {
        self.a + Vec3::new(self.d[0].dot(n), self.d[1].dot(n), self.d[2].dot(n))
    }
    /// As an ambient color + one directional light: exact facing the dominant direction, a bit
    /// bright facing away (the directional part can't go negative).
    pub fn to_sample(&self) -> LightProbeSample {
        let lum = self.d[0] * 0.3 + self.d[1] * 0.59 + self.d[2] * 0.11;
        let dir = lum.normalize_or_zero();
        if dir == Vec3::ZERO {
            return LightProbeSample { ambient: Color::rgb(self.a.x.max(0.0), self.a.y.max(0.0), self.a.z.max(0.0)), direction: Vec3::NEG_Y, color: Color::BLACK };
        }
        let along = Vec3::new(self.d[0].dot(dir).max(0.0), self.d[1].dot(dir).max(0.0), self.d[2].dot(dir).max(0.0));
        let amb = (self.a - along * 0.5).max(Vec3::ZERO);
        let light = along * 1.5;
        LightProbeSample { ambient: Color::rgb(amb.x, amb.y, amb.z), direction: -dir, color: Color::rgb(light.x, light.y, light.z) }
    }
}

/// Every light probe of the world with its captured light.
#[derive(Clone, Debug, Default)]
pub struct ProbeField {
    pub probes: Vec<(Vec3, ProbeSh)>,
}

impl ProbeField {
    /// Captures the probes of every active [`LightProbeGroup`] (None without any).
    pub fn capture(world: &World, baked: &[Light]) -> Option<ProbeField> {
        let mut positions = Vec::new();
        for (_, (g, t)) in world.query::<(&LightProbeGroup, &GlobalTransform)>().without::<&InactiveInHierarchy>().iter() {
            positions.extend(g.positions.iter().map(|p| t.0.transform_point3(*p)));
        }
        if positions.is_empty() {
            return None;
        }
        let settings = world.query::<&RenderSettings>().iter().next().map(|(_, s)| *s).unwrap_or_default();
        let mut base = ProbeSh::default();
        let sky = Vec3::new(settings.ambient.r, settings.ambient.g, settings.ambient.b);
        match settings.ambient_ground {
            // hemisphere: ground from below to sky from above, linear in n.y
            Some(g) => {
                let ground = Vec3::new(g.r, g.g, g.b);
                base.a = (sky + ground) * 0.5;
                let half = (sky - ground) * 0.5;
                base.d = [Vec3::Y * half.x, Vec3::Y * half.y, Vec3::Y * half.z];
            }
            None => base.a = sky,
        }
        let probes = positions
            .into_iter()
            .map(|p| {
                let mut sh = base;
                for l in baked {
                    match l.kind {
                        LightKind::Directional => sh.add_light(l.color, -l.direction.normalize_or(Vec3::NEG_Y)),
                        _ => {
                            let v = l.position - p;
                            let dist = v.length();
                            if dist >= l.range || dist < 1e-5 {
                                continue;
                            }
                            let to_light = v / dist;
                            let mut k = Light::attenuation(dist, l.range);
                            if let LightKind::Spot { cos_inner, cos_outer } = l.kind {
                                k *= Light::spot_factor((-to_light).dot(l.direction.normalize_or(Vec3::NEG_Z)), cos_inner, cos_outer);
                            }
                            if k > 0.0 {
                                sh.add_light(Color::rgb(l.color.r * k, l.color.g * k, l.color.b * k), to_light);
                            }
                        }
                    }
                }
                (p, sh)
            })
            .collect();
        Some(ProbeField { probes })
    }

    /// The light at `p`: the 4 nearest probes blended by inverse squared distance.
    pub fn sample_sh(&self, p: Vec3) -> ProbeSh {
        let mut best: [(f32, usize); 4] = [(f32::INFINITY, usize::MAX); 4];
        for (i, (q, _)) in self.probes.iter().enumerate() {
            let d = q.distance_squared(p);
            if d < best[3].0 {
                let mut k = 3;
                while k > 0 && best[k - 1].0 > d {
                    best[k] = best[k - 1];
                    k -= 1;
                }
                best[k] = (d, i);
            }
        }
        let mut out = ProbeSh::default();
        let mut total = 0.0;
        for (d, i) in best {
            if i == usize::MAX {
                continue;
            }
            if d < 1e-8 {
                return self.probes[i].1;
            }
            let w = 1.0 / d;
            out.scaled_add(&self.probes[i].1, w);
            total += w;
        }
        if total > 0.0 {
            out.a /= total;
            for k in 0..3 {
                out.d[k] /= total;
            }
        }
        out
    }

    pub fn sample(&self, p: Vec3) -> LightProbeSample {
        self.sample_sh(p).to_sample()
    }
}

/// The world's reflection probes as world boxes: (min, max, importance, volume, cube map).
pub fn reflection_boxes(world: &World) -> Vec<(Vec3, Vec3, i32, f32, TextureId)> {
    world
        .query::<(&ReflectionProbe, &GlobalTransform)>()
        .without::<&InactiveInHierarchy>()
        .iter()
        .filter_map(|(_, (r, t))| {
            let cube = r.cubemap?;
            let c = t.0.transform_point3(r.center);
            let h = r.size.abs() * 0.5;
            Some((c - h, c + h, r.importance, (h.x * h.y * h.z).max(1e-6), cube))
        })
        .collect()
}

/// The cube map for an object centred at `p`: the most important box holding it, then the smallest.
pub fn reflection_at(boxes: &[(Vec3, Vec3, i32, f32, TextureId)], p: Vec3) -> Option<TextureId> {
    let mut best: Option<(i32, f32, TextureId)> = None;
    for &(lo, hi, importance, volume, cube) in boxes {
        if p.cmpge(lo).all() && p.cmple(hi).all() {
            let better = match best {
                None => true,
                Some((bi, bv, _)) => importance > bi || (importance == bi && volume < bv),
            };
            if better {
                best = Some((importance, volume, cube));
            }
        }
    }
    best.map(|(_, _, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LightMode, PointLight, Transform};

    #[test]
    fn sh_of_one_light_matches_the_clamped_cosine_on_its_axis() {
        let mut sh = ProbeSh::default();
        sh.add_light(Color::WHITE, Vec3::Y);
        // facing the light: ¼ + ½ = 0.75 (the linear fit of max(0, n·l)); away: 0 (well, −¼ + ¼)
        assert!((sh.eval(Vec3::Y).x - 0.75).abs() < 1e-5);
        assert!((sh.eval(Vec3::NEG_Y).x + 0.25).abs() < 1e-5);
        let s = sh.to_sample();
        assert!((s.direction - Vec3::NEG_Y).length() < 1e-5, "light goes down: {:?}", s.direction);
        // ambient + directional reproduces the value facing the light
        assert!((s.ambient.r + s.color.r - 0.75).abs() < 1e-4);
    }

    #[test]
    fn probes_capture_baked_lights_and_blend_by_distance() {
        let mut w = World::new();
        w.spawn((RenderSettings { ambient: Color::rgb(0.1, 0.1, 0.1), ..Default::default() },));
        w.spawn((Transform::default(), GlobalTransform::default(), LightProbeGroup { positions: vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0)] }));
        // a red baked light right above the first probe, out of reach of the second
        let light = Light { kind: LightKind::Point, position: Vec3::new(0.0, 2.0, 0.0), direction: Vec3::NEG_Z, color: Color::rgb(1.0, 0.0, 0.0), range: 5.0 };
        let _ = (PointLight { mode: LightMode::Baked, ..Default::default() }, ());
        let f = ProbeField::capture(&w, &[light]).unwrap();
        let near = f.sample_sh(Vec3::ZERO);
        let far = f.sample_sh(Vec3::new(10.0, 0.0, 0.0));
        assert!(near.eval(Vec3::Y).x > 0.3, "lit from above: {:?}", near.eval(Vec3::Y));
        assert!((far.eval(Vec3::Y) - Vec3::splat(0.1)).length() < 1e-5, "only ambient: {:?}", far.eval(Vec3::Y));
        let mid = f.sample_sh(Vec3::new(5.0, 0.0, 0.0));
        assert!((mid.a.x - (near.a.x + far.a.x) * 0.5).abs() < 1e-4, "halfway blend");
    }

    #[test]
    fn reflection_probe_boxes_pick_importance_then_size() {
        let mut w = World::new();
        use goat_gfx::GraphicsBackend;
        let mut gfx = goat_gfx::NullBackend::new();
        let mut tex = || gfx.create_texture(&goat_gfx::TextureData::solid(Color::WHITE)).unwrap();
        let (a, b, c) = (tex(), tex(), tex());
        let at = |x: f32| (Transform::from_position(Vec3::new(x, 0.0, 0.0)), GlobalTransform(goat_core::math::Mat4::from_translation(Vec3::new(x, 0.0, 0.0))));
        w.spawn((at(0.0).0, at(0.0).1, ReflectionProbe { size: Vec3::splat(20.0), cubemap: Some(a), ..Default::default() }));
        w.spawn((at(0.0).0, at(0.0).1, ReflectionProbe { size: Vec3::splat(4.0), cubemap: Some(b), ..Default::default() }));
        w.spawn((at(8.0).0, at(8.0).1, ReflectionProbe { size: Vec3::splat(4.0), cubemap: Some(c), importance: 5, ..Default::default() }));
        let boxes = reflection_boxes(&w);
        assert_eq!(reflection_at(&boxes, Vec3::ZERO), Some(b), "the smaller box");
        assert_eq!(reflection_at(&boxes, Vec3::new(5.0, 0.0, 0.0)), Some(a));
        assert_eq!(reflection_at(&boxes, Vec3::new(8.0, 0.0, 0.0)), Some(c), "more important");
        assert_eq!(reflection_at(&boxes, Vec3::new(50.0, 0.0, 0.0)), None);
    }
}
