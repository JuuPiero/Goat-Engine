//! More of Unity's UI (uGUI) components: RawImage, CanvasGroup, AspectRatioFitter, ToggleGroup,
//! Scrollbar, RectMask2D, Shadow / Outline effects. The plugin draws / lays them out; the
//! rules that don't need the world live here (tested).

use goat_core::math::Vec2;
use goat_core::Color;
use goat_runtime::Entity;
use serde::{Deserialize, Serialize};

/// A texture drawn as is, with a uv rectangle (Unity's RawImage: render textures, video, tiles).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiRawImage {
    pub texture: Option<String>,
    pub color: Color,
    /// x, y, width, height in uv (0..1; > 1 tiles with a repeating texture).
    pub uv_rect: [f32; 4],
    pub visible: bool,
}

impl Default for UiRawImage {
    fn default() -> Self {
        UiRawImage { texture: None, color: Color::WHITE, uv_rect: [0.0, 0.0, 1.0, 1.0], visible: true }
    }
}

/// Fades / disables a whole UI subtree (Unity's CanvasGroup).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiCanvasGroup {
    /// Multiplies the alpha of everything below.
    pub alpha: f32,
    /// Controls below react to clicks / navigation.
    pub interactable: bool,
    /// The subtree takes hits (off: clicks go through to what's behind).
    pub blocks_raycasts: bool,
    /// Ignore the CanvasGroups above.
    pub ignore_parent_groups: bool,
}

impl Default for UiCanvasGroup {
    fn default() -> Self {
        UiCanvasGroup { alpha: 1.0, interactable: true, blocks_raycasts: true, ignore_parent_groups: false }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AspectMode {
    #[default]
    None,
    WidthControlsHeight,
    HeightControlsWidth,
    FitInParent,
    EnvelopeParent,
}

/// Keeps a rect at an aspect ratio (Unity's AspectRatioFitter).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiAspectRatioFitter {
    pub mode: AspectMode,
    /// Width / height.
    pub aspect_ratio: f32,
}

impl Default for UiAspectRatioFitter {
    fn default() -> Self {
        UiAspectRatioFitter { mode: AspectMode::None, aspect_ratio: 1.0 }
    }
}

impl UiAspectRatioFitter {
    /// The rect after fitting (`a`, `b` = resolved rect; `pa`, `pb` = parent rect), centered.
    pub fn fit(&self, a: Vec2, b: Vec2, pa: Vec2, pb: Vec2) -> (Vec2, Vec2) {
        let r = self.aspect_ratio.max(1e-4);
        let (size, center) = (b - a, (a + b) * 0.5);
        let parent = pb - pa;
        let new = match self.mode {
            AspectMode::None => return (a, b),
            AspectMode::WidthControlsHeight => Vec2::new(size.x, size.x / r),
            AspectMode::HeightControlsWidth => Vec2::new(size.y * r, size.y),
            AspectMode::FitInParent | AspectMode::EnvelopeParent => {
                let by_width = Vec2::new(parent.x, parent.x / r);
                let by_height = Vec2::new(parent.y * r, parent.y);
                let fit = by_width.y <= parent.y;
                let pick = if (self.mode == AspectMode::FitInParent) == fit { by_width } else { by_height };
                return ((pa + pb) * 0.5 - pick * 0.5, (pa + pb) * 0.5 + pick * 0.5);
            }
        };
        (center - new * 0.5, center + new * 0.5)
    }
}

/// Toggles below it (or naming it in `UiToggle.group`) are exclusive (Unity's ToggleGroup).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiToggleGroup {
    /// All toggles may be off (else one stays on).
    pub allow_switch_off: bool,
}

/// After `changed` toggles flipped: the group's rule. `members` = (entity, is_on), `changed` =
/// the one just clicked. Returns the new states.
pub fn resolve_toggle_group(group: &UiToggleGroup, members: &[(Entity, bool)], changed: Entity) -> Vec<(Entity, bool)> {
    let clicked_on = members.iter().find(|(e, _)| *e == changed).is_some_and(|(_, on)| *on);
    members
        .iter()
        .map(|&(e, on)| {
            if e == changed {
                // switching the only one off isn't allowed
                (e, on || !group.allow_switch_off && !clicked_on)
            } else if clicked_on {
                (e, false)
            } else {
                (e, on)
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScrollbarDirection {
    #[default]
    LeftToRight,
    RightToLeft,
    BottomToTop,
    TopToBottom,
}

/// A scrollbar: `value` 0..1, `size` = the handle's share of the track (Unity's Scrollbar). The
/// handle is a child named "Handle"; `scroll_rect` (a name) links it to a ScrollRect.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiScrollbar {
    pub value: f32,
    pub size: f32,
    pub direction: ScrollbarDirection,
    pub interactable: bool,
    /// Name of the ScrollRect it scrolls / follows (empty: none).
    pub scroll_rect: String,
    #[serde(skip)]
    pub changed: bool,
    #[serde(skip)]
    pub handle: Option<Entity>,
}

impl Default for UiScrollbar {
    fn default() -> Self {
        UiScrollbar { value: 0.0, size: 0.2, direction: ScrollbarDirection::LeftToRight, interactable: true, scroll_rect: String::new(), changed: false, handle: None }
    }
}

impl UiScrollbar {
    pub fn vertical(&self) -> bool {
        matches!(self.direction, ScrollbarDirection::BottomToTop | ScrollbarDirection::TopToBottom)
    }
    /// The handle's anchors along the track (min, max), 0..1.
    pub fn handle_anchors(&self) -> (f32, f32) {
        let size = self.size.clamp(0.0, 1.0);
        let v = self.value.clamp(0.0, 1.0);
        let start = v * (1.0 - size);
        match self.direction {
            ScrollbarDirection::LeftToRight | ScrollbarDirection::BottomToTop => (start, start + size),
            ScrollbarDirection::RightToLeft | ScrollbarDirection::TopToBottom => (1.0 - start - size, 1.0 - start),
        }
    }
    /// The value for a pointer at `p` on a track `a`..`b` (the handle centered on the pointer).
    pub fn value_at(&self, p: Vec2, a: Vec2, b: Vec2) -> f32 {
        let (pos, lo, hi) = if self.vertical() { (p.y, a.y, b.y) } else { (p.x, a.x, b.x) };
        let len = (hi - lo).max(1.0);
        let free = (len * (1.0 - self.size.clamp(0.0, 1.0))).max(1.0);
        let t = ((pos - lo - len * self.size.clamp(0.0, 1.0) * 0.5) / free).clamp(0.0, 1.0);
        match self.direction {
            ScrollbarDirection::LeftToRight | ScrollbarDirection::BottomToTop => t,
            _ => 1.0 - t,
        }
    }
}

/// Clips children to the rect without drawing anything (Unity's RectMask2D).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct UiRectMask2D;

/// A darker copy of the graphic behind it (Unity's Shadow effect): images and texts.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiShadow {
    pub color: Color,
    /// Pixels: x right, y down.
    pub distance: [f32; 2],
}

impl Default for UiShadow {
    fn default() -> Self {
        UiShadow { color: Color::rgba(0.0, 0.0, 0.0, 0.5), distance: [1.0, 1.0] }
    }
}

/// Copies of the graphic around it (Unity's Outline effect): images and texts.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiOutline {
    pub color: Color,
    pub distance: [f32; 2],
}

impl Default for UiOutline {
    fn default() -> Self {
        UiOutline { color: Color::rgba(0.0, 0.0, 0.0, 0.5), distance: [1.0, 1.0] }
    }
}

/// Offsets (pixels, y up) of the copies an image draws behind itself for its effects.
pub fn effect_offsets(shadow: Option<UiShadow>, outline: Option<UiOutline>) -> Vec<(Vec2, Color)> {
    let mut out = Vec::new();
    if let Some(o) = outline {
        let (x, y) = (o.distance[0], o.distance[1]);
        for (dx, dy) in [(x, -y), (-x, -y), (x, y), (-x, y)] {
            out.push((Vec2::new(dx, dy), o.color));
        }
    }
    if let Some(s) = shadow {
        out.push((Vec2::new(s.distance[0], -s.distance[1]), s.color));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_fitter_modes() {
        let f = |mode| UiAspectRatioFitter { mode, aspect_ratio: 2.0 };
        let (a, b, pa, pb) = (Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0), Vec2::new(0.0, 0.0), Vec2::new(400.0, 100.0));
        let (x, y) = f(AspectMode::WidthControlsHeight).fit(a, b, pa, pb);
        assert_eq!(y - x, Vec2::new(100.0, 50.0));
        let (x, y) = f(AspectMode::HeightControlsWidth).fit(a, b, pa, pb);
        assert_eq!(y - x, Vec2::new(200.0, 100.0));
        // parent 400 × 100: fitting a 2:1 rect = 200 × 100; enveloping = 400 × 200
        let (x, y) = f(AspectMode::FitInParent).fit(a, b, pa, pb);
        assert_eq!(y - x, Vec2::new(200.0, 100.0));
        let (x, y) = f(AspectMode::EnvelopeParent).fit(a, b, pa, pb);
        assert_eq!(y - x, Vec2::new(400.0, 200.0));
    }

    #[test]
    fn toggle_groups_and_scrollbars() {
        let w = &mut hecs::World::new();
        let (a, b, c) = (w.spawn(()), w.spawn(()), w.spawn(()));
        let g = UiToggleGroup::default();
        // b clicked on: a goes off
        assert_eq!(resolve_toggle_group(&g, &[(a, true), (b, true), (c, false)], b), vec![(a, false), (b, true), (c, false)]);
        // the only one clicked off: stays on unless allowed
        assert_eq!(resolve_toggle_group(&g, &[(a, false), (b, false), (c, false)], a), vec![(a, true), (b, false), (c, false)]);
        let free = UiToggleGroup { allow_switch_off: true };
        assert_eq!(resolve_toggle_group(&free, &[(a, false), (b, false)], a), vec![(a, false), (b, false)]);

        let s = UiScrollbar { value: 0.5, size: 0.2, ..Default::default() };
        assert_eq!(s.handle_anchors(), (0.4, 0.6));
        let t = UiScrollbar { direction: ScrollbarDirection::TopToBottom, value: 0.0, size: 0.25, ..Default::default() };
        assert_eq!(t.handle_anchors(), (0.75, 1.0));
        // the pointer at the track's middle → 0.5; at the start → 0
        assert!((s.value_at(Vec2::new(50.0, 0.0), Vec2::ZERO, Vec2::new(100.0, 10.0)) - 0.5).abs() < 1e-5);
        assert_eq!(s.value_at(Vec2::new(0.0, 0.0), Vec2::ZERO, Vec2::new(100.0, 10.0)), 0.0);
        assert_eq!(effect_offsets(Some(UiShadow::default()), Some(UiOutline::default())).len(), 5);
    }
}
