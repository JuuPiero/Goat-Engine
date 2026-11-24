//! Unity's `Camera.ScreenPointToRay`: the world ray through a pixel of a display, for the
//! camera drawing there (pointer picking, PhysicsRaycaster, OnMouse* callbacks).

use goat_core::math::{Vec2, Vec3};
use goat_gfx::{Display, DisplayInfo, Projection};
use hecs::{Entity, World};

use crate::{Camera, GlobalTransform, InactiveInHierarchy};

/// A world ray: origin and unit direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Ray {
    pub fn at(&self, t: f32) -> Vec3 {
        self.origin + self.direction * t
    }
    /// Where the ray crosses the plane z = `z` (2D worlds), if it does.
    pub fn at_z(&self, z: f32) -> Option<Vec3> {
        if self.direction.z.abs() < 1e-6 {
            return None;
        }
        let t = (z - self.origin.z) / self.direction.z;
        (t >= 0.0).then(|| self.at(t))
    }
}

/// The display the pointer is on: the secondary one (3DS bottom screen) when there is one.
pub fn pointer_display(displays: &[DisplayInfo]) -> Display {
    if displays.iter().any(|d| d.display == Display::Secondary) {
        Display::Secondary
    } else {
        Display::Main
    }
}

/// The ray of `camera` through pixel `p` (origin top-left) of its display.
pub fn screen_point_to_ray(world: &World, displays: &[DisplayInfo], camera: Entity, p: Vec2) -> Option<Ray> {
    let c = world.get::<&Camera>(camera).ok()?;
    let g = world.get::<&GlobalTransform>(camera).ok()?.0;
    let info = displays.iter().find(|d| d.display == c.display)?;
    let (w, h) = (info.width as f32, info.height as f32);
    let vp = c.viewport;
    let (vx, vy, vw, vh) = (vp.x * w, vp.y * h, (vp.width * w).max(1.0), (vp.height * h).max(1.0));
    let nx = ((p.x - vx) / vw) * 2.0 - 1.0;
    let ny = 1.0 - ((p.y - vy) / vh) * 2.0;
    let aspect = vw / vh;
    let (_, rot, pos) = g.to_scale_rotation_translation();
    match c.projection {
        Projection::Perspective { fov_y_radians, .. } => {
            let t = (fov_y_radians * 0.5).tan();
            let dir = (rot * Vec3::new(nx * t * aspect, ny * t, -1.0)).normalize();
            Some(Ray { origin: pos, direction: dir })
        }
        Projection::Orthographic { height, .. } => {
            let half = height * 0.5;
            Some(Ray { origin: pos + rot * Vec3::new(nx * half * aspect, ny * half, 0.0), direction: rot * Vec3::NEG_Z })
        }
    }
}

/// The camera drawing on top at pixel `p` of `display` (highest depth whose viewport contains it).
pub fn camera_at(world: &World, displays: &[DisplayInfo], display: Display, p: Vec2) -> Option<Entity> {
    let info = displays.iter().find(|d| d.display == display)?;
    let (w, h) = (info.width as f32, info.height as f32);
    world
        .query::<&Camera>()
        .without::<&InactiveInHierarchy>()
        .iter()
        .filter(|(_, c)| c.display == display)
        .filter(|(_, c)| {
            let v = c.viewport;
            p.x >= v.x * w && p.x <= (v.x + v.width) * w && p.y >= v.y * h && p.y <= (v.y + v.height) * h
        })
        .max_by_key(|(_, c)| c.depth)
        .map(|(e, _)| e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Transform;

    #[test]
    fn center_ray_looks_forward_and_ortho_rays_are_parallel() {
        let mut w = World::new();
        let displays = [DisplayInfo { display: Display::Main, width: 400, height: 240, stereo: false }];
        let cam = w.spawn((Camera::default(), GlobalTransform(Transform::from_position(Vec3::new(0.0, 0.0, 10.0)).matrix())));
        let r = screen_point_to_ray(&w, &displays, cam, Vec2::new(200.0, 120.0)).unwrap();
        assert!((r.direction - Vec3::NEG_Z).length() < 1e-5 && r.origin.z == 10.0);
        let right = screen_point_to_ray(&w, &displays, cam, Vec2::new(399.0, 120.0)).unwrap();
        assert!(right.direction.x > 0.3, "right edge looks right: {}", right.direction);
        w.get::<&mut Camera>(cam).unwrap().projection = Projection::Orthographic { height: 10.0, near: 0.1, far: 100.0 };
        let a = screen_point_to_ray(&w, &displays, cam, Vec2::new(0.0, 0.0)).unwrap();
        assert!((a.direction - Vec3::NEG_Z).length() < 1e-5);
        assert!((a.origin.y - 5.0).abs() < 1e-4 && a.origin.x < -8.0, "top-left of a 10-unit-high view: {}", a.origin);
        assert_eq!(camera_at(&w, &displays, Display::Main, Vec2::new(10.0, 10.0)), Some(cam));
        assert_eq!(a.at_z(0.0).unwrap().z, 0.0);
    }
}
