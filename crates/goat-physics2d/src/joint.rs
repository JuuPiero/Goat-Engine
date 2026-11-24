//! 2D joints (Unity's FixedJoint2D, HingeJoint2D, SliderJoint2D, SpringJoint2D, DistanceJoint2D,
//! WheelJoint2D, FrictionJoint2D, RelativeJoint2D):
//! connect this entity's body to another (`connected`, by name) or to the world.

use std::collections::HashMap;

use goat_core::math::Vec2;
use goat_runtime::{Entity, Name, World};
use rapier2d::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{v, Physics2D};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JointKind2D {
    Fixed,
    /// Turns around the anchor: optional limits (degrees), optional motor (degrees / s, max torque).
    Hinge {
        #[serde(default)]
        limits: Option<[f32; 2]>,
        #[serde(default)]
        motor_speed: f32,
        #[serde(default)]
        motor_torque: f32,
    },
    /// Slides along `axis` (local), optional limits in units.
    Slider {
        axis: Vec2,
        #[serde(default)]
        limits: Option<[f32; 2]>,
    },
    Spring { rest_length: f32, stiffness: f32, damping: f32 },
    /// At most `max_distance` apart (DistanceJoint2D with Max Distance Only).
    Distance { max_distance: f32 },
    /// A car wheel (WheelJoint2D): free rotation (+ motor) on a suspension spring along
    /// `suspension_angle` (degrees, 90 = up) of the connected frame.
    Wheel {
        #[serde(default = "deg90")]
        suspension_angle: f32,
        /// Suspension spring frequency (Hz) and damping ratio (0..1).
        #[serde(default = "two")]
        frequency: f32,
        #[serde(default = "seven_tenths")]
        damping_ratio: f32,
        #[serde(default)]
        motor_speed: f32,
        #[serde(default)]
        motor_torque: f32,
    },
    /// Resists relative motion with at most `max_force` / `max_torque` (FrictionJoint2D: top-down
    /// games' floor friction).
    Friction { max_force: f32, max_torque: f32 },
    /// Keeps the bodies at `linear_offset` / `angular_offset` (degrees) from each other with at most
    /// `max_force` / `max_torque` (RelativeJoint2D).
    Relative {
        #[serde(default)]
        linear_offset: Vec2,
        #[serde(default)]
        angular_offset: f32,
        max_force: f32,
        max_torque: f32,
    },
}

fn deg90() -> f32 {
    90.0
}
fn two() -> f32 {
    2.0
}
fn seven_tenths() -> f32 {
    0.7
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Joint2D {
    pub kind: JointKind2D,
    /// Name of the entity with the other body; empty = the world.
    pub connected: String,
    pub anchor: Vec2,
    pub connected_anchor: Vec2,
    pub enable_collision: bool,
}

impl Default for Joint2D {
    fn default() -> Self {
        Joint2D { kind: JointKind2D::Fixed, connected: String::new(), anchor: Vec2::ZERO, connected_anchor: Vec2::ZERO, enable_collision: false }
    }
}

impl Joint2D {
    pub fn new(kind: JointKind2D) -> Self {
        Joint2D { kind, ..Default::default() }
    }
    pub fn to(mut self, connected: &str) -> Self {
        self.connected = connected.into();
        self
    }
    pub fn at(mut self, anchor: Vec2, connected_anchor: Vec2) -> Self {
        self.anchor = anchor;
        self.connected_anchor = connected_anchor;
        self
    }

    fn build(&self) -> GenericJoint {
        let p = |x: Vec2| point![x.x, x.y];
        let (a1, a2) = (p(self.anchor), p(self.connected_anchor));
        let mut j: GenericJoint = match self.kind {
            JointKind2D::Fixed => FixedJointBuilder::new().local_anchor1(a1).local_anchor2(a2).into(),
            JointKind2D::Hinge { limits, motor_speed, motor_torque } => {
                let mut b = RevoluteJointBuilder::new().local_anchor1(a1).local_anchor2(a2);
                if let Some([lo, hi]) = limits {
                    b = b.limits([lo.to_radians(), hi.to_radians()]);
                }
                if motor_torque > 0.0 {
                    b = b.motor_velocity(motor_speed.to_radians(), motor_torque).motor_max_force(motor_torque);
                }
                b.into()
            }
            JointKind2D::Slider { axis, limits } => {
                let mut b = PrismaticJointBuilder::new(UnitVector::new_normalize(v(axis.normalize_or(Vec2::X)))).local_anchor1(a1).local_anchor2(a2);
                if let Some(l) = limits {
                    b = b.limits(l);
                }
                b.into()
            }
            JointKind2D::Spring { rest_length, stiffness, damping } => SpringJointBuilder::new(rest_length, stiffness, damping).local_anchor1(a1).local_anchor2(a2).into(),
            JointKind2D::Distance { max_distance } => RopeJointBuilder::new(max_distance).local_anchor1(a1).local_anchor2(a2).into(),
            JointKind2D::Wheel { suspension_angle, frequency, damping_ratio, motor_speed, motor_torque } => {
                // joint frame X = the suspension axis: X free on a spring, Y locked, rotation free (+ motor)
                let frame1 = Isometry::new(v(self.anchor), suspension_angle.to_radians());
                let frame2 = Isometry::new(v(self.connected_anchor), suspension_angle.to_radians());
                let mut j = GenericJointBuilder::new(JointAxesMask::LIN_Y).local_frame1(frame1).local_frame2(frame2).build();
                // a spring of `frequency` Hz (acceleration-based: independent of the mass)
                let w = std::f32::consts::TAU * frequency.max(0.01);
                j.set_motor_model(JointAxis::LinX, MotorModel::AccelerationBased);
                j.set_motor(JointAxis::LinX, 0.0, 0.0, w * w, 2.0 * damping_ratio * w);
                if motor_torque > 0.0 {
                    j.set_motor_velocity(JointAxis::AngX, motor_speed.to_radians(), motor_torque);
                    j.set_motor_max_force(JointAxis::AngX, motor_torque);
                }
                j
            }
            JointKind2D::Friction { max_force, max_torque } => {
                let mut j = GenericJointBuilder::new(JointAxesMask::empty()).local_anchor1(a1).local_anchor2(a2).build();
                for (axis, max) in [(JointAxis::LinX, max_force), (JointAxis::LinY, max_force), (JointAxis::AngX, max_torque)] {
                    j.set_motor_model(axis, MotorModel::ForceBased);
                    j.set_motor_velocity(axis, 0.0, 1e4);
                    j.set_motor_max_force(axis, max.max(0.0));
                }
                j
            }
            JointKind2D::Relative { linear_offset, angular_offset, max_force, max_torque } => {
                let mut j = GenericJointBuilder::new(JointAxesMask::empty()).local_anchor1(a1).local_anchor2(a2).build();
                for (axis, target, max) in [(JointAxis::LinX, -linear_offset.x, max_force), (JointAxis::LinY, -linear_offset.y, max_force), (JointAxis::AngX, -angular_offset.to_radians(), max_torque)] {
                    j.set_motor_model(axis, MotorModel::AccelerationBased);
                    j.set_motor(axis, target, 0.0, 60.0, 12.0);
                    j.set_motor_max_force(axis, max.max(0.0));
                }
                j
            }
        };
        j.set_contacts_enabled(self.enable_collision);
        j
    }
}

#[derive(Default)]
pub(crate) struct Joints(HashMap<Entity, (ImpulseJointHandle, String, JointKind2D)>);

impl Physics2D {
    pub(crate) fn sync_joints(&mut self, world: &World) {
        let stale: Vec<Entity> = self
            .joints
            .0
            .iter()
            .filter(|(e, (h, connected, kind))| match world.get::<&Joint2D>(**e) {
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
        let wanted: Vec<(Entity, Joint2D)> = world.query::<&Joint2D>().iter().filter(|(e, _)| !self.joints.0.contains_key(e) && self.tracked.contains_key(e)).map(|(e, j)| (e, j.clone())).collect();
        for (e, j) in wanted {
            let body1 = self.tracked[&e].body;
            let body2 = if j.connected.is_empty() {
                *self.world_anchor.get_or_insert_with(|| self.bodies.insert(RigidBodyBuilder::fixed().build()))
            } else {
                let other = world.query::<&Name>().iter().find(|(o, n)| n.0 == j.connected && *o != e).map(|(o, _)| o);
                match other.and_then(|o| self.tracked.get(&o)) {
                    Some(t) => t.body,
                    None => continue,
                }
            };
            let h = self.impulse_joints.insert(body1, body2, j.build(), true);
            self.joints.0.insert(e, (h, j.connected.clone(), j.kind));
        }
    }

    pub fn joint_count(&self) -> usize {
        self.impulse_joints.len()
    }
}
