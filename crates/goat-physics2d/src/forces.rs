//! Forces from components (Unity's ConstantForce2D and 2D effectors), applied every fixed step
//! before the simulation step as impulses (force × step):
//!
//! - [`ConstantForce2D`]: a force (world or relative to the body's rotation) + a torque.
//! - [`AreaEffector2D`]: inside its collider, a force along an angle (+ random variation) and
//!   extra drag (wind, conveyors in the air, currents).
//! - [`PointEffector2D`]: attraction / repulsion toward its position (constant, inverse linear,
//!   inverse squared), within its collider.
//! - [`BuoyancyEffector2D`]: below `surface_level` (local Y of the effector) bodies float up
//!   (by density), are slowed (drag) and pushed by a flow.
//!
//! An effector acts on dynamic bodies whose center lies inside its entity's Collider2D (box /
//! circle / capsule, usually a trigger).

use goat_core::math::{Vec2, Vec3};
use goat_runtime::{Entity, GlobalTransform, Transform, World};
use serde::{Deserialize, Serialize};

use crate::{Collider2D, Physics2D, Shape2D};

/// A constant force on the body (Unity's ConstantForce2D).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConstantForce2D {
    /// Newtons, world axes.
    pub force: Vec2,
    /// Newtons, along the body's own axes.
    pub relative_force: Vec2,
    /// Newton·metres around Z.
    pub torque: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AreaEffector2D {
    /// Degrees (0 = +X); relative to the effector's rotation unless `use_global_angle`.
    pub force_angle: f32,
    pub use_global_angle: bool,
    pub force_magnitude: f32,
    /// Random ± added to the magnitude each step.
    pub force_variation: f32,
    pub drag: f32,
    pub angular_drag: f32,
}

impl Default for AreaEffector2D {
    fn default() -> Self {
        AreaEffector2D { force_angle: 0.0, use_global_angle: false, force_magnitude: 0.0, force_variation: 0.0, drag: 0.0, angular_drag: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectorForceMode {
    #[default]
    Constant,
    InverseLinear,
    InverseSquared,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PointEffector2D {
    /// Positive pushes away, negative attracts.
    pub force_magnitude: f32,
    pub force_variation: f32,
    /// Distances are multiplied by this before the falloff.
    pub distance_scale: f32,
    pub force_mode: EffectorForceMode,
    pub drag: f32,
    pub angular_drag: f32,
}

impl Default for PointEffector2D {
    fn default() -> Self {
        PointEffector2D { force_magnitude: 0.0, force_variation: 0.0, distance_scale: 1.0, force_mode: EffectorForceMode::Constant, drag: 0.0, angular_drag: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BuoyancyEffector2D {
    /// Local Y of the fluid surface (relative to the effector).
    pub surface_level: f32,
    /// Fluid density: bodies lighter than it float.
    pub density: f32,
    pub linear_drag: f32,
    pub angular_drag: f32,
    /// Degrees; a current pushing submerged bodies.
    pub flow_angle: f32,
    pub flow_magnitude: f32,
}

impl Default for BuoyancyEffector2D {
    fn default() -> Self {
        BuoyancyEffector2D { surface_level: 0.0, density: 2.0, linear_drag: 1.0, angular_drag: 0.1, flow_angle: 0.0, flow_magnitude: 0.0 }
    }
}

/// One-way platforms (Unity's PlatformEffector2D): solid only from the side within `surface_arc`
/// (degrees, centred on the effector's local up turned by `rotational_offset`); bodies coming
/// from elsewhere (jumping up through it) pass. Contacts are filtered in the solver (lib.rs hooks).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PlatformEffector2D {
    pub use_one_way: bool,
    pub surface_arc: f32,
    pub rotational_offset: f32,
}

impl Default for PlatformEffector2D {
    fn default() -> Self {
        PlatformEffector2D { use_one_way: true, surface_arc: 180.0, rotational_offset: 0.0 }
    }
}

/// Conveyor surfaces (Unity's SurfaceEffector2D): bodies touching the collider are carried along
/// its local +X at `speed` (+ random `speed_variation`), friction-driven like Unity's.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SurfaceEffector2D {
    pub speed: f32,
    pub speed_variation: f32,
}

/// Pulls the body's anchor toward a world target with a spring (Unity's TargetJoint2D:
/// dragging objects with the mouse / a finger).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TargetJoint2D {
    pub target: Vec2,
    /// Spring frequency (Hz).
    pub frequency: f32,
    /// 0..1 (1 = critically damped).
    pub damping_ratio: f32,
    /// Newtons.
    pub max_force: f32,
}

impl Default for TargetJoint2D {
    fn default() -> Self {
        TargetJoint2D { target: Vec2::ZERO, frequency: 5.0, damping_ratio: 0.7, max_force: 1000.0 }
    }
}

/// Is a world point inside an entity's collider (2D, its world transform)?
fn inside(c: &Collider2D, m: &goat_core::math::Mat4, p: Vec2) -> bool {
    let local = m.inverse().transform_point3(Vec3::new(p.x, p.y, 0.0));
    let l = Vec2::new(local.x, local.y) - c.offset;
    match &c.shape {
        Shape2D::Box(h) => l.x.abs() <= h.x && l.y.abs() <= h.y,
        Shape2D::Circle(r) => l.length() <= *r,
        Shape2D::Capsule { half_height, radius } => {
            let y = l.y.clamp(-half_height, *half_height);
            (l - Vec2::new(0.0, y)).length() <= *radius
        }
        _ => false,
    }
}

/// Small deterministic noise for variations (same on every platform).
fn noise(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
}

/// Applies the forces of one fixed step (`dt`).
pub fn apply(world: &World, p: &mut Physics2D, dt: f32, seed: &mut u32) {
    // constant forces
    for (e, (f, t)) in world.query::<(&ConstantForce2D, &Transform)>().without::<&goat_runtime::InactiveInHierarchy>().iter() {
        let (_, _, angle) = t.rotation.to_euler(goat_core::math::EulerRot::XYZ);
        let rel = Vec2::new(f.relative_force.x * angle.cos() - f.relative_force.y * angle.sin(), f.relative_force.x * angle.sin() + f.relative_force.y * angle.cos());
        let total = f.force + rel;
        if total != Vec2::ZERO {
            p.apply_impulse(e, total * dt);
        }
        if f.torque != 0.0 {
            p.apply_torque_impulse(e, f.torque * dt);
        }
    }
    // target joints: a damped spring toward the target, force clamped
    let targets: Vec<(Entity, TargetJoint2D)> = world.query::<&TargetJoint2D>().without::<&goat_runtime::InactiveInHierarchy>().iter().map(|(e, j)| (e, *j)).collect();
    for (e, j) in targets {
        let Some((pos, vel, mass, _)) = p.body_state(e) else { continue };
        let w = std::f32::consts::TAU * j.frequency.max(0.01);
        let (k, c) = (mass * w * w, 2.0 * mass * j.damping_ratio * w);
        let mut f = (j.target - pos) * k - vel * c;
        if f.length() > j.max_force {
            f = f.normalize() * j.max_force;
        }
        // + cancel gravity like Unity's joint (the target holds the body)
        p.apply_impulse(e, (f - p.gravity * mass) * dt);
    }
    let areas: Vec<(Collider2D, goat_core::math::Mat4, AreaEffector2D)> = world.query::<(&Collider2D, &GlobalTransform, &AreaEffector2D)>().without::<&goat_runtime::InactiveInHierarchy>().iter().map(|(_, (c, g, a))| (c.clone(), g.0, *a)).collect();
    let points: Vec<(Collider2D, goat_core::math::Mat4, PointEffector2D)> = world.query::<(&Collider2D, &GlobalTransform, &PointEffector2D)>().without::<&goat_runtime::InactiveInHierarchy>().iter().map(|(_, (c, g, a))| (c.clone(), g.0, *a)).collect();
    let fluids: Vec<(Collider2D, goat_core::math::Mat4, BuoyancyEffector2D)> = world.query::<(&Collider2D, &GlobalTransform, &BuoyancyEffector2D)>().without::<&goat_runtime::InactiveInHierarchy>().iter().map(|(_, (c, g, a))| (c.clone(), g.0, *a)).collect();
    if areas.is_empty() && points.is_empty() && fluids.is_empty() {
        return;
    }
    let bodies: Vec<Entity> = p.dynamic_bodies();
    for e in bodies {
        let Some((pos, vel, mass, angvel)) = p.body_state(e) else { continue };
        let mut force = Vec2::ZERO;
        let (mut drag, mut ang_drag) = (0.0f32, 0.0f32);
        for (c, m, a) in &areas {
            if !inside(c, m, pos) {
                continue;
            }
            let base = if a.use_global_angle { 0.0 } else { m.to_scale_rotation_translation().1.to_euler(goat_core::math::EulerRot::XYZ).2 };
            let ang = base + a.force_angle.to_radians();
            let mag = a.force_magnitude + a.force_variation * noise(seed);
            force += Vec2::new(ang.cos(), ang.sin()) * mag;
            drag += a.drag;
            ang_drag += a.angular_drag;
        }
        for (c, m, pe) in &points {
            if !inside(c, m, pos) {
                continue;
            }
            let center = Vec2::new(m.w_axis.x, m.w_axis.y);
            let d = pos - center;
            let dist = (d.length() * pe.distance_scale).max(1e-3);
            let mag = pe.force_magnitude + pe.force_variation * noise(seed);
            let falloff = match pe.force_mode {
                EffectorForceMode::Constant => 1.0,
                EffectorForceMode::InverseLinear => 1.0 / dist,
                EffectorForceMode::InverseSquared => 1.0 / (dist * dist),
            };
            force += d.normalize_or_zero() * mag * falloff;
            drag += pe.drag;
            ang_drag += pe.angular_drag;
        }
        for (c, m, b) in &fluids {
            if !inside(c, m, pos) {
                continue;
            }
            let surface = m.transform_point3(Vec3::new(0.0, b.surface_level, 0.0)).y;
            if pos.y > surface {
                continue;
            }
            // Archimedes with bodies of density 1: lift = fluid density × g × mass × the submerged
            // share (ramped over the last half unit: no bouncing at the surface)
            let g = p.gravity.length();
            let submerged = ((surface - pos.y) / 0.5).clamp(0.0, 1.0);
            force += -p.gravity.normalize_or(Vec2::NEG_Y) * b.density * g * mass * submerged;
            let flow = b.flow_angle.to_radians();
            force += Vec2::new(flow.cos(), flow.sin()) * b.flow_magnitude;
            drag += b.linear_drag;
            ang_drag += b.angular_drag;
        }
        let impulse = force * dt - vel * (drag * mass * dt).min(mass);
        if impulse != Vec2::ZERO {
            p.apply_impulse(e, impulse);
        }
        if ang_drag > 0.0 {
            p.apply_torque_impulse(e, -angvel * (ang_drag * dt).min(1.0) * mass * 0.1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rigidbody2D;

    fn run(w: &mut World, seconds: f32) {
        let mut p = Physics2D::default();
        let mut seed = 1;
        let dt = 1.0 / 60.0;
        for _ in 0..(seconds / dt) as usize {
            goat_runtime::engine::propagate_transforms(w);
            apply(w, &mut p, dt, &mut seed);
            p.step(w, dt);
        }
    }
    fn ball(w: &mut World, at: Vec3) -> Entity {
        w.spawn((Transform::from_position(at), GlobalTransform::default(), Collider2D::circle(0.25), Rigidbody2D::dynamic()))
    }
    fn zone<C: hecs::Component>(w: &mut World, at: Vec3, half: Vec2, c: C) {
        w.spawn((Transform::from_position(at), GlobalTransform::default(), Collider2D::new(Shape2D::Box(half)).trigger(), c));
    }
    fn pos(w: &World, e: Entity) -> Vec3 {
        w.get::<&Transform>(e).unwrap().position
    }

    #[test]
    fn constant_force_and_area_wind() {
        let mut w = World::new();
        // a ball pushed up harder than gravity rises; one in a sideways wind drifts right
        let up = ball(&mut w, Vec3::new(-5.0, 0.0, 0.0));
        w.insert_one(up, ConstantForce2D { force: Vec2::new(0.0, 30.0), ..Default::default() }).unwrap();
        let blown = ball(&mut w, Vec3::new(5.0, 0.0, 0.0));
        zone(&mut w, Vec3::new(5.0, 0.0, 0.0), Vec2::new(10.0, 10.0), AreaEffector2D { force_magnitude: 20.0, ..Default::default() });
        run(&mut w, 1.0);
        assert!(pos(&w, up).y > 1.0, "{}", pos(&w, up));
        assert!(pos(&w, blown).x > 6.0, "{}", pos(&w, blown));
    }

    #[test]
    fn target_joint_holds_a_body_at_its_target() {
        let mut w = World::new();
        let b = ball(&mut w, Vec3::new(0.0, 0.0, 0.0));
        w.insert_one(b, TargetJoint2D { target: Vec2::new(3.0, 2.0), ..Default::default() }).unwrap();
        run(&mut w, 2.0);
        let p = pos(&w, b);
        assert!((p.x - 3.0).abs() < 0.1 && (p.y - 2.0).abs() < 0.1, "{p}");
    }

    #[test]
    fn point_attraction_and_buoyancy() {
        let mut w = World::new();
        // a magnet at the origin pulls a ball 2 units away toward it (gravity cancelled by a force)
        let b = ball(&mut w, Vec3::new(2.0, 0.0, 0.0));
        w.insert_one(b, ConstantForce2D { force: Vec2::new(0.0, 9.81 * 0.196), ..Default::default() }).unwrap();
        zone(&mut w, Vec3::ZERO, Vec2::new(5.0, 5.0), PointEffector2D { force_magnitude: -10.0, ..Default::default() });
        run(&mut w, 0.5);
        assert!(pos(&w, b).x < 1.5, "{}", pos(&w, b));
        // water up to y = 0 at x = 20: a dense fluid floats the ball near the surface, a thin one lets it sink
        let mut w = World::new();
        let floats = ball(&mut w, Vec3::new(20.0, -2.0, 0.0));
        zone(&mut w, Vec3::new(20.0, -5.0, 0.0), Vec2::new(3.0, 5.0), BuoyancyEffector2D { surface_level: 5.0, density: 2.0, ..Default::default() });
        let sinks = ball(&mut w, Vec3::new(40.0, -2.0, 0.0));
        zone(&mut w, Vec3::new(40.0, -5.0, 0.0), Vec2::new(3.0, 5.0), BuoyancyEffector2D { surface_level: 5.0, density: 0.3, ..Default::default() });
        run(&mut w, 3.0);
        let (yf, ys) = (pos(&w, floats).y, pos(&w, sinks).y);
        assert!(yf > -0.6 && yf < 0.6, "floats at the surface: {yf}");
        assert!(ys < -4.0, "sinks: {ys}");
    }

    #[test]
    fn one_way_platform_and_conveyor() {
        let mut w = World::new();
        // a ball thrown up from below passes through the platform and lands on it
        w.spawn((Transform::from_position(Vec3::new(0.0, 0.0, 0.0)), GlobalTransform::default(), Collider2D::new(Shape2D::Box(Vec2::new(2.0, 0.1))), PlatformEffector2D::default()));
        let up = ball(&mut w, Vec3::new(0.0, -1.0, 0.0));
        w.get::<&mut Rigidbody2D>(up).unwrap().velocity = Vec2::new(0.0, 8.0);
        // without the effector the same platform stops it from below
        w.spawn((Transform::from_position(Vec3::new(10.0, 0.0, 0.0)), GlobalTransform::default(), Collider2D::new(Shape2D::Box(Vec2::new(2.0, 0.1)))));
        let blocked = ball(&mut w, Vec3::new(10.0, -1.0, 0.0));
        w.get::<&mut Rigidbody2D>(blocked).unwrap().velocity = Vec2::new(0.0, 8.0);
        // a box on a conveyor moving +X
        w.spawn((Transform::from_position(Vec3::new(30.0, -0.5, 0.0)), GlobalTransform::default(), Collider2D::new(Shape2D::Box(Vec2::new(10.0, 0.5))), SurfaceEffector2D { speed: 3.0, speed_variation: 0.0 }));
        let crate_ = w.spawn((Transform::from_position(Vec3::new(30.0, 0.25, 0.0)), GlobalTransform::default(), Collider2D::new(Shape2D::Box(Vec2::new(0.25, 0.25))), Rigidbody2D::dynamic()));
        run(&mut w, 2.5);
        let y = pos(&w, up).y;
        assert!(y > 0.2 && y < 0.5, "landed on top of the one-way platform: {y}");
        assert!(pos(&w, blocked).y < 0.0, "a normal platform blocks from below: {}", pos(&w, blocked).y);
        assert!(pos(&w, crate_).x > 33.0, "carried by the conveyor: {}", pos(&w, crate_).x);
    }

    #[test]
    fn wheel_friction_and_relative_joints() {
        use crate::{Joint2D, JointKind2D};
        let mut w = World::new();
        // wheel on a suspension to the world: sags a bit, spins at the motor speed
        let wheel = ball(&mut w, Vec3::new(0.0, 0.0, 0.0));
        w.insert_one(wheel, Joint2D::new(JointKind2D::Wheel { suspension_angle: 90.0, frequency: 2.0, damping_ratio: 0.7, motor_speed: 180.0, motor_torque: 1000.0 })).unwrap();
        // friction joint to the world: a sliding body stops (no gravity on it)
        let slide = ball(&mut w, Vec3::new(10.0, 0.0, 0.0));
        {
            let mut rb = w.get::<&mut Rigidbody2D>(slide).unwrap();
            rb.velocity = Vec2::new(5.0, 0.0);
            rb.gravity_scale = 0.0;
        }
        w.insert_one(slide, Joint2D::new(JointKind2D::Friction { max_force: 1.0, max_torque: 1.0 }).at(Vec2::ZERO, Vec2::new(10.0, 0.0))).unwrap();
        // relative joint: held at an offset from the world anchor
        let held = ball(&mut w, Vec3::new(20.0, 0.0, 0.0));
        w.get::<&mut Rigidbody2D>(held).unwrap().gravity_scale = 0.0;
        w.insert_one(held, Joint2D::new(JointKind2D::Relative { linear_offset: Vec2::new(3.0, 0.0), angular_offset: 0.0, max_force: 1000.0, max_torque: 1000.0 }).at(Vec2::ZERO, Vec2::new(20.0, 0.0))).unwrap();
        run(&mut w, 3.0);
        let p = pos(&w, wheel);
        assert!(p.x.abs() < 0.05 && p.y < 0.0 && p.y > -0.3, "wheel on its suspension: {p}");
        let sx = pos(&w, slide).x;
        // 1 N on ~0.2 kg: ≈ 5 m/s² → stops after ≈ 2.5 m
        assert!(sx > 11.0 && sx < 14.0, "friction stopped the slide: {sx}");
        let hx = pos(&w, held).x;
        assert!(((hx - 20.0).abs() - 3.0).abs() < 0.1, "held 3 units from the anchor: {hx}");
    }
}