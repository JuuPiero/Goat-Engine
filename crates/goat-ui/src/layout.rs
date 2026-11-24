//! Auto layout (Unity's Horizontal / Vertical / Grid Layout Group, Content Size Fitter, Layout
//! Element) and RectMask2D-style clipping.

use goat_core::math::{Vec2, Vec3};
use goat_runtime::{Entity, World};
use goat_sprites::Quad;
use serde::{Deserialize, Serialize};

use crate::RectTransform;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutKind {
    #[default]
    Vertical,
    Horizontal,
    Grid,
}

/// Places the children one after another (Unity's Horizontal / Vertical / Grid Layout Group).
/// Children keep their RectTransform size unless the group controls it.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiLayoutGroup {
    pub kind: LayoutKind,
    /// Left, right, top, bottom (pixels).
    pub padding: [f32; 4],
    /// Between children (x for horizontal / grid columns, y for vertical / grid rows).
    pub spacing: Vec2,
    /// Where the children sit when they don't fill the group: (0, 1) = upper left, (0.5, 0.5) =
    /// middle center.
    pub child_alignment: Vec2,
    /// Grid: every cell's size.
    pub cell_size: Vec2,
    /// Grid: columns (0 = as many as fit).
    pub columns: u32,
    /// Children's width / height set by the group (filling the group across the main axis).
    pub control_width: bool,
    pub control_height: bool,
    /// Children share the leftover space along the main axis.
    pub expand_width: bool,
    pub expand_height: bool,
}

impl Default for UiLayoutGroup {
    fn default() -> Self {
        UiLayoutGroup {
            kind: LayoutKind::Vertical,
            padding: [0.0; 4],
            spacing: Vec2::ZERO,
            child_alignment: Vec2::new(0.0, 1.0),
            cell_size: Vec2::new(100.0, 100.0),
            columns: 0,
            control_width: false,
            control_height: false,
            expand_width: false,
            expand_height: false,
        }
    }
}

/// Overrides a child's size in a layout group (Unity's Layout Element).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiLayoutElement {
    /// Skipped by the group (positioned by its own RectTransform).
    pub ignore_layout: bool,
    /// Preferred size (negative = the RectTransform's size).
    pub preferred: Vec2,
    /// Share of leftover space when the group expands children (0 = none).
    pub flexible: Vec2,
}

impl Default for UiLayoutElement {
    fn default() -> Self {
        UiLayoutElement { ignore_layout: false, preferred: Vec2::splat(-1.0), flexible: Vec2::ONE }
    }
}

/// Resizes the entity to its layout group's preferred size (Unity's Content Size Fitter, with
/// Preferred Size on the enabled axes): scroll view contents, auto-sized panels.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiContentSizeFitter {
    pub horizontal: bool,
    pub vertical: bool,
}

/// Clips every descendant to this rect (Unity's RectMask2D; Mask with an image behaves the same
/// for rectangles). Scroll view viewports use it.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiMask {
    /// Draw this entity's own image too (Unity's Mask "Show Mask Graphic").
    pub show_graphic: bool,
}

impl Default for UiMask {
    fn default() -> Self {
        UiMask { show_graphic: true }
    }
}

pub(crate) struct Item {
    pub entity: Entity,
    pub preferred: Vec2,
    pub flexible: Vec2,
}

pub(crate) fn items(world: &World, children: &[Entity]) -> Vec<Item> {
    children
        .iter()
        .filter_map(|&c| {
            let rt = *world.get::<&RectTransform>(c).ok()?;
            let le = world.get::<&UiLayoutElement>(c).ok().map(|l| *l).unwrap_or_default();
            if le.ignore_layout {
                return None;
            }
            let pref = Vec2::new(if le.preferred.x >= 0.0 { le.preferred.x } else { rt.size.x.max(0.0) }, if le.preferred.y >= 0.0 { le.preferred.y } else { rt.size.y.max(0.0) });
            Some(Item { entity: c, preferred: pref, flexible: le.flexible })
        })
        .collect()
}

/// The size the group wants for these children (Content Size Fitter).
pub(crate) fn preferred_size(g: &UiLayoutGroup, items: &[Item], width_hint: f32) -> Vec2 {
    let [l, r, t, b] = g.padding;
    let n = items.len() as f32;
    match g.kind {
        LayoutKind::Vertical => {
            let h: f32 = items.iter().map(|i| i.preferred.y).sum::<f32>() + g.spacing.y * (n - 1.0).max(0.0);
            let w = items.iter().map(|i| i.preferred.x).fold(0.0, f32::max);
            Vec2::new(w + l + r, h + t + b)
        }
        LayoutKind::Horizontal => {
            let w: f32 = items.iter().map(|i| i.preferred.x).sum::<f32>() + g.spacing.x * (n - 1.0).max(0.0);
            let h = items.iter().map(|i| i.preferred.y).fold(0.0, f32::max);
            Vec2::new(w + l + r, h + t + b)
        }
        LayoutKind::Grid => {
            let cols = grid_columns(g, width_hint, items.len());
            let rows = (items.len() as f32 / cols as f32).ceil();
            Vec2::new(cols as f32 * (g.cell_size.x + g.spacing.x) - g.spacing.x + l + r, rows * (g.cell_size.y + g.spacing.y) - g.spacing.y.min(rows * 1e9) + t + b)
        }
    }
}

fn grid_columns(g: &UiLayoutGroup, width: f32, n: usize) -> usize {
    if g.columns > 0 {
        return g.columns as usize;
    }
    let inner = width - g.padding[0] - g.padding[1];
    (((inner + g.spacing.x) / (g.cell_size.x + g.spacing.x).max(1.0)).floor() as usize).clamp(1, n.max(1))
}

/// Child rects (min, max) for a group whose rect is (a, b), y up.
pub(crate) fn arrange(g: &UiLayoutGroup, items: &[Item], a: Vec2, b: Vec2) -> Vec<(Entity, Vec2, Vec2)> {
    let [l, r, t, bt] = g.padding;
    let inner_min = Vec2::new(a.x + l, a.y + bt);
    let inner_max = Vec2::new(b.x - r, b.y - t);
    let inner = (inner_max - inner_min).max(Vec2::ZERO);
    let n = items.len();
    let mut out = Vec::with_capacity(n);
    if n == 0 {
        return out;
    }
    match g.kind {
        LayoutKind::Grid => {
            let cols = grid_columns(g, b.x - a.x, n);
            let rows = n.div_ceil(cols);
            let used = Vec2::new(cols as f32 * (g.cell_size.x + g.spacing.x) - g.spacing.x, rows as f32 * (g.cell_size.y + g.spacing.y) - g.spacing.y);
            let start = Vec2::new(inner_min.x + (inner.x - used.x).max(0.0) * g.child_alignment.x, inner_max.y - (inner.y - used.y).max(0.0) * (1.0 - g.child_alignment.y));
            for (k, it) in items.iter().enumerate() {
                let (cx, cy) = ((k % cols) as f32, (k / cols) as f32);
                let min_x = start.x + cx * (g.cell_size.x + g.spacing.x);
                let max_y = start.y - cy * (g.cell_size.y + g.spacing.y);
                out.push((it.entity, Vec2::new(min_x, max_y - g.cell_size.y), Vec2::new(min_x + g.cell_size.x, max_y)));
            }
        }
        LayoutKind::Vertical | LayoutKind::Horizontal => {
            let vertical = g.kind == LayoutKind::Vertical;
            let (main_inner, cross_inner) = if vertical { (inner.y, inner.x) } else { (inner.x, inner.y) };
            let spacing = if vertical { g.spacing.y } else { g.spacing.x };
            let main_pref = |i: &Item| if vertical { i.preferred.y } else { i.preferred.x };
            let cross_pref = |i: &Item| if vertical { i.preferred.x } else { i.preferred.y };
            let flex = |i: &Item| if vertical { i.flexible.y } else { i.flexible.x };
            let (control_main, control_cross) = if vertical { (g.control_height, g.control_width) } else { (g.control_width, g.control_height) };
            let expand_main = if vertical { g.expand_height } else { g.expand_width };
            let expand_cross = if vertical { g.expand_width } else { g.expand_height };
            let total: f32 = items.iter().map(main_pref).sum::<f32>() + spacing * (n as f32 - 1.0);
            let leftover = (main_inner - total).max(0.0);
            let flex_sum: f32 = items.iter().map(flex).sum();
            let grow = expand_main && control_main && flex_sum > 0.0;
            let used = if grow { main_inner.max(total) } else { total };
            // start along the main axis (alignment applies to what's left)
            let align_main = if vertical { 1.0 - g.child_alignment.y } else { g.child_alignment.x };
            let mut cursor = (main_inner - used).max(0.0) * align_main;
            for it in items {
                let mut size_main = main_pref(it);
                if grow {
                    size_main += leftover * flex(it) / flex_sum;
                }
                let size_cross = if control_cross && expand_cross { cross_inner } else { cross_pref(it).min(if control_cross { cross_inner } else { f32::MAX }) };
                let align_cross = if vertical { g.child_alignment.x } else { 1.0 - g.child_alignment.y };
                let cross_off = (cross_inner - size_cross).max(0.0) * align_cross;
                let (min, max) = if vertical {
                    let top = inner_max.y - cursor;
                    let x0 = inner_min.x + cross_off;
                    (Vec2::new(x0, top - size_main), Vec2::new(x0 + size_cross, top))
                } else {
                    let x0 = inner_min.x + cursor;
                    let top = inner_max.y - cross_off;
                    (Vec2::new(x0, top - size_cross), Vec2::new(x0 + size_main, top))
                };
                out.push((it.entity, min, max));
                cursor += size_main + spacing;
            }
        }
    }
    out
}

/// Clips an axis-aligned quad (world space, corners bottom-left / bottom-right / top-right /
/// top-left) to [min, max] in x / y. Returns false when nothing is left.
pub(crate) fn clip_quad(q: &mut Quad, min: Vec2, max: Vec2) -> bool {
    let (x0, y0) = (q.corners[0].x, q.corners[0].y);
    let (x1, y1) = (q.corners[2].x, q.corners[2].y);
    if x1 <= min.x || x0 >= max.x || y1 <= min.y || y0 >= max.y || x1 <= x0 || y1 <= y0 {
        return false;
    }
    let (nx0, nx1, ny0, ny1) = (x0.max(min.x), x1.min(max.x), y0.max(min.y), y1.min(max.y));
    if (nx0, nx1, ny0, ny1) == (x0, x1, y0, y1) {
        return true;
    }
    // uv interpolated along the quad's edges (uv[0] = bottom-left ... uv[3] = top-left)
    let fx = |x: f32| (x - x0) / (x1 - x0);
    let fy = |y: f32| (y - y0) / (y1 - y0);
    let uv = q.uv;
    let at = |u: f32, v: f32| -> [f32; 2] {
        let bottom = [uv[0][0] + (uv[1][0] - uv[0][0]) * u, uv[0][1] + (uv[1][1] - uv[0][1]) * u];
        let top = [uv[3][0] + (uv[2][0] - uv[3][0]) * u, uv[3][1] + (uv[2][1] - uv[3][1]) * u];
        [bottom[0] + (top[0] - bottom[0]) * v, bottom[1] + (top[1] - bottom[1]) * v]
    };
    let z = q.corners[0].z;
    q.corners = [Vec3::new(nx0, ny0, z), Vec3::new(nx1, ny0, z), Vec3::new(nx1, ny1, z), Vec3::new(nx0, ny1, z)];
    q.uv = [at(fx(nx0), fy(ny0)), at(fx(nx1), fy(ny0)), at(fx(nx1), fy(ny1)), at(fx(nx0), fy(ny1))];
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(e: u32, w: f32, h: f32) -> Item {
        Item { entity: Entity::from_bits((1u64 << 32) | e as u64).unwrap(), preferred: Vec2::new(w, h), flexible: Vec2::ONE }
    }

    #[test]
    fn vertical_group_stacks_from_the_top() {
        let g = UiLayoutGroup { padding: [5.0, 5.0, 10.0, 0.0], spacing: Vec2::new(0.0, 4.0), ..Default::default() };
        let items = [it(1, 50.0, 20.0), it(2, 60.0, 30.0)];
        let r = arrange(&g, &items, Vec2::ZERO, Vec2::new(100.0, 200.0));
        assert_eq!((r[0].1, r[0].2), (Vec2::new(5.0, 170.0), Vec2::new(55.0, 190.0)));
        assert_eq!((r[1].1, r[1].2), (Vec2::new(5.0, 136.0), Vec2::new(65.0, 166.0)));
        assert_eq!(preferred_size(&g, &items, 100.0), Vec2::new(70.0, 64.0));
        // controlled + expanded width: children fill the inner width
        let g = UiLayoutGroup { control_width: true, expand_width: true, ..g };
        let r = arrange(&g, &items, Vec2::ZERO, Vec2::new(100.0, 200.0));
        assert_eq!(r[1].2.x - r[1].1.x, 90.0);
    }

    #[test]
    fn horizontal_expand_and_grid() {
        let g = UiLayoutGroup { kind: LayoutKind::Horizontal, spacing: Vec2::new(10.0, 0.0), control_width: true, expand_width: true, ..Default::default() };
        let r = arrange(&g, &[it(1, 20.0, 10.0), it(2, 20.0, 10.0)], Vec2::ZERO, Vec2::new(110.0, 10.0));
        // 60 px leftover shared: 50 each
        assert_eq!(r[0].2.x - r[0].1.x, 50.0);
        assert_eq!(r[1].1.x, 60.0);
        let g = UiLayoutGroup { kind: LayoutKind::Grid, cell_size: Vec2::new(30.0, 30.0), spacing: Vec2::splat(5.0), ..Default::default() };
        let items: Vec<Item> = (0..5).map(|k| it(k, 0.0, 0.0)).collect();
        let r = arrange(&g, &items, Vec2::ZERO, Vec2::new(100.0, 100.0));
        // 3 columns fit in 100 px; the 4th item starts row 2
        assert_eq!(r[3].1, Vec2::new(0.0, 100.0 - 65.0));
        assert_eq!(r[2].1.x, 70.0);
    }

    #[test]
    fn clipping_cuts_corners_and_uvs() {
        let mut q = Quad {
            sorting_order: 0,
            layer: 0,
            texture: { use goat_gfx::GraphicsBackend; goat_gfx::NullBackend::new().create_texture(&goat_gfx::TextureData::solid(goat_core::Color::WHITE)).unwrap() },
            blend: goat_gfx::BlendMode::Alpha,
            z: 0.0,
            corners: [Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(0.0, 10.0, 0.0)],
            normal: Vec3::Z,
            uv: [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
            color: [1.0; 4],
        };
        assert!(clip_quad(&mut q, Vec2::new(5.0, -5.0), Vec2::new(20.0, 5.0)));
        assert_eq!(q.corners[0], Vec3::new(5.0, 0.0, 0.0));
        assert_eq!(q.corners[2], Vec3::new(10.0, 5.0, 0.0));
        assert_eq!(q.uv[0], [0.5, 1.0]);
        assert_eq!(q.uv[2], [1.0, 0.5]);
        assert!(!clip_quad(&mut q, Vec2::new(50.0, 50.0), Vec2::new(60.0, 60.0)));
    }
}
