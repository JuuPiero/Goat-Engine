//! 2D joints (Unity's FixedJoint2D, HingeJoint2D, SliderJoint2D, SpringJoint2D, DistanceJoint2D):
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
