//! Joints (Unity's Fixed / Hinge / Character / Configurable / Spring Joint): connect this entity's
//! body to another body (`connected`, by name) or to the world.

use std::collections::HashMap;

use goat_core::math::Vec3;
use goat_runtime::{Entity, Name, World};
use rapier3d::na::{Matrix3, Rotation3, UnitQuaternion};
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{v, Physics};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JointKind {
    /// Glued together (Fixed Joint).
    Fixed,
    /// Turns around `axis` (Hinge Joint): optional limits in degrees, optional motor (degrees /
    /// second, max force).
    Hinge {
        axis: Vec3,
        #[serde(default)]
        limits: Option<[f32; 2]>,
        #[serde(default)]
        motor_speed: f32,
        #[serde(default)]
        motor_force: f32,
    },
    /// Turns freely around the anchor (ball and socket, Character Joint without limits).
    Ball,
    /// Slides along `axis` (Configurable Joint with one linear axis): optional limits in units.
    Slider {
        axis: Vec3,
        #[serde(default)]
        limits: Option<[f32; 2]>,
    },
    /// Pulled back to `rest_length` (Spring Joint).
    Spring { rest_length: f32, stiffness: f32, damping: f32 },
    /// At most `max_length` apart (a rope / chain).
    Rope { max_length: f32 },
    /// A ball and socket with limits (Unity's Character Joint, ragdolls): twist around `axis`
    /// between `low_twist` and `high_twist`, swing ±`swing1` around `swing_axis` and ±`swing2`
    /// around the third axis (degrees; 180 = free).
    Character {
        axis: Vec3,
        swing_axis: Vec3,
        #[serde(default = "minus_twist")]
        low_twist: f32,
        #[serde(default = "plus_twist")]
        high_twist: f32,
        #[serde(default = "swing")]
        swing1: f32,
        #[serde(default = "swing")]
        swing2: f32,
    },
    /// Every axis chosen one by one (Unity's Configurable Joint).
    Configurable(ConfigurableJoint),
}

fn minus_twist() -> f32 {
    -20.0
}
fn plus_twist() -> f32 {
    70.0
}
fn swing() -> f32 {
    40.0
}

/// How a Configurable Joint axis moves (Unity's ConfigurableJointMotion).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Motion {
    #[default]
    Locked,
    Limited,
    Free,
}

/// Unity's Configurable Joint: the joint frame is `axis` (x) and `secondary_axis` (y); each linear
/// and angular axis is locked, limited or free; drives pull toward a target position / rotation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigurableJoint {
    pub axis: Vec3,
    pub secondary_axis: Vec3,
    pub x_motion: Motion,
    pub y_motion: Motion,
    pub z_motion: Motion,
    pub angular_x_motion: Motion,
    pub angular_y_motion: Motion,
    pub angular_z_motion: Motion,
    /// Limited linear axes: −limit..limit (units).
    pub linear_limit: f32,
    /// Limited angular x: low..high (degrees).
    pub low_angular_x_limit: f32,
    pub high_angular_x_limit: f32,
    /// Limited angular y / z: ±limit (degrees).
    pub angular_y_limit: f32,
    pub angular_z_limit: f32,
    /// Linear drive (free / limited axes): target in the joint frame, spring, damper.
    pub target_position: Vec3,
    pub position_spring: f32,
    pub position_damper: f32,
    /// Angular drive: target angles around x / y / z of the joint frame (degrees).
    pub target_rotation: Vec3,
    pub rotation_spring: f32,
    pub rotation_damper: f32,
}

impl Default for ConfigurableJoint {
    fn default() -> Self {
        ConfigurableJoint {
            axis: Vec3::X,
            secondary_axis: Vec3::Y,
            x_motion: Motion::Free,
            y_motion: Motion::Free,
            z_motion: Motion::Free,
            angular_x_motion: Motion::Free,
            angular_y_motion: Motion::Free,
            angular_z_motion: Motion::Free,
            linear_limit: 1.0,
            low_angular_x_limit: -45.0,
            high_angular_x_limit: 45.0,
            angular_y_limit: 45.0,
            angular_z_limit: 45.0,
            target_position: Vec3::ZERO,
            position_spring: 0.0,
            position_damper: 0.0,
            target_rotation: Vec3::ZERO,
            rotation_spring: 0.0,
            rotation_damper: 0.0,
        }
    }
}

/// The joint frame: x = `axis`, y = `secondary` made perpendicular, z = x × y.
fn frame(axis: Vec3, secondary: Vec3) -> UnitQuaternion<Real> {
    let x = axis.normalize_or(Vec3::X);
    let mut y = secondary - x * secondary.dot(x);
    if y.length_squared() < 1e-8 {
        y = if x.x.abs() < 0.9 { Vec3::X.cross(x) } else { Vec3::Y.cross(x) };
    }
    let y = y.normalize();
    let z = x.cross(y);
    let m = Matrix3::new(x.x, y.x, z.x, x.y, y.y, z.y, x.z, y.z, z.z);
    UnitQuaternion::from_rotation_matrix(&Rotation3::from_matrix_unchecked(m))
}

/// A joint on the entity holding the first body.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Joint {
    pub kind: JointKind,
    /// Name of the entity with the other body; empty = attached to the world.
    pub connected: String,
    /// Anchor in this body's local space.
    pub anchor: Vec3,
    /// Anchor in the connected body's local space (the world when not connected).
    pub connected_anchor: Vec3,
    /// Let the two bodies collide with each other (Unity's Enable Collision).
    pub enable_collision: bool,
}

impl Default for Joint {
    fn default() -> Self {
        Joint { kind: JointKind::Fixed, connected: String::new(), anchor: Vec3::ZERO, connected_anchor: Vec3::ZERO, enable_collision: false }
    }
}

impl Joint {
    pub fn new(kind: JointKind) -> Self {
        Joint { kind, ..Default::default() }
    }
    pub fn to(mut self, connected: &str) -> Self {
        self.connected = connected.into();
        self
    }
    pub fn at(mut self, anchor: Vec3, connected_anchor: Vec3) -> Self {
        self.anchor = anchor;
        self.connected_anchor = connected_anchor;
        self
    }
    /// A hinge around `axis`.
    pub fn hinge(axis: Vec3) -> Self {
        Self::new(JointKind::Hinge { axis, limits: None, motor_speed: 0.0, motor_force: 0.0 })
    }

    fn build(&self) -> GenericJoint {
        let p = |x: Vec3| point![x.x, x.y, x.z];
        let axis = |a: Vec3| UnitVector::new_normalize(v(a.normalize_or(Vec3::Y)));
        let mut j: GenericJoint = match self.kind {
            JointKind::Fixed => FixedJointBuilder::new().local_anchor1(p(self.anchor)).local_anchor2(p(self.connected_anchor)).into(),
            JointKind::Ball => SphericalJointBuilder::new().local_anchor1(p(self.anchor)).local_anchor2(p(self.connected_anchor)).into(),
            JointKind::Hinge { axis: a, limits, motor_speed, motor_force } => {
                let mut b = RevoluteJointBuilder::new(axis(a)).local_anchor1(p(self.anchor)).local_anchor2(p(self.connected_anchor));
                if let Some([lo, hi]) = limits {
                    b = b.limits([lo.to_radians(), hi.to_radians()]);
                }
                if motor_force > 0.0 {
                    b = b.motor_velocity(motor_speed.to_radians(), motor_force).motor_max_force(motor_force);
                }
                b.into()
            }
            JointKind::Slider { axis: a, limits } => {
                let mut b = PrismaticJointBuilder::new(axis(a)).local_anchor1(p(self.anchor)).local_anchor2(p(self.connected_anchor));
                if let Some(l) = limits {
                    b = b.limits(l);
                }
                b.into()
            }
            JointKind::Spring { rest_length, stiffness, damping } => SpringJointBuilder::new(rest_length, stiffness, damping).local_anchor1(p(self.anchor)).local_anchor2(p(self.connected_anchor)).into(),
            JointKind::Rope { max_length } => RopeJointBuilder::new(max_length).local_anchor1(p(self.anchor)).local_anchor2(p(self.connected_anchor)).into(),
            JointKind::Character { axis: a, swing_axis, low_twist, high_twist, swing1, swing2 } => {
                let r = frame(a, swing_axis);
                let mut b = GenericJointBuilder::new(JointAxesMask::LIN_AXES)
                    .local_frame1(Isometry::from_parts(p(self.anchor).coords.into(), r))
                    .local_frame2(Isometry::from_parts(p(self.connected_anchor).coords.into(), r));
                let lim = |d: f32| d.clamp(-180.0, 180.0).to_radians();
                if high_twist - low_twist < 360.0 {
                    b = b.limits(JointAxis::AngX, [lim(low_twist.min(high_twist)), lim(high_twist.max(low_twist))]);
                }
                if swing1 < 180.0 {
                    b = b.limits(JointAxis::AngY, [-lim(swing1.abs()), lim(swing1.abs())]);
                }
                if swing2 < 180.0 {
                    b = b.limits(JointAxis::AngZ, [-lim(swing2.abs()), lim(swing2.abs())]);
                }
                b.into()
            }
            JointKind::Configurable(c) => {
                let r = frame(c.axis, c.secondary_axis);
                let mut locked = JointAxesMask::empty();
                let axes = [
                    (c.x_motion, JointAxesMask::LIN_X, JointAxis::LinX),
                    (c.y_motion, JointAxesMask::LIN_Y, JointAxis::LinY),
                    (c.z_motion, JointAxesMask::LIN_Z, JointAxis::LinZ),
                    (c.angular_x_motion, JointAxesMask::ANG_X, JointAxis::AngX),
                    (c.angular_y_motion, JointAxesMask::ANG_Y, JointAxis::AngY),
                    (c.angular_z_motion, JointAxesMask::ANG_Z, JointAxis::AngZ),
                ];
                for (m, mask, _) in axes {
                    if m == Motion::Locked {
                        locked |= mask;
                    }
                }
                let mut b = GenericJointBuilder::new(locked)
                    .local_frame1(Isometry::from_parts(p(self.anchor).coords.into(), r))
                    .local_frame2(Isometry::from_parts(p(self.connected_anchor).coords.into(), r));
                for (i, (m, _, axis)) in axes.into_iter().enumerate() {
                    if m == Motion::Locked {
                        continue;
                    }
                    if m == Motion::Limited {
                        let l = match i {
                            0..=2 => [-c.linear_limit.abs(), c.linear_limit.abs()],
                            3 => [c.low_angular_x_limit.min(c.high_angular_x_limit).to_radians(), c.high_angular_x_limit.max(c.low_angular_x_limit).to_radians()],
                            4 => [-c.angular_y_limit.abs().to_radians(), c.angular_y_limit.abs().to_radians()],
                            _ => [-c.angular_z_limit.abs().to_radians(), c.angular_z_limit.abs().to_radians()],
                        };
                        b = b.limits(axis, l);
                    }
                    // drives: springs toward the target (Unity's X / Y / Z Drive, Angular Drive)
                    let (target, spring, damper) = match i {
                        0 => (c.target_position.x, c.position_spring, c.position_damper),
                        1 => (c.target_position.y, c.position_spring, c.position_damper),
                        2 => (c.target_position.z, c.position_spring, c.position_damper),
                        3 => (c.target_rotation.x.to_radians(), c.rotation_spring, c.rotation_damper),
                        4 => (c.target_rotation.y.to_radians(), c.rotation_spring, c.rotation_damper),
                        _ => (c.target_rotation.z.to_radians(), c.rotation_spring, c.rotation_damper),
                    };
                    if spring > 0.0 || damper > 0.0 {
                        b = b.motor_model(axis, MotorModel::ForceBased).motor_position(axis, target, spring, damper);
                    }
                }
                b.into()
            }
        };
        j.set_contacts_enabled(self.enable_collision);
        j
    }
}

/// Joints created from components: entity → (joint handle, the component it was built from).
#[derive(Default)]
pub(crate) struct Joints(HashMap<Entity, (ImpulseJointHandle, String, JointKind)>);

impl Physics {
    /// Creates / removes Rapier joints for [`Joint`] components (both bodies must exist).
    pub(crate) fn sync_joints(&mut self, world: &World) {
        // gone or changed
        let stale: Vec<Entity> = self
            .joints
            .0
            .iter()
            .filter(|(e, (h, connected, kind))| match world.get::<&Joint>(**e) {
                // (removing a body removes its joints too)
                Ok(j) => j.connected != *connected || j.kind != *kind || !self.tracked.contains_key(e) || self.impulse_joints.get(*h).is_none(),
                Err(_) => true,
            })
            .map(|(e, _)| *e)
            .collect();
        for e in stale {
            if let Some((h, _, _)) = self.joints.0.remove(&e) {
                self.impulse_joints.remove(h, true);
            }
        }
        let wanted: Vec<(Entity, Joint)> = world.query::<&Joint>().iter().filter(|(e, _)| !self.joints.0.contains_key(e) && self.tracked.contains_key(e)).map(|(e, j)| (e, j.clone())).collect();
        for (e, j) in wanted {
            let body1 = self.tracked[&e].body;
            let body2 = if j.connected.is_empty() {
                *self.world_anchor.get_or_insert_with(|| self.bodies.insert(RigidBodyBuilder::fixed().build()))
            } else {
                let other = world.query::<&Name>().iter().find(|(o, n)| n.0 == j.connected && *o != e).map(|(o, _)| o);
                match other.and_then(|o| self.tracked.get(&o)) {
                    Some(t) => t.body,
                    None => continue, // not simulated yet: next step
                }
            };
            let h = self.impulse_joints.insert(body1, body2, j.build(), true);
            self.joints.0.insert(e, (h, j.connected.clone(), j.kind));
        }
    }

    /// Number of live joints (tests, stats).
    pub fn joint_count(&self) -> usize {
        self.impulse_joints.len()
    }
}
