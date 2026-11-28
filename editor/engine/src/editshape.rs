//! Edit Collider / Edit Points (like PiaEngine's and Unity's): with the tool on, the Scene view shows
//! dots on the selected object's collider (box faces, sphere / capsule radius and ends, 2D box edges
//! and corners, circle radius, polygon / edge points) or its LineRenderer points instead of the
//! transform handles. Dragging a box face moves that face only (the opposite one stays: center and
//! size change together); points move on the XY plane (2D shapes) or on a plane facing the camera
//! (line points); the hollow dot in the middle of a segment inserts a point, Ctrl+click deletes one.
//! The geometry follows the gizmo outlines (gizmos.rs) and the physics: centers through the full
//! matrix, sizes × |scale|.

use goat_core::math::{Vec2, Vec3};
use goat_runtime::*;

use crate::core::EditorCore;

/// Handle ids of the edit dots start here (`handles::AXIS`… are below).
pub const EDIT: usize = 1000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditTool {
    #[default]
    None,
    /// Edit Collider: Collider / Collider2D / CharacterController.
    Collider,
    /// Edit Points: LineRenderer.
    Points,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    /// a face of a 3D box (local axis, side)
    Face { axis: usize, sign: f32 },
    /// a sphere's radius
    Radius { axis: usize, sign: f32 },
    /// a capsule's radius (local X / Z) and its ends (local Y)
    CapRadius { axis: usize, sign: f32 },
    CapEnd { sign: f32 },
    /// 2D box: an edge (one of sx / sy is 0) or a corner
    Box2 { sx: f32, sy: f32 },
    /// 2D circle radius
    Circle { axis: usize, sign: f32 },
    /// a point of a polygon / edge collider / line
    Point(usize),
    /// inserts a point at index i (middle of the segment before it)
    Insert(usize),
}

pub(crate) struct Dot {
    pub kind: Kind,
    pub pos: Vec3,
}

/// What is being edited on the entity.
#[derive(Clone, Debug)]
enum Target {
    Collider(goat_physics::Collider),
    Controller(goat_physics::CharacterController),
    Collider2D(goat_physics2d::Collider2D),
    Line(goat_lines::LineRenderer),
}

pub(crate) struct EditDrag {
    entity: Entity,
    kind: Kind,
    start: Target,
    origin: Vec3,
    /// axis drags: world direction and the parameter at the press
    dir: Vec3,
    t0: f32,
    /// point drags: plane normal and where the press hit it
    normal: Vec3,
    grab: Vec3,
}

fn axis_param(o: Vec3, a: Vec3, r0: Vec3, rd: Vec3) -> Option<f32> {
    let w0 = o - r0;
    let b = a.dot(rd);
    let denom = 1.0 - b * b;
    if denom < 1e-5 {
        return None;
    }
    Some((b * rd.dot(w0) - a.dot(w0)) / denom)
}

fn plane_hit(o: Vec3, n: Vec3, r0: Vec3, rd: Vec3) -> Option<Vec3> {
    let dn = rd.dot(n);
    if dn.abs() < 1e-5 {
        return None;
    }
    Some(r0 + rd * ((o - r0).dot(n) / dn))
}

const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];

impl EditorCore {
    fn edit_target(&self, e: Entity) -> Option<Target> {
        let w = crate::core::view_world!(self);
        match self.tools.edit {
            EditTool::None => None,
            EditTool::Collider => {
                if let Ok(c) = w.get::<&goat_physics::Collider>(e) {
                    if !matches!(c.shape, goat_physics::Shape::Mesh { .. }) {
                        return Some(Target::Collider((*c).clone()));
                    }
                }
                if let Ok(c) = w.get::<&goat_physics2d::Collider2D>(e) {
                    return Some(Target::Collider2D((*c).clone()));
                }
                w.get::<&goat_physics::CharacterController>(e).ok().map(|c| Target::Controller((*c).clone()))
            }
            EditTool::Points => w.get::<&goat_lines::LineRenderer>(e).ok().map(|l| Target::Line((*l).clone())),
        }
    }

    /// The edit dots of the selected object (empty when the tool is off / nothing to edit).
    pub(crate) fn edit_dots(&self) -> Vec<Dot> {
        let mut out = Vec::new();
        let Some(e) = self.selected else { return out };
        let Some(target) = self.edit_target(e) else { return out };
        let w = crate::core::view_world!(self);
        let Ok(g) = w.get::<&GlobalTransform>(e) else { return out };
        let m = g.0;
        let (scale, rot, pos) = m.to_scale_rotation_translation();
        let s = scale.abs();
        match target {
            Target::Collider(c) => {
                let center = m.transform_point3(c.center);
                match c.shape {
                    goat_physics::Shape::Box(h) => {
                        for axis in 0..3 {
                            for sign in [-1.0f32, 1.0] {
                                out.push(Dot { kind: Kind::Face { axis, sign }, pos: center + rot * AXES[axis] * sign * h[axis] * s[axis] });
                            }
                        }
                    }
                    goat_physics::Shape::Sphere(r) => {
                        let r = r * s.max_element();
                        for axis in 0..3 {
                            for sign in [-1.0f32, 1.0] {
                                out.push(Dot { kind: Kind::Radius { axis, sign }, pos: center + rot * AXES[axis] * sign * r });
                            }
                        }
                    }
                    goat_physics::Shape::Capsule { half_height, radius } => {
                        let r = radius * s.x.max(s.z);
                        for axis in [0usize, 2] {
                            for sign in [-1.0f32, 1.0] {
                                out.push(Dot { kind: Kind::CapRadius { axis, sign }, pos: center + rot * AXES[axis] * sign * r });
                            }
                        }
                        for sign in [-1.0f32, 1.0] {
                            out.push(Dot { kind: Kind::CapEnd { sign }, pos: center + rot * Vec3::Y * sign * (half_height * s.y + r) });
                        }
                    }
                    goat_physics::Shape::Mesh { .. } => {}
                }
            }
            Target::Controller(c) => {
                // upright capsule, center × |scale| (like the gizmo outline)
                let center = pos + c.center * s;
                for axis in [0usize, 2] {
                    for sign in [-1.0f32, 1.0] {
                        out.push(Dot { kind: Kind::CapRadius { axis, sign }, pos: center + AXES[axis] * sign * c.radius });
                    }
                }
                for sign in [-1.0f32, 1.0] {
                    out.push(Dot { kind: Kind::CapEnd { sign }, pos: center + Vec3::Y * sign * (c.height * 0.5).max(c.radius) });
                }
            }
            Target::Collider2D(c) => {
                let off = m.transform_point3(Vec3::new(c.offset.x, c.offset.y, 0.0));
                let p = |v: Vec2| off + rot * Vec3::new(v.x * s.x, v.y * s.y, 0.0);
                match &c.shape {
                    goat_physics2d::Shape2D::Box(h) => {
                        for (sx, sy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                            out.push(Dot { kind: Kind::Box2 { sx, sy }, pos: p(Vec2::new(sx * h.x, sy * h.y)) });
                        }
                    }
                    goat_physics2d::Shape2D::Circle(r) => {
                        let r = r * s.x.max(s.y);
                        for axis in 0..2 {
                            for sign in [-1.0f32, 1.0] {
                                out.push(Dot { kind: Kind::Circle { axis, sign }, pos: off + rot * AXES[axis] * sign * r });
                            }
                        }
                    }
                    goat_physics2d::Shape2D::Capsule { half_height, radius } => {
                        for sign in [-1.0f32, 1.0] {
                            out.push(Dot { kind: Kind::CapRadius { axis: 0, sign }, pos: off + rot * Vec3::X * sign * radius * s.x });
                            out.push(Dot { kind: Kind::CapEnd { sign }, pos: off + rot * Vec3::Y * sign * (half_height * s.y + radius * s.x) });
                        }
                    }
                    goat_physics2d::Shape2D::Polygon(pts) | goat_physics2d::Shape2D::Edge(pts) => {
                        let closed = matches!(c.shape, goat_physics2d::Shape2D::Polygon(_));
                        points_dots(&mut out, pts.iter().map(|v| p(*v)).collect(), closed);
                    }
                }
            }
            Target::Line(l) => {
                let to_world = |q: [f32; 3]| if l.use_world_space { Vec3::from(q) } else { m.transform_point3(Vec3::from(q)) };
                points_dots(&mut out, l.positions.iter().map(|q| to_world(*q)).collect(), l.looped);
            }
        }
        out
    }

    /// Starts dragging edit dot `index` (of `edit_dots`) from view pixel `at`.
    pub(crate) fn edit_start(&mut self, index: usize, at: Vec2) -> bool {
        let Some(e) = self.selected else { return false };
        let dots = self.edit_dots();
        let Some(dot) = dots.get(index) else { return false };
        let (kind, origin) = (dot.kind, dot.pos);
        let Some(mut start) = self.edit_target(e) else { return false };
        let w = crate::core::view_world!(self);
        let Some(m) = w.get::<&GlobalTransform>(e).ok().map(|g| g.0) else { return false };
        let (_, rot, _) = m.to_scale_rotation_translation();
        let mut kind = kind;
        // a segment's middle: insert the point there, then drag it
        if let Kind::Insert(i) = kind {
            match &mut start {
                Target::Collider2D(c) => {
                    if let goat_physics2d::Shape2D::Polygon(pts) | goat_physics2d::Shape2D::Edge(pts) = &mut c.shape {
                        let a = pts[(i + pts.len() - 1) % pts.len()];
                        let b = pts[i % pts.len()];
                        pts.insert(i, (a + b) * 0.5);
                    }
                }
                Target::Line(l) => {
                    let n = l.positions.len();
                    let (a, b) = (Vec3::from(l.positions[(i + n - 1) % n]), Vec3::from(l.positions[i % n]));
                    l.positions.insert(i, ((a + b) * 0.5).to_array());
                }
                _ => return false,
            }
            self.write_target(e, &start);
            kind = Kind::Point(i);
        }
        let (r0, rd) = self.edit_ray(at);
        let dir = match kind {
            Kind::Face { axis, sign } | Kind::Radius { axis, sign } | Kind::Circle { axis, sign } => match &start {
                Target::Controller(_) => AXES[axis] * sign,
                _ => rot * AXES[axis] * sign,
            },
            Kind::CapRadius { axis, sign } => match &start {
                Target::Controller(_) => AXES[axis] * sign,
                _ => rot * AXES[axis] * sign,
            },
            Kind::CapEnd { sign } => match &start {
                Target::Controller(_) => Vec3::Y * sign,
                _ => rot * Vec3::Y * sign,
            },
            Kind::Box2 { sx, sy } => (rot * Vec3::new(sx, sy, 0.0)).normalize_or(Vec3::X),
            _ => Vec3::X,
        };
        let points_2d = matches!(start, Target::Collider2D(_)) || self.view.mode2d;
        let normal = if points_2d { rot * Vec3::Z } else { self.edit_eye(origin) };
        let grab = plane_hit(origin, normal, r0, rd).unwrap_or(origin);
        let t0 = axis_param(origin, dir, r0, rd).unwrap_or(0.0);
        self.tools.edit_drag = Some(EditDrag { entity: e, kind, start, origin, dir, t0, normal, grab });
        true
    }

    /// A drag step (`cur` = pointer in view pixels, `snap` = Ctrl).
    pub(crate) fn edit_drag_to(&mut self, cur: Vec2, snap: bool) {
        let Some(d) = self.tools.edit_drag.as_ref() else { return };
        let (r0, rd) = self.edit_ray(cur);
        let step = self.tools.snap[0];
        let round = |v: f32| if snap && step > 0.0 { (v / step).round() * step } else { v };
        let e = d.entity;
        let w = crate::core::view_world!(self);
        let Some(m) = w.get::<&GlobalTransform>(e).ok().map(|g| g.0) else { return };
        let (scale, rot, _) = m.to_scale_rotation_translation();
        let s = scale.abs().max(Vec3::splat(1e-6));
        let along = axis_param(d.origin, d.dir, r0, rd).map(|t| round(t - d.t0)).unwrap_or(0.0);
        let world_point = plane_hit(d.origin, d.normal, r0, rd).map(|p| d.origin + (p - d.grab)).unwrap_or(d.origin);
        let world_point = if snap && step > 0.0 { (world_point / step).round() * step } else { world_point };
        let mut next = d.start.clone();
        match (&mut next, d.kind) {
            (Target::Collider(c), kind) => match (&mut c.shape, kind) {
                (goat_physics::Shape::Box(h), Kind::Face { axis, sign }) => {
                    // the dragged face moves, the opposite one stays
                    let delta = along / s[axis];
                    let face = c.center[axis] + sign * h[axis] + sign * delta;
                    let other = c.center[axis] - sign * h[axis];
                    c.center[axis] = (face + other) * 0.5;
                    h[axis] = ((face - other) * 0.5).abs().max(0.001);
                }
                (goat_physics::Shape::Sphere(r), Kind::Radius { .. }) => *r = (*r + along / s.max_element()).max(0.001),
                (goat_physics::Shape::Capsule { radius, .. }, Kind::CapRadius { .. }) => *radius = (*radius + along / s.x.max(s.z)).max(0.001),
                (goat_physics::Shape::Capsule { half_height, radius }, Kind::CapEnd { sign }) => {
                    let delta = along / s.y;
                    let r = *radius * s.x.max(s.z) / s.y;
                    let end = c.center.y + sign * (*half_height + r) + sign * delta;
                    let other = c.center.y - sign * (*half_height + r);
                    c.center.y = (end + other) * 0.5;
                    *half_height = ((end - other).abs() * 0.5 - r).max(0.0);
                }
                _ => {}
            },
            (Target::Controller(c), Kind::CapRadius { .. }) => c.radius = (c.radius + along).max(0.001),
            (Target::Controller(c), Kind::CapEnd { sign }) => {
                // world units: center × |scale|, height unscaled (like the outline)
                let cw = c.center.y * s.y;
                let end = cw + sign * (c.height * 0.5) + sign * along;
                let other = cw - sign * (c.height * 0.5);
                c.center.y = (end + other) * 0.5 / s.y;
                c.height = (end - other).abs().max(c.radius * 2.0);
            }
            (Target::Collider2D(c), kind) => {
                // shape space: relative to the (start) offset, before the object's scale
                let off0 = c.offset;
                let off_world = m.transform_point3(Vec3::new(off0.x, off0.y, 0.0));
                let v = rot.inverse() * (world_point - off_world);
                let local_point = Vec2::new(v.x / s.x, v.y / s.y);
                match (&mut c.shape, kind) {
                    (goat_physics2d::Shape2D::Box(h), Kind::Box2 { sx, sy }) => {
                        // edges / corners: the dragged sides move, the opposite ones stay
                        for (i, sign) in [(0usize, sx), (1usize, sy)] {
                            if sign == 0.0 {
                                continue;
                            }
                            let face = local_point[i];
                            let other = -sign * h[i];
                            h[i] = ((face - other) * 0.5).abs().max(0.001);
                            c.offset[i] = off0[i] + (face + other) * 0.5;
                        }
                    }
                    (goat_physics2d::Shape2D::Circle(r), Kind::Circle { .. }) => *r = (*r + along / s.x.max(s.y)).max(0.001),
                    (goat_physics2d::Shape2D::Capsule { radius, .. }, Kind::CapRadius { .. }) => *radius = (*radius + along / s.x).max(0.001),
                    (goat_physics2d::Shape2D::Capsule { half_height, radius }, Kind::CapEnd { sign }) => {
                        let delta = along / s.y;
                        let r = *radius * s.x / s.y;
                        let end = sign * (*half_height + r) + sign * delta;
                        let other = -sign * (*half_height + r);
                        c.offset.y += (end + other) * 0.5;
                        *half_height = ((end - other).abs() * 0.5 - r).max(0.0);
                    }
                    (goat_physics2d::Shape2D::Polygon(pts) | goat_physics2d::Shape2D::Edge(pts), Kind::Point(i)) => {
                        if let Some(p) = pts.get_mut(i) {
                            *p = local_point;
                        }
                    }
                    _ => {}
                }
            }
            (Target::Line(l), Kind::Point(i)) => {
                let q = if l.use_world_space { world_point } else { m.inverse().transform_point3(world_point) };
                if let Some(p) = l.positions.get_mut(i) {
                    *p = q.to_array();
                }
            }
            _ => {}
        }
        self.write_target(e, &next);
    }

    /// Ctrl+click on a point: deletes it (polygons keep 3, edges / lines 2). `dry` = only tell.
    pub fn edit_delete(&mut self, x: f32, y: f32, w: f32, h: f32, dry: bool) -> bool {
        let Some(e) = self.selected else { return false };
        let Some(crate::editshape::Kind::Point(i)) = self.gizmo_hit(x, y, w, h).and_then(|id| id.checked_sub(EDIT)).and_then(|k| self.edit_dots().get(k).map(|d| d.kind)) else { return false };
        let Some(mut t) = self.edit_target(e) else { return false };
        let ok = match &mut t {
            Target::Collider2D(c) => match &mut c.shape {
                goat_physics2d::Shape2D::Polygon(pts) if pts.len() > 3 && i < pts.len() => {
                    pts.remove(i);
                    true
                }
                goat_physics2d::Shape2D::Edge(pts) if pts.len() > 2 && i < pts.len() => {
                    pts.remove(i);
                    true
                }
                _ => false,
            },
            Target::Line(l) if l.positions.len() > 2 && i < l.positions.len() => {
                l.positions.remove(i);
                true
            }
            _ => false,
        };
        if ok && !dry {
            self.write_target(e, &t);
        }
        ok
    }

    fn write_target(&mut self, e: Entity, t: &Target) {
        let w = crate::core::view_world!(mut self);
        match t {
            Target::Collider(c) => {
                if let Ok(mut x) = w.get::<&mut goat_physics::Collider>(e) {
                    *x = c.clone();
                }
            }
            Target::Controller(c) => {
                if let Ok(mut x) = w.get::<&mut goat_physics::CharacterController>(e) {
                    *x = c.clone();
                }
            }
            Target::Collider2D(c) => {
                if let Ok(mut x) = w.get::<&mut goat_physics2d::Collider2D>(e) {
                    *x = c.clone();
                }
            }
            Target::Line(l) => {
                if let Ok(mut x) = w.get::<&mut goat_lines::LineRenderer>(e) {
                    *x = l.clone();
                }
            }
        }
    }

    fn edit_ray(&self, p: Vec2) -> (Vec3, Vec3) {
        let v = self.tools.view_px.max(Vec2::ONE);
        self.ray(p.x / v.x, p.y / v.y)
    }

    fn edit_eye(&self, p: Vec3) -> Vec3 {
        if self.view.ortho {
            self.orbit.rotation() * Vec3::Z
        } else {
            (self.orbit.position() - p).normalize_or(Vec3::Z)
        }
    }
}

/// Dots of a point list: the points, then the segment middles (inserting a point there).
fn points_dots(out: &mut Vec<Dot>, pts: Vec<Vec3>, closed: bool) {
    let n = pts.len();
    for (i, p) in pts.iter().enumerate() {
        out.push(Dot { kind: Kind::Point(i), pos: *p });
    }
    let segs = if closed { n } else { n.saturating_sub(1) };
    for k in 0..segs {
        let (a, b) = (pts[k], pts[(k + 1) % n]);
        out.push(Dot { kind: Kind::Insert(k + 1), pos: (a + b) * 0.5 });
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use goat_assets::AssetIo;
    use goat_gfx::NullBackend;

    fn core_with(setup: impl FnOnce(&mut hecs::World, Entity)) -> (EditorCore, NullBackend, Entity) {
        let io = AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets"));
        let mut gfx = NullBackend::new();
        let mut c = EditorCore::new(io);
        c.first_frame(&mut gfx);
        c.open_scene(&mut gfx, "scenes/showcase.scene.json").unwrap();
        let e = c.create_entity("Edited", None);
        setup(&mut c.edit.world, e);
        c.selected = Some(e);
        c.selection = vec![e];
        c.frame_selected(&gfx);
        c.frame(&mut gfx, 0.016);
        (c, gfx, e)
    }

    fn px(c: &EditorCore, p: Vec3) -> Vec2 {
        c.project(p).unwrap() * c.tools.view_px
    }

    #[test]
    fn box_face_drag_moves_only_that_face() {
        let (mut c, _gfx, e) = core_with(|w, e| {
            w.insert_one(e, goat_physics::Collider::default()).unwrap();
        });
        let h0 = match w_collider(&c, e).shape {
            goat_physics::Shape::Box(h) => h,
            _ => panic!("default collider is a box"),
        };
        c.tools.edit = EditTool::Collider;
        let dots = c.edit_dots();
        assert_eq!(dots.len(), 6, "6 face dots");
        let i = dots.iter().position(|d| d.kind == Kind::Face { axis: 0, sign: 1.0 }).unwrap();
        let from = px(&c, dots[i].pos);
        let toward = (px(&c, dots[i].pos + Vec3::X) - from).normalize();
        assert!(c.edit_start(i, from));
        c.edit_drag_to(from + toward * 40.0, false);
        let col = w_collider(&c, e);
        let h = match col.shape {
            goat_physics::Shape::Box(h) => h,
            _ => unreachable!(),
        };
        assert!(h.x > h0.x + 0.01, "the box grew along X ({} → {})", h0.x, h.x);
        assert!((col.center.x - h.x - (-h0.x)).abs() < 1e-4, "the −X face stayed at {}", -h0.x);
        assert!((h.y - h0.y).abs() < 1e-6 && (h.z - h0.z).abs() < 1e-6, "other sizes unchanged");
    }

    #[test]
    fn line_points_insert_move_delete() {
        let (mut c, _gfx, e) = core_with(|w, e| {
            w.insert_one(e, goat_lines::LineRenderer { positions: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]], ..Default::default() }).unwrap();
        });
        c.tools.edit = EditTool::Points;
        let dots = c.edit_dots();
        assert_eq!(dots.iter().filter(|d| matches!(d.kind, Kind::Point(_))).count(), 2);
        let ins = dots.iter().position(|d| d.kind == Kind::Insert(1)).unwrap();
        let from = px(&c, dots[ins].pos);
        assert!(c.edit_start(ins, from));
        c.edit_drag_to(from + Vec2::new(0.0, -30.0), false);
        c.gizmo_stop();
        let pts = c.edit.world.get::<&goat_lines::LineRenderer>(e).unwrap().positions.clone();
        assert_eq!(pts.len(), 3, "a point was inserted");
        assert!(pts[1][1] > 0.01 && (pts[1][0] - 1.0).abs() < 0.2, "and dragged up (y = {})", pts[1][1]);
        // Ctrl+click deletes it
        let p = px(&c, Vec3::from(pts[1])) / c.tools.view_px;
        let (w, h) = (c.tools.view_px.x, c.tools.view_px.y);
        assert!(c.edit_delete(p.x, p.y, w, h, true), "dry run");
        assert!(c.edit_delete(p.x, p.y, w, h, false));
        assert_eq!(c.edit.world.get::<&goat_lines::LineRenderer>(e).unwrap().positions.len(), 2);
        // never below two points
        let p0 = px(&c, Vec3::ZERO) / c.tools.view_px;
        assert!(!c.edit_delete(p0.x, p0.y, w, h, false));
    }

    fn w_collider(c: &EditorCore, e: Entity) -> goat_physics::Collider {
        (*c.edit.world.get::<&goat_physics::Collider>(e).unwrap()).clone()
    }
}

/// World points spanning an entity's colliders / line (F frames objects without renderers by them).
pub(crate) fn shape_points(w: &hecs::World, e: Entity) -> Vec<Vec3> {
    let mut out = Vec::new();
    let Ok(g) = w.get::<&GlobalTransform>(e) else { return out };
    let m = g.0;
    let (scale, rot, pos) = m.to_scale_rotation_translation();
    let s = scale.abs();
    let mut around = |c: Vec3, r: Vec3| {
        for k in 0..8 {
            out.push(c + rot * Vec3::new(if k & 1 == 0 { -r.x } else { r.x }, if k & 2 == 0 { -r.y } else { r.y }, if k & 4 == 0 { -r.z } else { r.z }));
        }
    };
    if let Ok(c) = w.get::<&goat_physics::Collider>(e) {
        let center = m.transform_point3(c.center);
        match c.shape {
            goat_physics::Shape::Box(h) => around(center, h * s),
            goat_physics::Shape::Sphere(r) => around(center, Vec3::splat(r * s.max_element())),
            goat_physics::Shape::Capsule { half_height, radius } => around(center, Vec3::new(radius * s.x, half_height * s.y + radius * s.x, radius * s.z)),
            goat_physics::Shape::Mesh { .. } => {}
        }
    }
    if let Ok(c) = w.get::<&goat_physics::CharacterController>(e) {
        around(pos + c.center * s, Vec3::new(c.radius, c.height * 0.5, c.radius));
    }
    if let Ok(c) = w.get::<&goat_physics2d::Collider2D>(e) {
        let off = m.transform_point3(Vec3::new(c.offset.x, c.offset.y, 0.0));
        match &c.shape {
            goat_physics2d::Shape2D::Box(h) => around(off, Vec3::new(h.x * s.x, h.y * s.y, 0.0)),
            goat_physics2d::Shape2D::Circle(r) => around(off, Vec3::new(r * s.x, r * s.y, 0.0)),
            goat_physics2d::Shape2D::Capsule { half_height, radius } => around(off, Vec3::new(radius * s.x, half_height * s.y + radius * s.x, 0.0)),
            goat_physics2d::Shape2D::Polygon(pts) | goat_physics2d::Shape2D::Edge(pts) => out.extend(pts.iter().map(|v| off + rot * Vec3::new(v.x * s.x, v.y * s.y, 0.0))),
        }
    }
    if let Ok(l) = w.get::<&goat_lines::LineRenderer>(e) {
        out.extend(l.positions.iter().map(|q| if l.use_world_space { Vec3::from(*q) } else { m.transform_point3(Vec3::from(*q)) }));
    }
    out
}
