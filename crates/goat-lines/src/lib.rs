//! Lines and trails (Unity's LineRenderer / TrailRenderer).
//!
//! - [`LineRenderer`]: a ribbon through `positions` (world or local space, optionally looped),
//!   width from `start_width` to `end_width`, color from `start_color` to `end_color`.
//! - [`TrailRenderer`]: a ribbon through the recent path of its object: a point is added when the
//!   object moved `min_vertex_distance`, points live `time` seconds; width / color go from the
//!   object (start) to the oldest point (end). `emitting = false` detaches it; `autodestruct`
//!   despawns the object once it stood still and the trail is gone.
//! - `alignment`: `View` (faces the camera) or `TransformZ` (flat, facing the object's Z).
//!   `texture_mode`: `Stretch` (the texture once along the line) or `Tile` (repeat every width).
//!
//! [`LinePlugin`] builds the quads every frame and draws them through the sprite [`Batcher`]
//! (unlit, alpha or additive), so lines cost one draw per texture on the 3DS too. Lines draw in
//! Edit Mode; trails record in Play Mode.

use goat_assets::AssetServer;
use goat_core::math::{Mat4, Quat, Vec3};
use goat_core::Color;
use goat_gfx::{BlendMode, Display, TextureId};
use goat_runtime::{Camera, Context, Entity, GlobalTransform, Plugin};
use goat_sprites::{Batcher, Quad};
use serde::{Deserialize, Serialize};

/// How the ribbon is oriented.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineAlignment {
    /// Faces the main camera.
    #[default]
    View,
    /// Lies in the object's XY plane (faces its Z axis).
    TransformZ,
}

/// How the texture maps along the line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineTextureMode {
    /// Once along the whole line.
    #[default]
    Stretch,
    /// Repeats every `width` units.
    Tile,
}

/// A line through points (Unity's LineRenderer).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LineRenderer {
    pub positions: Vec<[f32; 3]>,
    /// Positions are world coordinates (else local to the object).
    pub use_world_space: bool,
    /// Connect the last point back to the first.
    #[serde(rename = "loop")]
    pub looped: bool,
    pub start_width: f32,
    pub end_width: f32,
    pub width_multiplier: f32,
    pub start_color: Color,
    pub end_color: Color,
    pub alignment: LineAlignment,
    pub texture_mode: LineTextureMode,
    /// Texture asset key (white when empty).
    pub texture: String,
    pub blend: BlendMode,
    pub layer: u8,
    pub sorting_order: i32,
    pub enabled: bool,
}

impl Default for LineRenderer {
    fn default() -> Self {
        LineRenderer {
            positions: vec![[0.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            use_world_space: true,
            looped: false,
            start_width: 0.2,
            end_width: 0.2,
            width_multiplier: 1.0,
            start_color: Color::WHITE,
            end_color: Color::WHITE,
            alignment: LineAlignment::View,
            texture_mode: LineTextureMode::Stretch,
            texture: String::new(),
            blend: BlendMode::Alpha,
            layer: 0,
            sorting_order: 0,
            enabled: true,
        }
    }
}

impl LineRenderer {
    pub fn position_count(&self) -> usize {
        self.positions.len()
    }
    pub fn set_position(&mut self, i: usize, p: Vec3) {
        if i >= self.positions.len() {
            self.positions.resize(i + 1, [0.0; 3]);
        }
        self.positions[i] = p.into();
    }
    pub fn set_positions(&mut self, points: &[Vec3]) {
        self.positions = points.iter().map(|p| (*p).into()).collect();
    }
}

/// A ribbon behind a moving object (Unity's TrailRenderer).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TrailRenderer {
    /// Seconds a point lives.
    pub time: f32,
    /// A point is added once the object moved this far.
    pub min_vertex_distance: f32,
    pub start_width: f32,
    pub end_width: f32,
    pub width_multiplier: f32,
    pub start_color: Color,
    pub end_color: Color,
    /// Add points (off: the trail stays behind and fades out).
    pub emitting: bool,
    /// Despawn the object once it stood still for `time` and the trail is gone.
    pub autodestruct: bool,
    pub alignment: LineAlignment,
    pub texture_mode: LineTextureMode,
    pub texture: String,
    pub blend: BlendMode,
    pub layer: u8,
    pub sorting_order: i32,
    pub enabled: bool,
    /// Recorded points (world position, time added), oldest first.
    #[serde(skip)]
    pub points: Vec<(Vec3, f32)>,
    #[serde(skip)]
    clock: f32,
    #[serde(skip)]
    still: f32,
}

impl Default for TrailRenderer {
    fn default() -> Self {
        TrailRenderer {
            time: 5.0,
            min_vertex_distance: 0.1,
            start_width: 0.5,
            end_width: 0.0,
            width_multiplier: 1.0,
            start_color: Color::WHITE,
            end_color: Color::rgba(1.0, 1.0, 1.0, 0.0),
            emitting: true,
            autodestruct: false,
            alignment: LineAlignment::View,
            texture_mode: LineTextureMode::Stretch,
            texture: String::new(),
            blend: BlendMode::Alpha,
            layer: 0,
            sorting_order: 0,
            enabled: true,
            points: Vec::new(),
            clock: 0.0,
            still: 0.0,
        }
    }
}

impl TrailRenderer {
    /// Forgets every point (Unity's Clear, e.g. after a teleport).
    pub fn clear(&mut self) {
        self.points.clear();
    }
    pub fn position_count(&self) -> usize {
        self.points.len()
    }

    /// Records the object's position (`dt` seconds later). Returns true when the object should
    /// be destroyed (autodestruct).
    pub fn record(&mut self, position: Vec3, dt: f32) -> bool {
        self.clock += dt;
        let moved = self.points.last().is_none_or(|(p, _)| (*p - position).length() >= self.min_vertex_distance);
        if self.emitting && moved {
            self.points.push((position, self.clock));
            self.still = 0.0;
        } else {
            self.still += dt;
        }
        let clock = self.clock;
        let time = self.time.max(0.0);
        self.points.retain(|(_, t)| clock - *t <= time);
        self.autodestruct && self.still >= time && self.points.len() <= 1
    }
}

fn lerp(a: Color, b: Color, t: f32) -> Color {
    Color::rgba(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t)
}

/// Style shared by lines and trails when building quads.
struct Style {
    start_width: f32,
    end_width: f32,
    start_color: Color,
    end_color: Color,
    texture_mode: LineTextureMode,
    texture: TextureId,
    blend: BlendMode,
    layer: u8,
    sorting_order: i32,
}

/// Quads of a ribbon through `pts` (start = first point). `face` = the direction the ribbon
/// faces: the view direction (`View`) or the object's Z (`TransformZ`).
fn ribbon(pts: &[Vec3], face: impl Fn(Vec3) -> Vec3, cam_fwd: Vec3, s: &Style, out: &mut Vec<Quad>) {
    if pts.len() < 2 {
        return;
    }
    // distances along the line
    let mut along = Vec::with_capacity(pts.len());
    let mut total = 0.0;
    along.push(0.0);
    for w in pts.windows(2) {
        total += (w[1] - w[0]).length();
        along.push(total);
    }
    if total <= 1e-6 {
        return;
    }
    let width = |f: f32| s.start_width + (s.end_width - s.start_width) * f;
    // per-point side vector (averaged at joints: no gaps between segments)
    let n = pts.len();
    let sides: Vec<Vec3> = (0..n)
        .map(|i| {
            let dir = if i == 0 {
                pts[1] - pts[0]
            } else if i == n - 1 {
                pts[n - 1] - pts[n - 2]
            } else {
                (pts[i] - pts[i - 1]).normalize_or_zero() + (pts[i + 1] - pts[i]).normalize_or_zero()
            };
            let f = face(pts[i]);
            f.cross(dir).normalize_or_zero() * (width(along[i] / total) * 0.5)
        })
        .collect();
    for i in 0..n - 1 {
        let (a, b) = (pts[i], pts[i + 1]);
        if (b - a).length_squared() < 1e-12 {
            continue;
        }
        let (fa, fb) = (along[i] / total, along[i + 1] / total);
        let (ua, ub) = match s.texture_mode {
            LineTextureMode::Stretch => (fa, fb),
            LineTextureMode::Tile => {
                let w = ((s.start_width + s.end_width) * 0.5).max(1e-3);
                (along[i] / w, along[i + 1] / w)
            }
        };
        let c = lerp(s.start_color, s.end_color, (fa + fb) * 0.5);
        let (sa, sb) = (sides[i], sides[i + 1]);
        out.push(Quad {
            sorting_order: s.sorting_order,
            layer: s.layer,
            texture: s.texture,
            blend: s.blend,
            z: -((a + b) * 0.5).dot(cam_fwd),
            corners: [a - sa, b - sb, b + sb, a + sa],
            normal: face((a + b) * 0.5),
            uv: [[ua, 1.0], [ub, 1.0], [ub, 0.0], [ua, 0.0]],
            color: [c.r, c.g, c.b, c.a],
        });
    }
}

/// Records trails (Play Mode) and draws lines + trails every frame.
pub struct LinePlugin {
    batcher: Batcher,
    white: Option<TextureId>,
}

impl Default for LinePlugin {
    fn default() -> Self {
        LinePlugin { batcher: Batcher::new("Lines"), white: None }
    }
}

impl Plugin for LinePlugin {
    fn name(&self) -> &str {
        "LinePlugin"
    }
    fn in_edit_mode(&self) -> bool {
        true
    }
    fn build(&mut self, _ctx: &mut Context) {
        #[cfg(feature = "scene")]
        {
            use goat_scene::{serde_codec, ComponentInfo as I};
            let r = goat_scene::registry(_ctx);
            r.component(serde_codec::<LineRenderer>("LineRenderer")).component(serde_codec::<TrailRenderer>("TrailRenderer"));
            r.describe("LineRenderer", I::new("Effects/Line Renderer").asset("texture", "texture").options("alignment", &["View", "TransformZ"]).options("texture_mode", &["Stretch", "Tile"]).options("blend", &["Alpha", "Additive"]).kind("layer", "layer"));
            r.describe(
                "TrailRenderer",
                I::new("Effects/Trail Renderer").asset("texture", "texture").options("alignment", &["View", "TransformZ"]).options("texture_mode", &["Stretch", "Tile"]).options("blend", &["Alpha", "Additive"]).kind("layer", "layer").tip("time", "Seconds a point lives").tip("min_vertex_distance", "A point is added once the object moved this far"),
            );
        }
    }

    /// Trails record the object's path (Play Mode only: `update` doesn't run while editing).
    fn update(&mut self, ctx: &mut Context) {
        let dt = ctx.time.delta;
        let mut doomed: Vec<Entity> = Vec::new();
        for (e, (t, g)) in ctx.world.query::<(&mut TrailRenderer, &GlobalTransform)>().without::<&goat_runtime::InactiveInHierarchy>().iter() {
            if t.enabled && t.record(g.0.w_axis.truncate(), dt) {
                doomed.push(e);
            }
        }
        for e in doomed {
            let _ = ctx.world.despawn(e);
        }
    }

    fn late_update(&mut self, ctx: &mut Context) {
        let any = ctx.world.query::<&LineRenderer>().without::<&goat_runtime::InactiveInHierarchy>().iter().next().is_some() || ctx.world.query::<&TrailRenderer>().without::<&goat_runtime::InactiveInHierarchy>().iter().any(|(_, t)| !t.points.is_empty());
        if !any {
            self.batcher.submit(ctx, Vec::new());
            return;
        }
        if self.white.is_none() {
            self.white = ctx.gfx.create_texture(&goat_gfx::TextureData::solid(Color::WHITE)).ok();
        }
        let Some(white) = self.white else { return };
        // the main screen's first camera (view-facing ribbons, back-to-front order)
        let (cam_pos, cam_rot) = ctx
            .world
            .query::<(&Camera, &GlobalTransform)>()
            .iter()
            .filter(|(_, (c, _))| c.display == Display::Main)
            .min_by_key(|(_, (c, _))| c.depth)
            .map(|(_, (_, g))| {
                let (_, r, t) = g.0.to_scale_rotation_translation();
                (t, r)
            })
            .unwrap_or((Vec3::ZERO, Quat::IDENTITY));
        let cam_fwd = cam_rot * Vec3::Z;
        // textures (asset keys → ids) first
        let mut keys: Vec<String> = Vec::new();
        for (_, l) in ctx.world.query::<&LineRenderer>().without::<&goat_runtime::InactiveInHierarchy>().iter() {
            keys.push(l.texture.clone());
        }
        for (_, t) in ctx.world.query::<&TrailRenderer>().without::<&goat_runtime::InactiveInHierarchy>().iter() {
            keys.push(t.texture.clone());
        }
        let mut tex: std::collections::HashMap<String, TextureId> = std::collections::HashMap::new();
        for k in keys {
            if k.is_empty() || tex.contains_key(&k) {
                continue;
            }
            let id = ctx.resources.get_mut::<AssetServer>().and_then(|a| a.texture(ctx.gfx, &k).ok()).unwrap_or(white);
            tex.insert(k, id);
        }
        let texture_of = |k: &str| if k.is_empty() { white } else { tex.get(k).copied().unwrap_or(white) };
        let face_view = |p: Vec3| (cam_pos - p).normalize_or(cam_fwd);

        let mut quads = Vec::new();
        for (_, (l, g)) in ctx.world.query::<(&LineRenderer, Option<&GlobalTransform>)>().without::<&goat_runtime::InactiveInHierarchy>().iter() {
            if !l.enabled || l.positions.len() < 2 {
                continue;
            }
            let m = g.map(|g| g.0).unwrap_or(Mat4::IDENTITY);
            let mut pts: Vec<Vec3> = l.positions.iter().map(|p| if l.use_world_space { Vec3::from(*p) } else { m.transform_point3(Vec3::from(*p)) }).collect();
            if l.looped {
                pts.push(pts[0]);
            }
            let style = Style {
                start_width: l.start_width * l.width_multiplier,
                end_width: l.end_width * l.width_multiplier,
                start_color: l.start_color,
                end_color: l.end_color,
                texture_mode: l.texture_mode,
                texture: texture_of(&l.texture),
                blend: l.blend,
                layer: l.layer,
                sorting_order: l.sorting_order,
            };
            let z = m.transform_vector3(Vec3::Z).normalize_or(Vec3::Z);
            match l.alignment {
                LineAlignment::View => ribbon(&pts, face_view, cam_fwd, &style, &mut quads),
                LineAlignment::TransformZ => ribbon(&pts, |_| z, cam_fwd, &style, &mut quads),
            }
        }
        for (_, (t, g)) in ctx.world.query::<(&TrailRenderer, &GlobalTransform)>().without::<&goat_runtime::InactiveInHierarchy>().iter() {
            if !t.enabled || t.points.is_empty() {
                continue;
            }
            // newest first (start = at the object), the object's current position as the head
            let head = g.0.w_axis.truncate();
            let mut pts: Vec<Vec3> = Vec::with_capacity(t.points.len() + 1);
            if t.emitting {
                pts.push(head);
            }
            pts.extend(t.points.iter().rev().map(|(p, _)| *p));
            pts.dedup_by(|a, b| (*a - *b).length_squared() < 1e-10);
            let style = Style {
                start_width: t.start_width * t.width_multiplier,
                end_width: t.end_width * t.width_multiplier,
                start_color: t.start_color,
                end_color: t.end_color,
                texture_mode: t.texture_mode,
                texture: texture_of(&t.texture),
                blend: t.blend,
                layer: t.layer,
                sorting_order: t.sorting_order,
            };
            let z = g.0.transform_vector3(Vec3::Z).normalize_or(Vec3::Z);
            match t.alignment {
                LineAlignment::View => ribbon(&pts, face_view, cam_fwd, &style, &mut quads),
                LineAlignment::TransformZ => ribbon(&pts, |_| z, cam_fwd, &style, &mut quads),
            }
        }
        self.batcher.submit(ctx, quads);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> Style {
        Style {
            start_width: 1.0,
            end_width: 0.0,
            start_color: Color::WHITE,
            end_color: Color::rgba(1.0, 1.0, 1.0, 0.0),
            texture_mode: LineTextureMode::Stretch,
            texture: goat_gfx::GraphicsBackend::create_texture(&mut goat_gfx::NullBackend::new(), &goat_gfx::TextureData::solid(Color::WHITE)).unwrap(),
            blend: BlendMode::Alpha,
            layer: 0,
            sorting_order: 0,
        }
    }

    #[test]
    fn ribbons_taper_and_fade_along_the_line() {
        let pts = [Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)];
        let mut q = Vec::new();
        ribbon(&pts, |_| Vec3::Z, Vec3::Z, &style(), &mut q);
        assert_eq!(q.len(), 2);
        // width 1 at the start (±0.5 in Y), 0 at the end; alpha fades
        assert!((q[0].corners[3].y - 0.5).abs() < 1e-5 && (q[1].corners[2].y).abs() < 1e-5);
        assert!(q[0].color[3] > q[1].color[3]);
        assert_eq!(q[0].uv[0][0], 0.0);
        assert_eq!(q[1].uv[1][0], 1.0);
    }

    #[test]
    fn trails_record_age_and_autodestruct() {
        let mut t = TrailRenderer { time: 1.0, min_vertex_distance: 0.5, autodestruct: true, ..Default::default() };
        for i in 0..10 {
            assert!(!t.record(Vec3::new(i as f32 * 0.25, 0.0, 0.0), 0.1));
        }
        // a point every 0.5 units: 0, 0.5, 1.0... (every other frame)
        assert_eq!(t.points.len(), 5);
        // standing still: points age out, then the object is destroyed
        let mut destroyed = false;
        for _ in 0..30 {
            destroyed |= t.record(Vec3::new(2.25, 0.0, 0.0), 0.1);
        }
        assert!(t.points.len() <= 1 && destroyed);
        // not emitting: no new points
        let mut t2 = TrailRenderer { emitting: false, ..Default::default() };
        t2.record(Vec3::ONE, 0.1);
        assert!(t2.points.is_empty());
    }
}
