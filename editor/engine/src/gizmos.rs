//! Component gizmos in the Scene view (Unity's OnDrawGizmos / collider outlines): wire shapes of
//! the selected objects' colliders (3D / 2D, character controllers), camera frustums, lights
//! (direction, range, cone), audio ranges, navigation agents / obstacles. One mesh of
//! camera-facing line quads rebuilt every frame on the editor layer, drawn over the scene.

use goat_core::math::{Mat4, Quat, Vec2, Vec3};
use goat_core::Color;
use goat_gfx::{BlendMode, CullMode, GraphicsBackend, LightingModel, MaterialDesc, MaterialId, MeshData, MeshId, Projection, Vertex};
use goat_runtime::*;

use crate::core::{EditorCore, EDITOR_LAYER};

// PiaEngine's gizmo colours: colliders 0x8ef08e, lights 0xffe066, camera frustum 0xdddddd
const COLLIDER: [f32; 4] = [0.557, 0.941, 0.557, 0.9];
const LIGHT: [f32; 4] = [1.0, 0.878, 0.4, 0.9];
const CAMERA: [f32; 4] = [0.867, 0.867, 0.867, 0.9];
const AUDIO: [f32; 4] = [0.45, 0.7, 1.0, 0.8];
const NAV: [f32; 4] = [0.4, 0.95, 0.85, 0.9];

#[derive(Default)]
pub struct Gizmos {
    entity: Option<Entity>,
    mesh: Option<MeshId>,
    material: Option<MaterialId>,
}

/// Line segments → camera-facing quads.
pub struct Lines {
    eye: Vec3,
    /// World width per unit of distance from the eye.
    width: f32,
    pub mesh: MeshData,
}

impl Lines {
    pub fn new(eye: Vec3, width: f32) -> Self {
        Lines { eye, width, mesh: MeshData::default() }
    }
    pub fn line(&mut self, a: Vec3, b: Vec3, color: [f32; 4]) {
        if self.mesh.vertices.len() > 65000 {
            return;
        }
        let d = b - a;
        if d.length_squared() < 1e-12 {
            return;
        }
        let mid = (a + b) * 0.5;
        let side = d.cross(self.eye - mid).normalize_or_zero();
        let (wa, wb) = ((self.eye - a).length() * self.width, (self.eye - b).length() * self.width);
        let base = self.mesh.vertices.len() as u16;
        for p in [a - side * wa, a + side * wa, b + side * wb, b - side * wb] {
            self.mesh.vertices.push(Vertex { position: p.into(), normal: [0.0, 1.0, 0.0], uv: [0.0, 0.0], color });
        }
        self.mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    pub fn polyline(&mut self, pts: &[Vec3], closed: bool, color: [f32; 4]) {
        for w in pts.windows(2) {
            self.line(w[0], w[1], color);
        }
        if closed && pts.len() > 2 {
            self.line(pts[pts.len() - 1], pts[0], color);
        }
    }
    /// A circle around `center` in the plane spanned by `u`, `v` (unit vectors).
    pub fn circle(&mut self, center: Vec3, u: Vec3, v: Vec3, r: f32, color: [f32; 4]) {
        let pts: Vec<Vec3> = (0..32).map(|i| {
            let a = i as f32 / 32.0 * std::f32::consts::TAU;
            center + (u * a.cos() + v * a.sin()) * r
        })
        .collect();
        self.polyline(&pts, true, color);
    }
    /// A box through a matrix (`c` center, `h` half size, local space).
    pub fn wire_box(&mut self, m: &Mat4, c: Vec3, h: Vec3, color: [f32; 4]) {
        let p = |x: f32, y: f32, z: f32| m.transform_point3(c + Vec3::new(x * h.x, y * h.y, z * h.z));
        for (a, b) in [((-1., -1., -1.), (1., -1., -1.)), ((1., -1., -1.), (1., -1., 1.)), ((1., -1., 1.), (-1., -1., 1.)), ((-1., -1., 1.), (-1., -1., -1.)), ((-1., 1., -1.), (1., 1., -1.)), ((1., 1., -1.), (1., 1., 1.)), ((1., 1., 1.), (-1., 1., 1.)), ((-1., 1., 1.), (-1., 1., -1.)), ((-1., -1., -1.), (-1., 1., -1.)), ((1., -1., -1.), (1., 1., -1.)), ((1., -1., 1.), (1., 1., 1.)), ((-1., -1., 1.), (-1., 1., 1.))] {
            self.line(p(a.0, a.1, a.2), p(b.0, b.1, b.2), color);
        }
    }
    /// A sphere (3 great circles) of world radius `r`.
    pub fn wire_sphere(&mut self, c: Vec3, rot: Quat, r: f32, color: [f32; 4]) {
        let (x, y, z) = (rot * Vec3::X, rot * Vec3::Y, rot * Vec3::Z);
        self.circle(c, x, y, r, color);
        self.circle(c, y, z, r, color);
        self.circle(c, z, x, r, color);
    }
    /// A capsule along `up` (world): `half` = half the straight part.
    pub fn wire_capsule(&mut self, c: Vec3, rot: Quat, r: f32, half: f32, color: [f32; 4]) {
        let (x, y, z) = (rot * Vec3::X, rot * Vec3::Y, rot * Vec3::Z);
        for s in [-1.0f32, 1.0] {
            let cc = c + y * half * s;
            self.circle(cc, x, z, r, color);
            // half circles closing the ends
            for (u, v) in [(x, y * s), (z, y * s)] {
                let pts: Vec<Vec3> = (0..=16).map(|i| {
                    let a = i as f32 / 16.0 * std::f32::consts::PI;
                    cc + (u * a.cos() + v * a.sin()) * r
                })
                .collect();
                self.polyline(&pts, false, color);
            }
        }
        for d in [x, -x, z, -z] {
            self.line(c + d * r - y * half, c + d * r + y * half, color);
        }
    }
}

impl EditorCore {
    /// The gizmo lines of the current selection (also used by tests).
    pub fn gizmo_lines(&self) -> Lines {
        let w = &self.edit.world;
        let eye = self.orbit.position();
        let mut l = Lines::new(eye, 0.0016);
        for &e in &self.selection {
            let Ok(g) = w.get::<&GlobalTransform>(e) else { continue };
            let m = g.0;
            let (scale, rot, pos) = m.to_scale_rotation_translation();
            // 3D colliders: center through the matrix, sizes × |scale| (like the physics)
            if let Ok(c) = w.get::<&goat_physics::Collider>(e) {
                let center = m.transform_point3(c.center);
                let s = scale.abs();
                match &c.shape {
                    goat_physics::Shape::Box(h) => l.wire_box(&Mat4::from_rotation_translation(rot, center), Vec3::ZERO, *h * s, COLLIDER),
                    goat_physics::Shape::Sphere(r) => l.wire_sphere(center, rot, r * s.max_element(), COLLIDER),
                    goat_physics::Shape::Capsule { half_height, radius } => l.wire_capsule(center, rot, radius * s.x.max(s.z), half_height * s.y, COLLIDER),
                    goat_physics::Shape::Mesh { .. } => {}
                }
            }
            if let Ok(c) = w.get::<&goat_physics::CharacterController>(e) {
                let center = pos + c.center * scale.abs();
                l.wire_capsule(center, Quat::IDENTITY, c.radius, (c.height * 0.5 - c.radius).max(0.0), COLLIDER);
            }
            // 2D colliders (XY plane)
            if let Ok(c) = w.get::<&goat_physics2d::Collider2D>(e) {
                let off = m.transform_point3(Vec3::new(c.offset.x, c.offset.y, 0.0));
                let s = scale.abs();
                let p = |v: Vec2| off + rot * Vec3::new(v.x * s.x, v.y * s.y, 0.0);
                match &c.shape {
                    goat_physics2d::Shape2D::Box(h) => l.polyline(&[p(Vec2::new(-h.x, -h.y)), p(Vec2::new(h.x, -h.y)), p(Vec2::new(h.x, h.y)), p(Vec2::new(-h.x, h.y))], true, COLLIDER),
                    goat_physics2d::Shape2D::Circle(r) => l.circle(off, rot * Vec3::X, rot * Vec3::Y, r * s.x.max(s.y), COLLIDER),
                    goat_physics2d::Shape2D::Capsule { half_height, radius } => {
                        let (x, y) = (rot * Vec3::X, rot * Vec3::Y);
                        let (r, hh) = (radius * s.x, half_height * s.y);
                        l.line(off - x * r - y * hh, off - x * r + y * hh, COLLIDER);
                        l.line(off + x * r - y * hh, off + x * r + y * hh, COLLIDER);
                        l.circle(off + y * hh, x, y, r, COLLIDER);
                        l.circle(off - y * hh, x, y, r, COLLIDER);
                    }
                    goat_physics2d::Shape2D::Polygon(pts) => l.polyline(&pts.iter().map(|v| p(*v)).collect::<Vec<_>>(), true, COLLIDER),
                    goat_physics2d::Shape2D::Edge(pts) => l.polyline(&pts.iter().map(|v| p(*v)).collect::<Vec<_>>(), false, COLLIDER),
                }
            }
            // cameras: the frustum (display aspect 5:3 for the top screen)
            if let Ok(c) = w.get::<&Camera>(e) {
                let aspect = 400.0 / 240.0 * (c.viewport.width / c.viewport.height.max(1e-3));
                let (fwd, up, right) = (rot * Vec3::NEG_Z, rot * Vec3::Y, rot * Vec3::X);
                let (hw_n, hh_n, hw_f, hh_f, near, far) = match c.projection {
                    Projection::Perspective { fov_y_radians, near, far } => {
                        let t = (fov_y_radians * 0.5).tan();
                        let far = far.min(near + 30.0);
                        (t * near * aspect, t * near, t * far * aspect, t * far, near, far)
                    }
                    Projection::Orthographic { height, near, far } => {
                        let far = far.min(near + 30.0);
                        (height * 0.5 * aspect, height * 0.5, height * 0.5 * aspect, height * 0.5, near, far)
                    }
                };
                let rect = |d: f32, hw: f32, hh: f32| [pos + fwd * d - right * hw - up * hh, pos + fwd * d + right * hw - up * hh, pos + fwd * d + right * hw + up * hh, pos + fwd * d - right * hw + up * hh];
                let (n, f) = (rect(near, hw_n, hh_n), rect(far, hw_f, hh_f));
                l.polyline(&n, true, CAMERA);
                l.polyline(&f, true, CAMERA);
                for i in 0..4 {
                    l.line(n[i], f[i], CAMERA);
                }
            }
            // lights
            if w.get::<&DirectionalLight>(e).is_ok() {
                let d = rot * Vec3::NEG_Z;
                let (u, v) = (rot * Vec3::X, rot * Vec3::Y);
                l.circle(pos, u, v, 0.35, LIGHT);
                for k in 0..8 {
                    let a = k as f32 / 8.0 * std::f32::consts::TAU;
                    let o = pos + (u * a.cos() + v * a.sin()) * 0.35;
                    l.line(o, o + d * 1.5, LIGHT);
                }
            }
            if let Ok(p) = w.get::<&PointLight>(e) {
                l.wire_sphere(pos, Quat::IDENTITY, p.range, LIGHT);
            }
            if let Ok(s) = w.get::<&SpotLight>(e) {
                let d = rot * Vec3::NEG_Z;
                let r = (s.spot_angle.to_radians() * 0.5).tan() * s.range;
                let (u, v) = (rot * Vec3::X, rot * Vec3::Y);
                let end = pos + d * s.range;
                l.circle(end, u, v, r, LIGHT);
                for k in 0..4 {
                    let a = k as f32 / 4.0 * std::f32::consts::TAU;
                    l.line(pos, end + (u * a.cos() + v * a.sin()) * r, LIGHT);
                }
            }
            if let Ok(a) = w.get::<&goat_audio::AudioSource>(e) {
                if a.spatial {
                    l.wire_sphere(pos, Quat::IDENTITY, a.min_distance, AUDIO);
                    l.wire_sphere(pos, Quat::IDENTITY, a.max_distance, AUDIO);
                }
            }
            if let Ok(a) = w.get::<&goat_navigation::NavMeshAgent>(e) {
                let feet = pos - Vec3::Y * a.base_offset;
                l.circle(feet, Vec3::X, Vec3::Z, a.radius, NAV);
                l.circle(feet + Vec3::Y * a.height, Vec3::X, Vec3::Z, a.radius, NAV);
                for d in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
                    l.line(feet + d * a.radius, feet + d * a.radius + Vec3::Y * a.height, NAV);
                }
            }
            if let Ok(o) = w.get::<&goat_navigation::NavMeshObstacle>(e) {
                match o.shape {
                    goat_navigation::ObstacleShape::Box => l.wire_box(&m, Vec3::from(o.center), Vec3::from(o.size) * 0.5, NAV),
                    goat_navigation::ObstacleShape::Capsule => l.wire_capsule(m.transform_point3(Vec3::from(o.center)), rot, o.radius * scale.x.abs(), (o.height * 0.5 - o.radius).max(0.0) * scale.y.abs(), NAV),
                }
            }
        }
        l
    }

    /// Rebuilds the gizmo mesh (edit mode).
    pub(crate) fn update_gizmos(&mut self, gfx: &mut dyn GraphicsBackend) {
        let lines = self.gizmo_lines();
        if self.gizmos.material.is_none() {
            self.gizmos.material = gfx.create_material(&MaterialDesc { base_color: Color::WHITE, lighting: LightingModel::Unlit, blend: BlendMode::Alpha, cull: CullMode::None, depth_write: false, depth_test: false, fog: false, ..Default::default() }).ok();
        }
        let Some(mat) = self.gizmos.material else { return };
        if self.gizmos.entity.is_none() {
            let Ok(mesh) = gfx.create_mesh(&MeshData::default()) else { return };
            let e = self.edit.world.spawn((Name("Component Gizmos".into()), DontSave, Transform::default(), GlobalTransform::default(), MeshRenderer::new(mesh, mat).with_layer(EDITOR_LAYER).with_sorting_order(1500)));
            self.gizmos.entity = Some(e);
            self.gizmos.mesh = Some(mesh);
        }
        let (Some(e), Some(mesh)) = (self.gizmos.entity, self.gizmos.mesh) else { return };
        if let Ok(mut r) = self.edit.world.get::<&mut MeshRenderer>(e) {
            r.visible = self.view.gizmos;
        }
        if !self.view.gizmos {
            return;
        }
        let empty = lines.mesh.vertices.is_empty();
        if !empty {
            let _ = gfx.update_mesh(mesh, &lines.mesh);
        }
        if let Ok(mut r) = self.edit.world.get::<&mut MeshRenderer>(e) {
            r.visible = !empty;
        }
    }
}
