//! Character controller (Unity's CharacterController): a capsule moved by code that slides along
//! walls, climbs steps and gentle slopes, and knows when it is on the ground.

use goat_core::math::Vec3;
use goat_runtime::{Entity, Parent, Transform, World};
use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{gv, iso, v, world_matrix, Collider, Physics, Shape};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterController {
    /// Capsule height (ends included) and radius, centered on `center`.
    pub height: f32,
    pub radius: f32,
    pub center: Vec3,
    /// Highest step climbed without jumping.
    pub step_offset: f32,
    /// Steepest slope walked up, degrees.
    pub slope_limit: f32,
    /// Gap kept around the capsule (Unity's Skin Width).
    pub skin_width: f32,
    pub layer: u8,
    /// Standing on something after the last move.
    #[serde(skip)]
    pub is_grounded: bool,
    /// What the last move touched.
    #[serde(skip)]
    pub collision_flags: CollisionFlags,
    /// Movement actually done by the last move, per second (Unity's velocity).
    #[serde(skip)]
    pub velocity: Vec3,
}

impl Default for CharacterController {
    fn default() -> Self {
        CharacterController { height: 2.0, radius: 0.5, center: Vec3::ZERO, step_offset: 0.3, slope_limit: 45.0, skin_width: 0.02, layer: 0, is_grounded: false, collision_flags: CollisionFlags::default(), velocity: Vec3::ZERO }
    }
}

/// Unity's CollisionFlags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CollisionFlags {
    pub below: bool,
    pub sides: bool,
    pub above: bool,
}

impl CharacterController {
    /// The capsule as a collider (what other bodies collide with).
    pub(crate) fn collider(&self) -> Collider {
        let half = (self.height * 0.5 - self.radius).max(0.0);
        Collider { center: self.center, layer: self.layer, ..Collider::new(Shape::Capsule { half_height: half, radius: self.radius }) }
    }
}

impl Physics {
    /// Moves a character by `motion` (world units, e.g. `velocity * dt`): slides along walls,
    /// climbs steps / slopes within its limits, snaps down onto the ground when walking. Writes the
    /// Transform and the controller's `is_grounded` / `collision_flags` / `velocity`. Returns the
    /// movement done (Unity's `controller.Move`).
    pub fn move_character(&mut self, world: &mut World, e: Entity, motion: Vec3, dt: f32) -> Vec3 {
        let Ok(cc) = world.get::<&CharacterController>(e).map(|c| *c) else { return Vec3::ZERO };
        let m = world_matrix(world, e);
        let (scale, rot, pos) = m.to_scale_rotation_translation();
        let s = scale.abs();
        let half = (cc.height * 0.5 - cc.radius).max(0.0) * s.y;
        let shape = rapier3d::geometry::Capsule::new_y(half, cc.radius * s.x.max(s.z));
        let center = pos + rot * (cc.center * s);
        let controller = KinematicCharacterController {
            offset: CharacterLength::Absolute(cc.skin_width.max(0.001)),
            up: Vector::y_axis(),
            max_slope_climb_angle: cc.slope_limit.to_radians(),
            min_slope_slide_angle: cc.slope_limit.to_radians(),
            autostep: (cc.step_offset > 0.0).then_some(CharacterAutostep { max_height: CharacterLength::Absolute(cc.step_offset), min_width: CharacterLength::Absolute(cc.radius * 0.5), include_dynamic_bodies: false }),
            snap_to_ground: Some(CharacterLength::Absolute(cc.step_offset.max(0.05))),
            ..Default::default()
        };
        let mut filter = QueryFilter::default().exclude_sensors().groups(InteractionGroups::new(Group::from_bits_truncate(1 << cc.layer.min(31)), Group::from_bits_truncate(self.layers[cc.layer.min(31) as usize])));
        if let Some(t) = self.tracked.get(&e) {
            filter = filter.exclude_rigid_body(t.body);
        }
        let mut flags = CollisionFlags::default();
        let moved = controller.move_shape(dt.max(1e-4), &self.bodies, &self.colliders, &self.queries, &shape, &iso(center, goat_core::math::Quat::IDENTITY), v(motion), filter, |c| {
            let n = gv(&c.hit.normal1);
            if n.y > 0.7 {
                flags.below = true;
            } else if n.y < -0.7 {
                flags.above = true;
            } else {
                flags.sides = true;
            }
        });
        let done = gv(&moved.translation);
        flags.below |= moved.grounded;
        // world → local (under a parent)
        let new_world = pos + done;
        let parent = world.get::<&Parent>(e).ok().map(|p| p.0);
        let local = match parent {
            Some(p) => world_matrix(world, p).inverse().transform_point3(new_world),
            None => new_world,
        };
        if let Ok(mut t) = world.get::<&mut Transform>(e) {
            t.position = local;
        }
        if let Ok(mut c) = world.get::<&mut CharacterController>(e) {
            c.is_grounded = moved.grounded;
            c.collision_flags = flags;
            c.velocity = if dt > 0.0 { done / dt } else { Vec3::ZERO };
        }
        // keep the kinematic capsule (what others hit) with it
        if let Some(t) = self.tracked.get_mut(&e) {
            if let Some(b) = self.bodies.get_mut(t.body) {
                b.set_next_kinematic_position(iso(pos + done, rot));
            }
        }
        done
    }
}
