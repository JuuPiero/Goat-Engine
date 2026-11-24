//! Joints (Unity's Fixed / Hinge / Character / Configurable / Spring Joint): connect this entity's
//! body to another body (`connected`, by name) or to the world.

use std::collections::HashMap;

use goat_core::math::Vec3;
use goat_runtime::{Entity, Name, World};
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
