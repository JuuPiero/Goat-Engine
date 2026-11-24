//! Transform handles and the Scene view overlay, drawn like PiaEngine's: in 3D the three.js
//! TransformControls PiaEngine uses (arrows with cones turned towards the camera, plane squares,
//! the centre handle, half rings facing the camera + the view ring + the free-rotation ball,
//! scale cubes), in the 2D view PiaEngine's 2D handles (X / Y arrows, the XY square, one ring,
//! scale squares). Also the orange selection outlines, camera / light icons and the 2D camera
//! rectangles. `scene_overlay` returns the shapes in view pixels — the editor UI draws them on a
//! canvas over the view — and picking / dragging use the same geometry.

use goat_core::math::{Mat4, Quat, Vec2, Vec3};
use goat_gfx::{GraphicsBackend, Projection};
use goat_runtime::*;
use serde_json::{json, Value};

use crate::core::EditorCore;
use crate::tools::GizmoMode;

/// Handle ids: X / Y / Z arrows (scale cubes in the Scale tool) …
pub const AXIS: usize = 0;
/// … the plane squares (plane whose normal is axis `id - PLANE`: YZ, XZ, XY) …
pub const PLANE: usize = 3;
/// … the centre (free move in the Move tool, uniform scale in Scale / 2D Transform) …
pub const CENTER: usize = 6;
/// … the rotation rings about X / Y / Z …
pub const RING: usize = 7;
/// … the view ring (rotation about the view direction) and the free-rotation ball.
pub const VIEW_RING: usize = 10;
pub const TRACKBALL: usize = 11;

const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];

// three.js TransformControls colours (3D) and PiaEngine's 2D handle colours
const C3: [&str; 3] = ["#ff0000", "#00ff00", "#0000ff"];
const YELLOW3: &str = "#ffff00";
const GRAY3: &str = "#787878";
const C2: [&str; 3] = ["#db4c3f", "#8bc34a", "#4a8fe0"];
const YELLOW2: &str = "#ffd23f";
const WHITE2: &str = "#e8e8e8";
const SELECTION: &str = "#ff9a1f";

// PiaEngine's 2D sizes (pixels)
const HANDLE_2D: f32 = 80.0;
const RING_2D: f32 = 70.0;
const T_ARROW_2D: f32 = 66.0;
const T_RING_2D: f32 = 98.0;

pub(crate) struct DragObject {
    pub entity: Entity,
    pub start: Transform,
    pub world: Mat4,
    pub parent: Mat4,
}

/// A handle drag in progress: the frame it started in, pointer positions (view pixels) and the
/// transforms it started from (each step recomputes from them, so snapping is exact).
pub(crate) struct GizmoDrag {
    pub handle: usize,
    pub origin: Vec3,
    pub basis: Quat,
    /// World units of one gizmo unit (three.js) / one pixel (2D) at the origin.
    pub unit: f32,
    pub start: Vec2,
    pub cur: Vec2,
    /// Where the pointer grabbed the handle: on the axis (parameter) or on the plane.
    axis_start: f32,
    grab: Option<Vec3>,
    /// Rotation so far (rings: accumulated angle; ball: accumulated rotation).
    last: Option<Vec3>,
    angle: f32,
    ball: Quat,
    pub objects: Vec<DragObject>,
    pub label: String,
}

enum Prim {
    /// Polyline (width px; `closed`).
    Line(Vec<Vec2>, f32, bool),
    /// Filled polygon with an optional outline colour.
    Poly(Vec<Vec2>, Option<&'static str>),
    /// Circle: stroke width (0 = filled).
    Circle(Vec2, f32, f32),
}

struct Part {
    id: Option<usize>,
    prim: Prim,
    color: &'static str,
    alpha: f32,
    pick: bool,
}

fn part(id: usize, prim: Prim, color: &'static str, alpha: f32) -> Part {
    Part { id: Some(id), prim, color, alpha, pick: true }
}
fn deco(prim: Prim, color: &'static str, alpha: f32) -> Part {
    Part { id: None, prim, color, alpha, pick: false }
}

/// Convex hull (monotone chain) of projected points: cones, cubes, the centre octahedron.
fn hull(mut p: Vec<Vec2>) -> Vec<Vec2> {
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    if p.len() < 3 {
        return p;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut h: Vec<Vec2> = Vec::with_capacity(p.len() * 2);
    for pass in 0..2 {
        let start = h.len();
        let iter: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 { Box::new(p.iter()) } else { Box::new(p.iter().rev()) };
        for &q in iter {
            while h.len() >= start + 2 && cross(h[h.len() - 2], h[h.len() - 1], q) <= 0.0 {
                h.pop();
            }
            h.push(q);
        }
        h.pop();
    }
    h
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

fn inside(p: Vec2, poly: &[Vec2]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c
}

fn hit_part(prim: &Prim, p: Vec2) -> bool {
    match prim {
        Prim::Line(pts, w, closed) => {
            let tol = (w * 0.5 + 5.0).max(6.0);
            let n = pts.len();
            (0..n.saturating_sub(1)).any(|i| seg_dist(p, pts[i], pts[i + 1]) < tol) || (*closed && n > 2 && seg_dist(p, pts[n - 1], pts[0]) < tol)
        }
        Prim::Poly(pts, _) => inside(p, pts) || (0..pts.len()).any(|i| seg_dist(p, pts[i], pts[(i + 1) % pts.len()]) < 3.0),
        Prim::Circle(c, r, w) => {
            let d = (p - *c).length();
            if *w == 0.0 {
                d < r + 3.0
            } else {
                (d - r).abs() < (w * 0.5 + 5.0).max(7.0)
            }
        }
    }
}

/// Closest point parameter on the line `o + s·a` (a unit) to the ray `r0 + t·rd`.
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
    let t = (o - r0).dot(n) / dn;
    Some(r0 + rd * t)
}

fn hex(c: &str) -> Value {
    Value::String(c.to_string())
}

impl EditorCore {
    fn view_world(&self) -> &hecs::World {
        &self.play.as_ref().unwrap_or(&self.edit).world
    }

    /// World units per view pixel at `p` (perspective: at its depth).
    pub(crate) fn world_per_px(&self, p: Vec3) -> f32 {
        let h = self.tools.view_px.y.max(1.0);
        if self.view.ortho {
            return self.ortho_height() / h;
        }
        let fwd = self.orbit.rotation() * Vec3::NEG_Z;
        let depth = (p - self.orbit.position()).dot(fwd).max(0.05);
        2.0 * depth * (55f32.to_radians() * 0.5).tan() / h
    }

    /// One three.js gizmo unit in pixels (TransformControls scale with the view height; size 0.9).
    fn unit_px(&self) -> f32 {
        (self.tools.view_px.y * 0.214).clamp(115.0, 220.0)
    }

    fn to_px(&self, p: Vec3) -> Option<Vec2> {
        self.project(p).map(|v| v * self.tools.view_px)
    }

    /// Direction towards the viewer from `p`.
    fn eye_dir(&self, p: Vec3) -> Vec3 {
        if self.view.ortho {
            self.orbit.rotation() * Vec3::Z
        } else {
            (self.orbit.position() - p).normalize_or(Vec3::Z)
        }
    }

    fn ray_px(&self, p: Vec2) -> (Vec3, Vec3) {
        let v = self.tools.view_px.max(Vec2::ONE);
        self.ray(p.x / v.x, p.y / v.y)
    }

    /// Where the handles are: origin, axes (Global = world, Local = the active object's rotation;
    /// Scale is always local, like Unity) and world units per gizmo unit (3D) / pixel (2D).
    /// Frozen while dragging.
    pub(crate) fn gizmo_frame(&self) -> Option<(Vec3, Quat, f32)> {
        if let Some(d) = &self.tools.drag {
            return Some((d.origin, d.basis, d.unit));
        }
        if self.is_playing() {
            return None;
        }
        let e = self.selected?;
        if self.edit.world.get::<&InactiveInHierarchy>(e).is_ok() {
            return None;
        }
        let g = self.edit.world.get::<&GlobalTransform>(e).ok()?.0;
        let (_, rot, pos) = g.to_scale_rotation_translation();
        let roots = self.selection_roots();
        let p = if self.tools.center && roots.len() > 1 {
            let ps: Vec<Vec3> = roots.iter().filter_map(|r| self.edit.world.get::<&GlobalTransform>(*r).ok().map(|g| g.0.w_axis.truncate())).collect();
            ps.iter().copied().sum::<Vec3>() / ps.len().max(1) as f32
        } else {
            pos
        };
        let mut basis = if self.tools.local || self.tools.mode == GizmoMode::Scale { rot.normalize() } else { Quat::IDENTITY };
        if self.view.mode2d && basis != Quat::IDENTITY {
            // the 2D view shows the rotation about Z only
            let x = basis * Vec3::X;
            basis = Quat::from_rotation_z(x.y.atan2(x.x));
        }
        let wpp = self.world_per_px(p);
        let unit = if self.view.mode2d { wpp } else { self.unit_px() * wpp };
        Some((p, basis, unit))
    }

    /// The handles as screen shapes, in draw order (later ones are on top and win picking).
    fn handle_parts(&self) -> Vec<Part> {
        let mut out = Vec::new();
        let Some((o, basis, unit)) = self.gizmo_frame() else { return out };
        let Some(c) = self.to_px(o) else { return out };
        let mode = self.tools.mode;
        if self.view.mode2d {
            let dir = |a: Vec3| self.to_px(o + basis * a * unit * 10.0).map(|p| (p - c).normalize_or(Vec2::X)).unwrap_or(Vec2::X);
            let (ax, ay) = (dir(Vec3::X), dir(Vec3::Y));
            let at = |u: f32, v: f32| c + ax * u + ay * v;
            let arrows = |out: &mut Vec<Part>, len: f32, inset: f32| {
                for (i, d) in [ax, ay].into_iter().enumerate() {
                    let e = c + d * len;
                    let n = d.perp();
                    out.push(part(AXIS + i, Prim::Line(vec![c, e], 2.0, false), C2[i], 1.0));
                    out.push(part(AXIS + i, Prim::Poly(vec![e + d * 14.0, e + n * 6.0, e - n * 6.0], None), C2[i], 1.0));
                }
                let (a, b) = (inset, inset + 18.0);
                out.push(part(PLANE + 2, Prim::Poly(vec![at(a, a), at(b, a), at(b, b), at(a, b)], Some(C2[2])), C2[2], 0.45));
            };
            let ring = |out: &mut Vec<Part>, r: f32| {
                out.push(part(RING + 2, Prim::Circle(c, r, 2.0), C2[2], 1.0));
                out.push(deco(Prim::Line(vec![c, c + ax * r], 1.0, false), C2[2], 0.7));
                out.push(deco(Prim::Circle(c + ax * r, 3.5, 0.0), C2[2], 1.0));
            };
            let square = |r: f32| vec![c + Vec2::new(-r, -r), c + Vec2::new(r, -r), c + Vec2::new(r, r), c + Vec2::new(-r, r)];
            match mode {
                GizmoMode::Move => arrows(&mut out, HANDLE_2D, 0.0),
                GizmoMode::Rotate => ring(&mut out, RING_2D),
                GizmoMode::Scale => {
                    for (i, d) in [ax, ay].into_iter().enumerate() {
                        let e = c + d * HANDLE_2D;
                        out.push(part(AXIS + i, Prim::Line(vec![c, e], 2.0, false), C2[i], 1.0));
                        out.push(part(AXIS + i, Prim::Poly(vec![e + Vec2::new(-5.0, -5.0), e + Vec2::new(5.0, -5.0), e + Vec2::new(5.0, 5.0), e + Vec2::new(-5.0, 5.0)], None), C2[i], 1.0));
                    }
                    out.push(part(CENTER, Prim::Poly(square(7.0), Some("#222222")), WHITE2, 1.0));
                }
                GizmoMode::Transform => {
                    ring(&mut out, T_RING_2D);
                    arrows(&mut out, T_ARROW_2D, 10.0);
                    out.push(part(CENTER, Prim::Poly(square(6.0), Some("#222222")), WHITE2, 1.0));
                }
            }
            return out;
        }

        // ---- 3D: three.js TransformControls
        let eye = self.eye_dir(o);
        let s = unit; // world length of one gizmo unit
        let pts = |list: &[Vec3]| -> Option<Vec<Vec2>> { list.iter().map(|p| self.to_px(*p)).collect() };
        // axes turned towards the camera (three's AXIS_FLIP), hidden when seen end-on
        let axis = |i: usize| {
            let a = basis * AXES[i];
            if a.dot(eye) < 0.0 {
                -a
            } else {
                a
            }
        };
        let visible = |i: usize| (basis * AXES[i]).dot(eye).abs() < 0.99;
        let cube = |center: Vec3, h: f32| -> Option<Vec<Vec2>> {
            let mut v = Vec::with_capacity(8);
            for x in [-1.0f32, 1.0] {
                for y in [-1.0f32, 1.0] {
                    for z in [-1.0f32, 1.0] {
                        v.push(self.to_px(center + basis * Vec3::new(x, y, z) * h)?);
                    }
                }
            }
            Some(hull(v))
        };
        let rings = |out: &mut Vec<Part>, radius: f32| {
            let r = radius * s;
            // free-rotation ball (picked inside the gray circle) and its outline
            let rpx = self.to_px(o + (self.orbit.rotation() * Vec3::X) * r).map(|p| (p - c).length()).unwrap_or(0.0);
            out.push(Part { id: Some(TRACKBALL), prim: Prim::Circle(c, rpx, 0.0), color: GRAY3, alpha: 0.0, pick: true });
            out.push(deco(Prim::Circle(c, rpx, 1.5), GRAY3, 1.0));
            // view ring
            let epx = self.to_px(o + (self.orbit.rotation() * Vec3::X) * r * 1.5).map(|p| (p - c).length()).unwrap_or(0.0);
            out.push(part(VIEW_RING, Prim::Circle(c, epx, 1.5), YELLOW3, 0.5));
            // half rings facing the camera
            for i in 0..3 {
                let (u, v) = (basis * AXES[(i + 1) % 3], basis * AXES[(i + 2) % 3]);
                let mut run: Vec<Vec2> = Vec::new();
                let mut runs: Vec<Vec<Vec2>> = Vec::new();
                let mut starts_front = false;
                for k in 0..=64 {
                    let a = k as f32 / 64.0 * std::f32::consts::TAU;
                    let d = u * a.cos() + v * a.sin();
                    match (d.dot(eye) >= -0.02).then(|| self.to_px(o + d * r)).flatten() {
                        Some(p) => {
                            starts_front |= k == 0;
                            run.push(p)
                        }
                        None if run.len() > 1 => runs.push(std::mem::take(&mut run)),
                        None => run.clear(),
                    }
                }
                let wraps = !run.is_empty();
                if run.len() > 1 {
                    runs.push(run);
                }
                // the front half crossing angle 0 comes out as two runs: join them
                if wraps && starts_front && runs.len() >= 2 {
                    let first = runs.remove(0);
                    runs.last_mut().unwrap().extend(first);
                }
                for run in runs {
                    out.push(part(RING + i, Prim::Line(run, 2.5, false), C3[i], 1.0));
                }
            }
        };
        let arrows = |out: &mut Vec<Part>| {
            // plane squares (normal axis colour), in the quadrant facing the camera
            for i in 0..3 {
                let n = basis * AXES[i];
                if n.dot(eye).abs() < 0.2 {
                    continue;
                }
                let (u, v) = (axis((i + 1) % 3), axis((i + 2) % 3));
                let q = |a: f32, b: f32| o + (u * a + v * b) * s;
                if let Some(p) = pts(&[q(0.075, 0.075), q(0.225, 0.075), q(0.225, 0.225), q(0.075, 0.225)]) {
                    out.push(part(PLANE + i, Prim::Poly(p, None), C3[i], 0.5));
                }
            }
            for i in 0..3 {
                if !visible(i) {
                    continue;
                }
                let a = axis(i);
                if let Some(p) = pts(&[o, o + a * 0.5 * s]) {
                    out.push(part(AXIS + i, Prim::Line(p, 2.0, false), C3[i], 1.0));
                }
                // cone: tip at 0.6, base radius 0.04 at 0.5
                let (u, v) = (basis * AXES[(i + 1) % 3], basis * AXES[(i + 2) % 3]);
                let mut cone: Vec<Vec3> = (0..12).map(|k| {
                    let t = k as f32 / 12.0 * std::f32::consts::TAU;
                    o + a * 0.5 * s + (u * t.cos() + v * t.sin()) * 0.04 * s
                })
                .collect();
                cone.push(o + a * 0.6 * s);
                if let Some(p) = pts(&cone) {
                    out.push(part(AXIS + i, Prim::Poly(hull(p), None), C3[i], 1.0));
                }
            }
        };
        match mode {
            GizmoMode::Move => {
                arrows(&mut out);
                // centre: the octahedron (free move in the view plane)
                let ps: Vec<Vec3> = AXES.iter().flat_map(|a| [o + basis * *a * 0.1 * s, o - basis * *a * 0.1 * s]).collect();
                if let Some(p) = pts(&ps) {
                    out.push(part(CENTER, Prim::Poly(hull(p), None), "#ffffff", 0.35));
                }
            }
            GizmoMode::Rotate => rings(&mut out, 0.5),
            GizmoMode::Transform => {
                rings(&mut out, 0.5 * 1.15 / 0.9);
                arrows(&mut out);
            }
            GizmoMode::Scale => {
                for i in 0..3 {
                    if !visible(i) {
                        continue;
                    }
                    let a = axis(i);
                    if let Some(p) = pts(&[o, o + a * 0.5 * s]) {
                        out.push(part(AXIS + i, Prim::Line(p, 2.0, false), C3[i], 1.0));
                    }
                    if let Some(p) = cube(o + a * 0.5 * s, 0.04 * s) {
                        out.push(part(AXIS + i, Prim::Poly(p, None), C3[i], 1.0));
                    }
                }
                if let Some(p) = cube(o, 0.05 * s) {
                    out.push(part(CENTER, Prim::Poly(p, None), "#ffffff", 0.6));
                }
            }
        }
        out
    }

    /// The handle under a view point (0..1 of a `w × h` view), if any.
    pub fn gizmo_hit(&mut self, x: f32, y: f32, w: f32, h: f32) -> Option<usize> {
        self.tools.view_px = Vec2::new(w, h);
        let p = Vec2::new(x * w, y * h);
        self.handle_parts().iter().rev().find(|pt| pt.pick && hit_part(&pt.prim, p)).and_then(|pt| pt.id)
    }

    /// Pointer over the view (no button): the handle it would grab is highlighted.
    pub fn gizmo_hover(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.tools.hover = if self.tools.drag.is_none() { self.gizmo_hit(x, y, w, h) } else { self.tools.hover };
    }

    /// Starts dragging `handle` from view pixel `at`: remembers the selection's transforms (every
    /// selected object without a selected ancestor follows).
    pub fn gizmo_start(&mut self, handle: usize, at: Vec2) {
        let Some((origin, basis, unit)) = self.gizmo_frame() else { return };
        let w = &self.edit.world;
        let parent_world = |e: Entity| w.get::<&Parent>(e).ok().and_then(|p| w.get::<&GlobalTransform>(p.0).ok().map(|g| g.0)).unwrap_or(Mat4::IDENTITY);
        let mut roots = self.selection_roots();
        if roots.is_empty() {
            roots.extend(self.selected);
        }
        let objects = roots
            .into_iter()
            .filter_map(|e| {
                let t = *w.get::<&Transform>(e).ok()?;
                let g = w.get::<&GlobalTransform>(e).ok()?.0;
                Some(DragObject { entity: e, start: t, world: g, parent: parent_world(e) })
            })
            .collect();
        let (r0, rd) = self.ray_px(at);
        let axis_start = if handle < 3 { axis_param(origin, basis * AXES[handle], r0, rd).unwrap_or(0.0) } else { 0.0 };
        let n = self.drag_plane_normal(handle, origin, basis);
        let grab = n.and_then(|n| plane_hit(origin, n, r0, rd));
        self.tools.dragging = Some(handle);
        self.tools.hover = Some(handle);
        self.tools.drag = Some(GizmoDrag { handle, origin, basis, unit, start: at, cur: at, axis_start, grab, last: grab.map(|g| g - origin), angle: 0.0, ball: Quat::IDENTITY, objects, label: String::new() });
    }

    /// The plane a handle drags on (planes, the free centre, rings, the view ring).
    fn drag_plane_normal(&self, handle: usize, origin: Vec3, basis: Quat) -> Option<Vec3> {
        let toward = self.eye_dir(origin);
        match handle {
            h if (PLANE..PLANE + 3).contains(&h) => Some(basis * AXES[h - PLANE]),
            CENTER if self.tools.mode == GizmoMode::Move => Some(toward),
            h if (RING..RING + 3).contains(&h) => {
                let n = basis * AXES[h - RING];
                // a ring seen edge-on drags along its tangent instead
                (n.dot(toward).abs() > 0.1).then_some(n)
            }
            VIEW_RING => Some(toward),
            _ => None,
        }
    }

    pub fn gizmo_stop(&mut self) {
        self.tools.dragging = None;
        self.tools.drag = None;
    }

    /// The pointer moved by (dx, dy) pixels of a `w × h` view during a drag; `snap` (Ctrl held)
    /// rounds to the snap increments (Unity's Increment Snapping).
    pub fn gizmo_drag(&mut self, dx: f32, dy: f32, w: f32, h: f32, snap: bool) {
        self.tools.view_px = Vec2::new(w, h);
        let Some(d) = self.tools.drag.as_mut() else { return };
        d.cur += Vec2::new(dx, dy);
        let (handle, origin, basis, unit, start, cur) = (d.handle, d.origin, d.basis, d.unit, d.start, d.cur);
        let steps = self.tools.snap;
        let round = |v: f32, step: f32| if snap && step > 0.0 { (v / step).round() * step } else { v };
        let mode = self.tools.mode;
        let (r0, rd) = self.ray_px(cur);
        let plane_n = self.drag_plane_normal(handle, origin, basis);
        let cam = self.orbit.rotation();
        let c_px = self.to_px(origin).unwrap_or(start);
        let handle_px = |a: Vec3, len: f32| self.to_px(origin + a * len).map(|p| p - c_px).unwrap_or(Vec2::X * 80.0);
        let mode2d = self.view.mode2d;
        // screen directions: an edge-on ring's tangent, a scale handle (its length = ×2)
        let ring_n = if (RING..RING + 3).contains(&handle) { basis * AXES[handle - RING] } else { cam * Vec3::Z };
        let tangent_px = handle_px(ring_n.cross(cam * Vec3::Z).normalize_or(Vec3::X), unit * 10.0).normalize_or(Vec2::X);
        let scale_px = if handle < 3 { handle_px(basis * AXES[handle], if mode2d { HANDLE_2D * unit } else { 0.5 * unit }) } else { Vec2::X };
        let d = self.tools.drag.as_mut().unwrap();

        enum Change {
            Move(Vec3),
            Rotate(Quat),
            Scale(Vec3),
        }
        let scale_handle = |i: usize| mode == GizmoMode::Scale && i < 3;
        let change = match handle {
            i if i < 3 && !scale_handle(i) => {
                let a = basis * AXES[i];
                let s = axis_param(origin, a, r0, rd).map(|s| s - d.axis_start).unwrap_or(0.0);
                Change::Move(a * round(s, steps[0]))
            }
            i if (PLANE..PLANE + 3).contains(&i) || (i == CENTER && mode == GizmoMode::Move) => {
                let (Some(n), Some(g)) = (plane_n, d.grab) else { return };
                let Some(p) = plane_hit(origin, n, r0, rd) else { return };
                let delta = basis.inverse() * (p - g);
                let delta = Vec3::new(round(delta.x, steps[0]), round(delta.y, steps[0]), round(delta.z, steps[0]));
                Change::Move(basis * delta)
            }
            i if (RING..RING + 3).contains(&i) || i == VIEW_RING => {
                let n = if i == VIEW_RING { plane_n.unwrap_or(cam * Vec3::Z) } else { basis * AXES[i - RING] };
                match (plane_n, d.last) {
                    (Some(pn), Some(last)) => {
                        if let Some(p) = plane_hit(origin, pn, r0, rd) {
                            let v = p - origin;
                            if v.length_squared() > 1e-10 && last.length_squared() > 1e-10 {
                                d.angle += n.dot(last.cross(v)).atan2(last.dot(v));
                                d.last = Some(v);
                            }
                        }
                    }
                    _ => {
                        // edge-on ring: drag along its tangent on screen
                        d.angle = (cur - start).dot(tangent_px) * 0.01;
                    }
                }
                let a = round(d.angle, steps[1].to_radians());
                d.label = format!("{:.1}°", a.to_degrees());
                Change::Rotate(Quat::from_axis_angle(n.normalize_or(Vec3::Y), a))
            }
            TRACKBALL => {
                let delta = Vec2::new(dx, dy);
                let q = Quat::from_axis_angle(cam * Vec3::Y, delta.x * 0.01) * Quat::from_axis_angle(cam * Vec3::X, delta.y * 0.01);
                d.ball = (q * d.ball).normalize();
                Change::Rotate(d.ball)
            }
            i if i < 3 => {
                // scale along an axis: the handle's screen length = ×2
                let hp = scale_px;
                let len = hp.length().max(8.0);
                let f = (1.0 + round((cur - start).dot(hp / len) / len, steps[2])).max(0.01);
                d.label = format!("{f:.2}");
                let mut v = Vec3::ONE;
                v[i] = f;
                Change::Scale(v)
            }
            CENTER => {
                let t = cur - start;
                let f = (1.0 + round((t.x - t.y) / 100.0, steps[2])).max(0.01);
                d.label = format!("{f:.2}");
                Change::Scale(Vec3::splat(f))
            }
            _ => return,
        };
        let center = self.tools.center;
        let d = self.tools.drag.as_ref().unwrap();
        let mut writes = Vec::with_capacity(d.objects.len());
        for o in &d.objects {
            let (_, start_rot, start_pos) = o.world.to_scale_rotation_translation();
            let inv_parent = o.parent.inverse();
            let (_, parent_rot, _) = o.parent.to_scale_rotation_translation();
            let mut t = o.start;
            match &change {
                Change::Move(v) => t.position += inv_parent.transform_vector3(*v),
                Change::Rotate(r) => {
                    t.rotation = (parent_rot.inverse() * (*r * start_rot)).normalize();
                    // Center: the objects turn about the centre; Pivot: each about its own pivot
                    if center {
                        t.position += inv_parent.transform_vector3(origin + *r * (start_pos - origin) - start_pos);
                    }
                }
                Change::Scale(f) => {
                    t.scale = o.start.scale * *f;
                    if center {
                        let off = basis.inverse() * (start_pos - origin);
                        t.position += inv_parent.transform_vector3(basis * (off * *f) - basis * off);
                    }
                }
            }
            writes.push((o.entity, t));
        }
        for (e, t) in writes {
            if let Ok(mut cur) = self.edit.world.get::<&mut Transform>(e) {
                *cur = t;
            }
        }
    }

    /// Selected entities' world boxes (renderers of the entity and its children), as 8 corners
    /// of an oriented box per renderer.
    fn selection_boxes(&self, gfx: &dyn GraphicsBackend) -> Vec<[Vec3; 8]> {
        let w = self.view_world();
        let children: Vec<(Entity, Entity)> = w.query::<&Parent>().iter().map(|(c, p)| (p.0, c)).collect();
        let mut out = Vec::new();
        for &e in &self.selection {
            if w.get::<&InactiveInHierarchy>(e).is_ok() {
                continue;
            }
            let mut stack = vec![e];
            let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            let mut corners2d: Option<[Vec3; 8]> = None;
            while let Some(x) = stack.pop() {
                if let (Ok(r), Ok(g)) = (w.get::<&MeshRenderer>(x), w.get::<&GlobalTransform>(x)) {
                    if let Some(b) = gfx.mesh_bounds(r.mesh) {
                        let mut k = 0;
                        let mut cs = [Vec3::ZERO; 8];
                        for sx in [-1.0f32, 1.0] {
                            for sy in [-1.0f32, 1.0] {
                                for sz in [-1.0f32, 1.0] {
                                    let p = g.0.transform_point3(b.center + b.half_extents * Vec3::new(sx, sy, sz));
                                    min = min.min(p);
                                    max = max.max(p);
                                    cs[k] = p;
                                    k += 1;
                                }
                            }
                        }
                        if x == e {
                            corners2d = Some(cs);
                        }
                    }
                }
                stack.extend(children.iter().filter(|(p, _)| *p == x).map(|(_, c)| *c));
            }
            if min.x > max.x {
                continue;
            }
            // 2D view: the sprite's own (turned) rectangle, like PiaEngine's 2D outlines
            if self.view.mode2d {
                if let Some(cs) = corners2d {
                    out.push(cs);
                    continue;
                }
            }
            let mut cs = [Vec3::ZERO; 8];
            let mut k = 0;
            for x in [min.x, max.x] {
                for y in [min.y, max.y] {
                    for z in [min.z, max.z] {
                        cs[k] = Vec3::new(x, y, z);
                        k += 1;
                    }
                }
            }
            out.push(cs);
        }
        out
    }

    /// Camera / light icons under a view point (0..1): selects objects without renderers.
    pub fn icon_hit(&self, x: f32, y: f32) -> Option<Entity> {
        if !self.view.gizmos || self.view.mode2d {
            return None;
        }
        let p = Vec2::new(x, y) * self.tools.view_px;
        let w = &self.edit.world;
        let mut best: Option<(f32, Entity)> = None;
        for (e, g) in w.query::<&GlobalTransform>().without::<&DontSave>().without::<&InactiveInHierarchy>().iter() {
            let is_icon = w.get::<&Camera>(e).is_ok() || w.get::<&DirectionalLight>(e).is_ok() || w.get::<&PointLight>(e).is_ok() || w.get::<&SpotLight>(e).is_ok();
            if !is_icon {
                continue;
            }
            let Some(s) = self.to_px(g.0.w_axis.truncate()) else { continue };
            let d = (s - p).length();
            if d < 13.0 && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, e));
            }
        }
        best.map(|(_, e)| e)
    }

    /// Everything the UI draws over the Scene view, in pixels of a `w × h` view:
    /// `[{ k: "l" (polyline) | "p" (polygon) | "c" (circle) | "t" (text) | "i" (icon), … }]`.
    pub fn scene_overlay(&mut self, gfx: &dyn GraphicsBackend, w: f32, h: f32, pointer: Option<Vec2>) -> Value {
        self.tools.view_px = Vec2::new(w, h);
        let mut items: Vec<Value> = Vec::new();
        if !self.view.gizmos {
            return Value::Array(items);
        }
        let world = self.view_world();
        // 2D: camera rectangles + camera glyphs (PiaEngine's 2D view); 3D: camera / light icons
        for (e, g) in world.query::<&GlobalTransform>().without::<&DontSave>().without::<&InactiveInHierarchy>().iter() {
            let pos = g.0.w_axis.truncate();
            if let Ok(cam) = world.get::<&Camera>(e) {
                if self.view.mode2d {
                    let aspect = 400.0 / 240.0 * (cam.viewport.width / cam.viewport.height.max(1e-3));
                    let hh = match cam.projection {
                        Projection::Orthographic { height, .. } => height * 0.5,
                        Projection::Perspective { fov_y_radians, .. } => pos.z.abs() * (fov_y_radians * 0.5).tan(),
                    };
                    let hw = hh * aspect;
                    let sel = self.selection.contains(&e);
                    let rect: Option<Vec<Vec2>> = [(-hw, hh), (hw, hh), (hw, -hh), (-hw, -hh)].iter().map(|(x, y)| self.to_px(Vec3::new(pos.x + x, pos.y + y, 0.0))).collect();
                    if let Some(r) = rect {
                        items.push(json!({ "k": "l", "p": r.iter().flat_map(|p| [p.x, p.y]).collect::<Vec<_>>(), "s": "#ffffff", "w": 1, "a": if sel { 0.9 } else { 0.45 }, "z": true }));
                    }
                    if let Some(s) = self.to_px(Vec3::new(pos.x, pos.y, 0.0)) {
                        items.push(json!({ "k": "i", "n": "camera2d", "p": [s.x, s.y] }));
                    }
                    continue;
                }
                if let Some(s) = self.to_px(pos) {
                    items.push(json!({ "k": "i", "n": "camera", "p": [s.x, s.y] }));
                }
            } else if !self.view.mode2d && (world.get::<&DirectionalLight>(e).is_ok() || world.get::<&PointLight>(e).is_ok() || world.get::<&SpotLight>(e).is_ok()) {
                if let Some(s) = self.to_px(pos) {
                    items.push(json!({ "k": "i", "n": "light", "p": [s.x, s.y] }));
                }
            }
        }
        // selection outlines (orange): the world box in 3D, the turned rectangle in 2D
        const EDGES: [(usize, usize); 12] = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
        for cs in self.selection_boxes(gfx) {
            if self.view.mode2d {
                // the outline of the object's turned box as seen (a sprite: its 4 corners)
                let face: Option<Vec<Vec2>> = cs.iter().map(|p| self.to_px(*p)).collect();
                if let Some(f) = face.map(hull) {
                    items.push(json!({ "k": "l", "p": f.iter().flat_map(|p| [p.x, p.y]).collect::<Vec<_>>(), "s": SELECTION, "w": 1.5, "a": 0.95, "z": true }));
                }
                continue;
            }
            let mut segs = Vec::new();
            for (a, b) in EDGES {
                if let (Some(p), Some(q)) = (self.to_px(cs[a]), self.to_px(cs[b])) {
                    segs.extend([p.x, p.y, q.x, q.y]);
                }
            }
            items.push(json!({ "k": "s", "p": segs, "s": SELECTION, "w": 1, "a": 0.85 }));
        }
        // handles
        let hot = self.tools.dragging.or(self.tools.hover);
        let yellow = if self.view.mode2d { YELLOW2 } else { YELLOW3 };
        for p in self.handle_parts() {
            if p.alpha <= 0.0 {
                continue;
            }
            let lit = p.id.is_some() && p.id == hot;
            let (color, alpha) = if lit { (yellow, p.alpha.max(0.6)) } else { (p.color, p.alpha) };
            let flat = |v: &[Vec2]| v.iter().flat_map(|p| [p.x, p.y]).collect::<Vec<_>>();
            items.push(match &p.prim {
                Prim::Line(pts, w, closed) => json!({ "k": "l", "p": flat(pts), "s": hex(color), "w": if lit { w + 1.0 } else { *w }, "a": alpha, "z": closed }),
                Prim::Poly(pts, stroke) => json!({ "k": "p", "p": flat(pts), "f": hex(color), "fa": alpha, "s": stroke.map(|s| if lit { yellow } else { s }), "w": 1 }),
                Prim::Circle(c, r, w) if *w == 0.0 => json!({ "k": "c", "p": [c.x, c.y, r], "f": hex(color), "fa": alpha }),
                Prim::Circle(c, r, w) => json!({ "k": "c", "p": [c.x, c.y, r], "s": hex(color), "w": if lit { w + 1.0 } else { *w }, "a": alpha }),
            });
        }
        // live value next to the pointer (angle, scale)
        if let (Some(d), Some(p)) = (&self.tools.drag, pointer) {
            if !d.label.is_empty() {
                items.push(json!({ "k": "t", "p": [p.x + 16.0, p.y + 14.0], "t": d.label }));
            }
        }
        Value::Array(items)
    }
}

