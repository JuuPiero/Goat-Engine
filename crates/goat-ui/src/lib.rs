//! Screen UI (Unity's UGUI).
//!
//! - [`Canvas`] on a root entity: one per display (3DS top / bottom screen, desktop window),
//!   1 unit = 1 pixel, drawn by its own camera over the scene.
//! - [`RectTransform`] on every UI entity (children = `Parent` chain under the canvas): anchors
//!   (min / max, 0..1 of the parent), pivot, offset and size in pixels, y up from the bottom-left
//!   like Unity.
//! - Elements: [`UiImage`] (color / texture; Simple, 9-sliced or Filled), [`UiText`] (goat-text
//!   fonts), [`UiButton`] (color states, `clicked` for one frame), [`UiToggle`], [`UiSlider`],
//!   [`UiInputField`], [`UiDropdown`], [`UiScrollRect`].
//! - Layout: [`UiLayoutGroup`] (horizontal / vertical / grid), [`UiLayoutElement`],
//!   [`UiContentSizeFitter`]; [`UiMask`] clips descendants (RectMask2D).
//! - Input: the pointer (`Input::pointer`: the 3DS touch screen / the mouse on the second screen)
//!   hits the topmost interactable element of canvases on the secondary display; with a
//!   [`UiEventSystem`] the D-pad / stick moves a selection and A clicks it (any display).
//!   Text boxes take typed text (desktop) or open the system keyboard ([`SoftwareKeyboard`], 3DS).
//! - Drawing: per canvas, in hierarchy order (open dropdown lists last), images and texts in one
//!   quad batcher.

pub mod layout;
#[cfg(feature = "script")]
pub mod script;
pub mod widgets;

use std::collections::HashMap;

use goat_assets::AssetServer;
use goat_core::math::{Mat4, Vec2, Vec3};
use goat_core::Color;
use goat_gfx::{BlendMode, Display, Projection, TextureData, TextureId, Viewport};
use goat_runtime::{layer_mask, Button, Camera, Context, DontSave, Entity, GlobalTransform, Name, Parent, Plugin, Transform, World};
use goat_sprites::{Batcher, Quad};
use goat_text::{Align, Fonts, Text, VAlign};
use serde::{Deserialize, Serialize};

pub use layout::{LayoutKind, UiContentSizeFitter, UiLayoutElement, UiLayoutGroup, UiMask};
pub use goat_runtime::{KeyboardRequest, SoftwareKeyboard};
pub use widgets::{ContentType, UiDropdown, UiEventSystem, UiInputField, UiOverlay, UiScrollRect, UiSelection};
use widgets::DropdownItem;

/// Layer UI canvases draw on (their cameras see only it).
pub const UI_LAYER: u8 = 30;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Canvas {
    pub display: Display,
    /// Canvases of one display draw in increasing order.
    pub sorting_order: i32,
}

impl Default for Canvas {
    fn default() -> Self {
        Canvas::new(Display::Secondary)
    }
}

impl Canvas {
    pub fn new(display: Display) -> Self {
        Canvas { display, sorting_order: 0 }
    }
}

/// Placement of a UI element in its parent's rectangle (pixels, y up).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RectTransform {
    pub anchor_min: Vec2,
    pub anchor_max: Vec2,
    pub pivot: Vec2,
    /// Offset of the pivot from the anchor reference point.
    pub offset: Vec2,
    /// Size added to the anchored span (= the size when both anchors are equal).
    pub size: Vec2,
}

impl Default for RectTransform {
    fn default() -> Self {
        RectTransform::anchored(Vec2::splat(0.5), Vec2::ZERO, Vec2::new(100.0, 30.0))
    }
}

impl RectTransform {
    /// A fixed-size rect anchored at a point of the parent (0..1), e.g. `(0.5, 1.0)` = top center.
    pub fn anchored(anchor: Vec2, offset: Vec2, size: Vec2) -> Self {
        RectTransform { anchor_min: anchor, anchor_max: anchor, pivot: anchor, offset, size }
    }
    /// Fills the parent, inset by `margin` pixels.
    pub fn stretch(margin: f32) -> Self {
        RectTransform { anchor_min: Vec2::ZERO, anchor_max: Vec2::ONE, pivot: Vec2::splat(0.5), offset: Vec2::ZERO, size: Vec2::splat(-2.0 * margin) }
    }
    /// The rect (min, max) inside a parent rect.
    pub fn resolve(&self, parent_min: Vec2, parent_max: Vec2) -> (Vec2, Vec2) {
        let psize = parent_max - parent_min;
        let a0 = parent_min + psize * self.anchor_min;
        let a1 = parent_min + psize * self.anchor_max;
        let size = (a1 - a0) + self.size;
        let reference = a0 + (a1 - a0) * self.pivot;
        let min = reference + self.offset - size * self.pivot;
        (min, min + size)
    }
}

/// How an image fills its rect (Unity's Image Type).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageType {
    #[default]
    Simple,
    /// 9-slice: the `border` keeps its size, the middle stretches (panels, buttons).
    Sliced,
    /// Shows `fill_amount` of the image (health bars, progress).
    Filled,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillMethod {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiImage {
    pub color: Color,
    /// Texture asset key (None = plain color).
    pub texture: Option<String>,
    pub visible: bool,
    pub image_type: ImageType,
    /// Sliced: left, bottom, right, top borders in texture pixels (drawn at the same size).
    pub border: [f32; 4],
    pub fill_method: FillMethod,
    /// Filled: from the right / top instead of the left / bottom.
    pub fill_reverse: bool,
    pub fill_amount: f32,
}

impl Default for UiImage {
    fn default() -> Self {
        UiImage::color(Color::WHITE)
    }
}

impl UiImage {
    pub fn color(color: Color) -> Self {
        UiImage { color, texture: None, visible: true, image_type: ImageType::Simple, border: [0.0; 4], fill_method: FillMethod::Horizontal, fill_reverse: false, fill_amount: 1.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiText {
    pub text: String,
    pub font: String,
    /// Pixels per em.
    pub size: f32,
    pub color: Color,
    pub align: Align,
    pub valign: VAlign,
    pub visible: bool,
    /// Rich text tags (`<color=#hex>`, `<size=1.5>`, `<b>`, `<i>`).
    pub rich: bool,
    /// Outline color + width (fraction of the size).
    pub outline: Option<(Color, f32)>,
    /// Drop shadow color + offset (fractions of the size, x right / y down).
    pub shadow: Option<(Color, [f32; 2])>,
}

impl Default for UiText {
    fn default() -> Self {
        UiText::new("Text", "", 14.0)
    }
}

impl UiText {
    pub fn new(text: impl Into<String>, font: impl Into<String>, size: f32) -> Self {
        UiText { text: text.into(), font: font.into(), size, color: Color::WHITE, align: Align::Center, valign: VAlign::Middle, visible: true, rich: false, outline: None, shadow: None }
    }
}

/// Tints the entity's [`UiImage`] by state; `clicked` is true for the frame of a click.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiButton {
    pub normal: Color,
    pub hover: Color,
    pub pressed: Color,
    pub disabled: Color,
    pub interactable: bool,
    #[serde(skip)]
    pub clicked: bool,
}

impl Default for UiButton {
    fn default() -> Self {
        UiButton {
            normal: Color::hex(0x3a4152),
            hover: Color::hex(0x4a5470),
            pressed: Color::hex(0x2c5aa0),
            disabled: Color::rgba(0.25, 0.27, 0.3, 0.6),
            interactable: true,
            clicked: false,
        }
    }
}

/// A checkbox: a click flips `is_on`; `check` (an image entity) shows when on.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiToggle {
    pub is_on: bool,
    #[serde(skip)]
    pub changed: bool,
    /// The check mark (None: a descendant named "Check").
    #[serde(skip)]
    pub check: Option<Entity>,
}

/// A horizontal slider (0..1): dragging sets `value`; `fill` stretches to it, `handle` follows.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSlider {
    pub value: f32,
    #[serde(skip)]
    pub changed: bool,
    /// None: descendants named "Fill" / "Handle".
    #[serde(skip)]
    pub fill: Option<Entity>,
    #[serde(skip)]
    pub handle: Option<Entity>,
}

/// Rectangles computed by the last layout, in canvas pixels (y up) — hit tests, game code.
#[derive(Default)]
pub struct UiLayout {
    pub rects: HashMap<Entity, (Vec2, Vec2)>,
    /// Visible part (inside every mask above), when clipped.
    pub clips: HashMap<Entity, (Vec2, Vec2)>,
    /// Entities clicked this frame (pointer or navigation).
    pub clicked: Vec<Entity>,
}

impl UiLayout {
    fn contains(&self, e: Entity, p: Vec2) -> bool {
        let inside = |(a, b): (Vec2, Vec2)| p.x >= a.x && p.y >= a.y && p.x <= b.x && p.y <= b.y;
        self.rects.get(&e).is_some_and(|r| inside(*r)) && self.clips.get(&e).is_none_or(|r| inside(*r))
    }
}

struct CanvasState {
    camera: Entity,
    origin: Vec3,
}

struct Drag {
    start: Vec2,
    last: Vec2,
    scroll: Option<Entity>,
    active: bool,
}

pub struct UiPlugin {
    batcher: Batcher,
    canvases: HashMap<Entity, CanvasState>,
    white: Option<TextureId>,
    /// Element under the pointer when it went down.
    pressed: Option<Entity>,
    drag: Option<Drag>,
    /// Drawing order of the last layout (topmost last).
    order: Vec<(Entity, Display)>,
    /// Dropdowns to open at the next layout (they need their canvas and rect).
    to_open: Vec<Entity>,
    nav_hold: f32,
    nav_started: bool,
    /// Laid-out glyph quads per text, reused while nothing about the text changed.
    text_cache: HashMap<Entity, (TextKey, Vec<Quad>)>,
}

/// Everything a text's quads depend on.
#[derive(Clone, PartialEq)]
struct TextKey {
    text: String,
    font: String,
    numbers: [u32; 9],
    align: (u8, u8),
    /// Fonts::generation (an atlas refill moves glyphs).
    atlas: u32,
    /// rich / outline / shadow settings
    effects: String,
}

impl Default for UiPlugin {
    fn default() -> Self {
        UiPlugin { batcher: Batcher::new("UI"), canvases: HashMap::new(), white: None, pressed: None, drag: None, order: Vec::new(), to_open: Vec::new(), nav_hold: 0.0, nav_started: false, text_cache: HashMap::new() }
    }
}

/// The first entity named `name` among the descendants of `root`.
fn descendant_named(world: &World, root: Entity, name: &str) -> Option<Entity> {
    let pairs: Vec<(Entity, Entity)> = world.query::<&Parent>().iter().map(|(c, p)| (p.0, c)).collect();
    let mut stack = vec![root];
    while let Some(x) = stack.pop() {
        for &(p, c) in &pairs {
            if p == x {
                if world.get::<&Name>(c).is_ok_and(|n| n.0 == name) {
                    return Some(c);
                }
                stack.push(c);
            }
        }
    }
    None
}

fn first_child(world: &World, root: Entity) -> Option<Entity> {
    world.query::<&Parent>().with::<&RectTransform>().without::<&goat_runtime::InactiveInHierarchy>().iter().filter(|(_, p)| p.0 == root).map(|(c, _)| c).min_by_key(|c| c.id())
}

fn children_map(world: &World) -> HashMap<Entity, Vec<Entity>> {
    let mut map: HashMap<Entity, Vec<Entity>> = HashMap::new();
    // inactive objects (and so their subtrees) are neither laid out, drawn nor hit
    let mut pairs: Vec<(Entity, Entity)> = world.query::<&Parent>().with::<&RectTransform>().without::<&goat_runtime::InactiveInHierarchy>().iter().map(|(c, p)| (p.0, c)).collect();
    pairs.sort_by_key(|(_, c)| c.id());
    for (p, c) in pairs {
        map.entry(p).or_default().push(c);
    }
    map
}

fn canvas_of(world: &World, mut e: Entity) -> Option<Entity> {
    for _ in 0..64 {
        if world.get::<&Canvas>(e).is_ok() {
            return Some(e);
        }
        e = world.get::<&Parent>(e).ok()?.0;
    }
    None
}

fn is_ancestor(world: &World, ancestor: Entity, mut e: Entity) -> bool {
    for _ in 0..64 {
        if e == ancestor {
            return true;
        }
        match world.get::<&Parent>(e) {
            Ok(p) => e = p.0,
            Err(_) => return false,
        }
    }
    false
}

/// Whether the pointer / navigation can act on this element.
fn interactive(world: &World, e: Entity) -> bool {
    world.get::<&UiButton>(e).is_ok_and(|b| b.interactable)
        || world.get::<&UiToggle>(e).is_ok()
        || world.get::<&UiSlider>(e).is_ok()
        || world.get::<&UiInputField>(e).is_ok_and(|f| f.interactable)
        || world.get::<&UiDropdown>(e).is_ok_and(|d| d.interactable)
}

fn intersect(a: Option<(Vec2, Vec2)>, b: (Vec2, Vec2)) -> (Vec2, Vec2) {
    match a {
        Some((a0, a1)) => (a0.max(b.0), a1.min(b.1)),
        None => b,
    }
}

impl UiPlugin {
    /// A click on `e` (pointer release on it, or A on the selection).
    fn click(&mut self, ctx: &mut Context, e: Entity) {
        if let Some(layout) = ctx.resources.get_mut::<UiLayout>() {
            layout.clicked.push(e);
        }
        if let Ok(mut b) = ctx.world.get::<&mut UiButton>(e) {
            b.clicked = b.interactable;
        }
        if let Ok(mut t) = ctx.world.get::<&mut UiToggle>(e) {
            t.is_on = !t.is_on;
            t.changed = true;
        }
        let item = ctx.world.get::<&DropdownItem>(e).ok().map(|i| *i);
        if let Some(item) = item {
            if let Ok(mut d) = ctx.world.get::<&mut UiDropdown>(item.owner) {
                if d.value != item.index {
                    d.value = item.index;
                    d.changed = true;
                }
            }
            close_dropdown(ctx.world, item.owner);
            if let Some(sel) = ctx.resources.get_mut::<UiSelection>() {
                sel.selected = Some(item.owner);
            }
        }
        let open = ctx.world.get::<&UiDropdown>(e).ok().map(|d| (d.interactable, d.is_open()));
        if let Some((true, was_open)) = open {
            if was_open {
                close_dropdown(ctx.world, e);
            } else {
                self.to_open.push(e);
            }
        }
        let field = ctx.world.get::<&UiInputField>(e).ok().map(|f| (f.interactable, f.text.clone(), f.hint.clone(), f.character_limit, f.multi_line, f.content_type));
        if let Some((true, text, hint, limit, multi, kind)) = field {
            focus_field(ctx.world, Some(e));
            if let Some(kb) = ctx.resources.get_mut::<SoftwareKeyboard>() {
                kb.request = Some(KeyboardRequest {
                    hint,
                    initial: text,
                    max_length: limit,
                    multi_line: multi,
                    numeric: matches!(kind, ContentType::Integer | ContentType::Decimal),
                    password: kind == ContentType::Password,
                });
            }
        }
    }

    /// D-pad / stick navigation (with a [`UiEventSystem`]).
    fn navigate(&mut self, ctx: &mut Context) -> Option<Entity> {
        let es = ctx.world.query::<&UiEventSystem>().iter().next().map(|(_, e)| e.clone())?;
        if !es.navigation {
            return None;
        }
        let input = ctx.input;
        let typing = ctx.world.query::<&UiInputField>().iter().any(|(_, f)| f.focused);
        let layout = ctx.resources.get::<UiLayout>()?;
        let selectables: Vec<(Entity, Vec2)> = self
            .order
            .iter()
            .filter(|(e, _)| interactive(ctx.world, *e))
            .filter_map(|(e, _)| layout.rects.get(e).map(|(a, b)| (*e, (*a + *b) * 0.5)))

            .collect();
        // an open dropdown keeps the selection inside its list
        let open_list: Option<Entity> = ctx.world.query::<&UiDropdown>().iter().find_map(|(_, d)| d.list);
        let selectables: Vec<(Entity, Vec2)> = match open_list {
            Some(l) => selectables.into_iter().filter(|(e, _)| is_ancestor(ctx.world, l, *e)).collect(),
            None => selectables,
        };
        let mut sel = ctx.resources.get::<UiSelection>().and_then(|s| s.selected).filter(|s| selectables.iter().any(|(e, _)| e == s));
        if !self.nav_started {
            self.nav_started = true;
            if !es.first_selected.is_empty() {
                sel = ctx.find(&es.first_selected).filter(|e| selectables.iter().any(|(x, _)| x == e));
            }
        }
        // a direction: pressed, or held (repeats)
        let stick = input.stick();
        let mut dir = Vec2::ZERO;
        let held = |b| input.held(b);
        let want = Vec2::new((held(Button::Right) as i32 - held(Button::Left) as i32) as f32 + if stick.x.abs() > 0.6 { stick.x.signum() } else { 0.0 }, (held(Button::Up) as i32 - held(Button::Down) as i32) as f32 + if stick.y.abs() > 0.6 { stick.y.signum() } else { 0.0 });
        let want = Vec2::new(want.x.clamp(-1.0, 1.0), want.y.clamp(-1.0, 1.0));
        if want != Vec2::ZERO && !typing {
            if self.nav_hold <= 0.0 {
                dir = want;
                self.nav_hold = if self.nav_hold < 0.0 { 0.12 } else { 0.4 };
            } else {
                self.nav_hold -= ctx.time.unscaled_delta;
                if self.nav_hold <= 0.0 {
                    self.nav_hold = -1.0;
                }
            }
        } else {
            self.nav_hold = 0.0;
        }
        if dir != Vec2::ZERO {
            match sel {
                None => {
                    // the top-left element
                    sel = selectables.iter().min_by(|a, b| (a.1.x - a.1.y * 4.0).partial_cmp(&(b.1.x - b.1.y * 4.0)).unwrap()).map(|(e, _)| *e);
                }
                Some(cur) => {
                    let slider = ctx.world.get::<&mut UiSlider>(cur).ok().is_some();
                    if slider && dir.x != 0.0 && dir.y == 0.0 {
                        if let Ok(mut s) = ctx.world.get::<&mut UiSlider>(cur) {
                            s.value = (s.value + dir.x * 0.1).clamp(0.0, 1.0);
                            s.changed = true;
                        }
                    } else {
                        let from = selectables.iter().find(|(e, _)| *e == cur).map(|(_, c)| *c).unwrap_or_default();
                        let d = dir.normalize_or_zero();
                        let best = selectables
                            .iter()
                            .filter(|(e, _)| *e != cur)
                            .filter_map(|(e, c)| {
                                let v = *c - from;
                                let along = v.dot(d);
                                (along > 1.0).then(|| (*e, along + (v - d * along).length() * 2.0))
                            })
                            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                        if let Some((e, _)) = best {
                            sel = Some(e);
                        }
                    }
                }
            }
        }
        if let Some(s) = ctx.resources.get_mut::<UiSelection>() {
            s.selected = sel;
        }
        if typing {
            if input.pressed(Button::B) {
                focus_field(ctx.world, None);
            }
            return None;
        }
        if input.pressed(Button::B) {
            let open = ctx.world.query::<&UiDropdown>().iter().find(|(_, d)| d.list.is_some()).map(|(e, _)| e);
            if let Some(owner) = open {
                close_dropdown(ctx.world, owner);
                if let Some(s) = ctx.resources.get_mut::<UiSelection>() {
                    s.selected = Some(owner);
                }
            }
        }
        if input.pressed(Button::A) {
            return sel;
        }
        None
    }
}

fn focus_field(world: &mut World, e: Option<Entity>) {
    for (x, f) in world.query_mut::<&mut UiInputField>() {
        let on = Some(x) == e;
        if f.focused && !on {
            // leaving a field = end of edit
            f.submitted = true;
        }
        f.focused = on;
        f.blink = 0.0;
    }
}

fn close_dropdown(world: &mut World, owner: Entity) {
    let list = world.get::<&mut UiDropdown>(owner).ok().and_then(|mut d| d.list.take());
    if let Some(l) = list {
        let mut doomed = vec![l];
        let pairs: Vec<(Entity, Entity)> = world.query::<&Parent>().iter().map(|(c, p)| (p.0, c)).collect();
        let mut i = 0;
        while i < doomed.len() {
            let x = doomed[i];
            doomed.extend(pairs.iter().filter(|(p, _)| *p == x).map(|(_, c)| *c));
            i += 1;
        }
        for d in doomed {
            let _ = world.despawn(d);
        }
    }
}

impl Plugin for UiPlugin {
    fn in_edit_mode(&self) -> bool {
        true
    }

    fn build(&mut self, ctx: &mut Context) {
        ctx.resources.get_or_default::<UiLayout>();
        ctx.resources.get_or_default::<UiSelection>();
        #[cfg(feature = "scene")]
        {
            use goat_scene::serde_codec;
            goat_scene::registry(ctx)
                .component(serde_codec::<Canvas>("Canvas"))
                .component(serde_codec::<RectTransform>("RectTransform"))
                .component(serde_codec::<UiImage>("UiImage"))
                .component(serde_codec::<UiText>("UiText"))
                .component(serde_codec::<UiButton>("UiButton"))
                .component(serde_codec::<UiToggle>("UiToggle"))
                .component(serde_codec::<UiSlider>("UiSlider"))
                .component(serde_codec::<UiInputField>("UiInputField"))
                .component(serde_codec::<UiDropdown>("UiDropdown"))
                .component(serde_codec::<UiScrollRect>("UiScrollRect"))
                .component(serde_codec::<UiLayoutGroup>("UiLayoutGroup"))
                .component(serde_codec::<UiLayoutElement>("UiLayoutElement"))
                .component(serde_codec::<UiContentSizeFitter>("UiContentSizeFitter"))
                .component(serde_codec::<UiMask>("UiMask"))
                .component(serde_codec::<UiEventSystem>("UiEventSystem"));
        }
        #[cfg(feature = "script")]
        script::register(goat_script::extensions(ctx));
    }

    /// Pointer / navigation → hover, press, click, drag, scroll, typing.
    fn update(&mut self, ctx: &mut Context) {
        let input = ctx.input;
        let dt = ctx.time.unscaled_delta;
        let height = ctx.display(Display::Secondary).map(|d| d.height as f32).unwrap_or(240.0);
        // one-frame flags
        if let Some(layout) = ctx.resources.get_mut::<UiLayout>() {
            layout.clicked.clear();
        }
        for (_, b) in ctx.world.query_mut::<&mut UiButton>() {
            b.clicked = false;
        }
        for (_, t) in ctx.world.query_mut::<&mut UiToggle>() {
            t.changed = false;
        }
        for (_, s) in ctx.world.query_mut::<&mut UiSlider>() {
            s.changed = false;
        }
        for (_, d) in ctx.world.query_mut::<&mut UiDropdown>() {
            d.changed = false;
        }
        for (_, f) in ctx.world.query_mut::<&mut UiInputField>() {
            f.changed = false;
            f.submitted = false;
            f.blink += dt;
        }

        // pointer in canvas pixels (y up)
        let p = input.pointer.map(|p| Vec2::new(p.x, height - p.y));
        let (hit, scroll_hit) = {
            let Some(layout) = ctx.resources.get::<UiLayout>() else { return };
            let topmost = |f: &dyn Fn(Entity) -> bool| p.and_then(|p| self.order.iter().rev().find(|(e, d)| *d == Display::Secondary && f(*e) && layout.contains(*e, p)).map(|(e, _)| *e));
            (topmost(&|e| interactive(ctx.world, e)), topmost(&|e| ctx.world.get::<&UiScrollRect>(e).is_ok()))
        };
        let down = input.pointer_down;
        if input.pointer_pressed() {
            self.pressed = hit;
            if let Some(p) = p {
                self.drag = Some(Drag { start: p, last: p, scroll: scroll_hit, active: false });
            }
            // a press elsewhere closes lists and leaves text boxes
            let open: Vec<Entity> = ctx.world.query::<&UiDropdown>().iter().filter(|(_, d)| d.list.is_some()).map(|(e, _)| e).collect();
            for o in open {
                let inside = hit.is_some_and(|h| h == o || ctx.world.get::<&DropdownItem>(h).is_ok_and(|i| i.owner == o));
                if !inside {
                    close_dropdown(ctx.world, o);
                }
            }
            if hit.is_none_or(|h| ctx.world.get::<&UiInputField>(h).is_err()) {
                focus_field(ctx.world, None);
            }
            if let (Some(h), Some(sel)) = (hit, ctx.resources.get_mut::<UiSelection>()) {
                sel.selected = Some(h);
            }
        }
        // drags: scroll views (past a few pixels the press stops being a click) and sliders
        if let (true, Some(p)) = (down, p) {
            let mut scrolled = None;
            if let Some(d) = self.drag.as_mut() {
                let slider_drag = self.pressed.is_some_and(|e| ctx.world.get::<&UiSlider>(e).is_ok());
                if let (Some(s), false) = (d.scroll, slider_drag) {
                    if !d.active && (p - d.start).length() > 8.0 {
                        // the content follows from where the finger went down
                        d.active = true;
                        d.last = d.start;
                        self.pressed = None;
                    }
                    if d.active {
                        scrolled = Some((s, p - d.last));
                    }
                }
                d.last = p;
            }
            if let Some((s, delta)) = scrolled {
                scroll_by(ctx.world, s, delta, true, dt);
            }
            if let Some(e) = self.pressed {
                let rect = ctx.resources.get::<UiLayout>().and_then(|l| l.rects.get(&e).copied());
                if let (Ok(mut s), Some((a, b))) = (ctx.world.get::<&mut UiSlider>(e), rect) {
                    let v = ((p.x - a.x) / (b.x - a.x).max(1.0)).clamp(0.0, 1.0);
                    if (v - s.value).abs() > 1e-4 {
                        s.value = v;
                        s.changed = true;
                    }
                }
            }
        }
        let mut clicked = None;
        if input.pointer_released() {
            if self.pressed.is_some() && self.pressed == hit {
                clicked = hit;
            }
            self.pressed = None;
            self.drag = None;
        }
        if let Some(e) = clicked {
            self.click(ctx, e);
        }
        // D-pad / stick
        if let Some(e) = self.navigate(ctx) {
            self.click(ctx, e);
        }
        // scroll views: inertia, elastic edges, stick scrolling, jumps
        let dragging = self.drag.as_ref().filter(|d| d.active).and_then(|d| d.scroll);
        let scrolls: Vec<Entity> = ctx.world.query::<&UiScrollRect>().iter().map(|(e, _)| e).collect();
        let selected = ctx.resources.get::<UiSelection>().and_then(|s| s.selected);
        for s in scrolls {
            if selected == Some(s) {
                let speed = ctx.world.get::<&UiScrollRect>(s).map(|r| r.scroll_speed).unwrap_or(0.0);
                let v = input.axis(goat_runtime::Axis::RightY);
                if v.abs() > 0.2 {
                    scroll_by(ctx.world, s, Vec2::new(0.0, -v * speed * dt), false, dt);
                }
            }
            settle_scroll(ctx, s, dragging == Some(s), dt);
        }
        // typing into the focused text box
        let kb_result = ctx.resources.get_mut::<SoftwareKeyboard>().and_then(|k| k.result.take());
        for (_, f) in ctx.world.query_mut::<&mut UiInputField>() {
            if !f.focused {
                continue;
            }
            match &kb_result {
                Some(Some(text)) => {
                    f.text.clear();
                    f.type_text(text);
                    f.changed = true;
                    f.submitted = true;
                    f.focused = false;
                }
                Some(None) => f.focused = false,
                None => f.type_text(&input.text),
            }
        }
        // button colors
        let selected = ctx.resources.get::<UiSelection>().and_then(|s| s.selected);
        let sel_color = ctx.world.query::<&UiEventSystem>().iter().next().map(|(_, e)| e.selected_color);
        let pressed = self.pressed;
        for (e, (b, img)) in ctx.world.query_mut::<(&UiButton, &mut UiImage)>() {
            img.color = if !b.interactable {
                b.disabled
            } else if pressed == Some(e) && hit == Some(e) {
                b.pressed
            } else if hit == Some(e) {
                b.hover
            } else if selected == Some(e) && sel_color.is_some() {
                sel_color.unwrap()
            } else {
                b.normal
            };
        }
    }

    /// Layout + drawing.
    fn late_update(&mut self, ctx: &mut Context) {
        if self.white.is_none() {
            self.white = ctx.gfx.create_texture(&TextureData::solid(Color::WHITE)).ok();
        }
        let Some(white) = self.white else { return };
        // textures of images given by asset key
        let mut textures: HashMap<Entity, (TextureId, (u32, u32))> = HashMap::new();
        if let Some(server) = ctx.resources.get_mut::<AssetServer>() {
            let keyed: Vec<(Entity, String)> = ctx.world.query::<&UiImage>().iter().filter_map(|(e, i)| i.texture.clone().map(|k| (e, k))).collect();
            for (e, key) in keyed {
                if let Ok(t) = server.texture(ctx.gfx, &key) {
                    textures.insert(e, (t, server.texture_size(t).unwrap_or((1, 1))));
                }
            }
        }
        wire_parts(ctx.world);
        // dropdowns opened by a click
        for owner in std::mem::take(&mut self.to_open) {
            let rect = ctx.resources.get::<UiLayout>().and_then(|l| l.rects.get(&owner).copied());
            let (Some(canvas), Some(rect)) = (canvas_of(ctx.world, owner), rect) else { continue };
            let d = match ctx.world.get::<&UiDropdown>(owner) {
                Ok(d) => (*d).clone(),
                Err(_) => continue,
            };
            let list = widgets::open_dropdown(ctx.world, owner, canvas, rect, &d);
            if let Ok(mut d) = ctx.world.get::<&mut UiDropdown>(owner) {
                d.list = Some(list);
            }
        }

        let children = children_map(ctx.world);
        // content size fitters (deepest first, so nested fitted groups add up)
        let fitted: Vec<Entity> = ctx.world.query::<(&UiContentSizeFitter, &UiLayoutGroup)>().iter().map(|(e, _)| e).collect();
        let depth = |e: Entity| {
            let (mut n, mut x) = (0usize, e);
            while let (Ok(p), true) = (ctx.world.get::<&Parent>(x), n < 64) {
                x = p.0;
                n += 1;
            }
            n
        };
        let mut fitted: Vec<(usize, Entity)> = fitted.into_iter().map(|e| (depth(e), e)).collect();
        fitted.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, e) in fitted {
            let (Ok(f), Ok(g)) = (ctx.world.get::<&UiContentSizeFitter>(e).map(|f| *f), ctx.world.get::<&UiLayoutGroup>(e).map(|g| *g)) else { continue };
            let width = ctx.resources.get::<UiLayout>().and_then(|l| l.rects.get(&e)).map(|(a, b)| b.x - a.x).unwrap_or(0.0);
            let items = layout::items(ctx.world, children.get(&e).map(|v| v.as_slice()).unwrap_or(&[]));
            let pref = layout::preferred_size(&g, &items, width);
            if let Ok(mut rt) = ctx.world.get::<&mut RectTransform>(e) {
                if f.horizontal {
                    rt.size.x = pref.x;
                }
                if f.vertical {
                    rt.size.y = pref.y;
                }
            }
        }
        // driven parts
        drive_parts(ctx.world);

        let canvases: Vec<(Entity, Canvas)> = ctx.world.query::<&Canvas>().without::<&goat_runtime::InactiveInHierarchy>().iter().map(|(e, c)| (e, *c)).collect();
        // cameras for new canvases, forget removed ones
        self.canvases.retain(|e, s| {
            let keep = canvases.iter().any(|(c, _)| c == e);
            if !keep {
                let _ = ctx.world.despawn(s.camera);
            }
            keep
        });
        let mut quads = Vec::new();
        let mut rects = HashMap::new();
        let mut clips = HashMap::new();
        let mut order = Vec::new();
        let mut fonts = ctx.resources.take::<Fonts>().unwrap_or_default();
        let assets = ctx.resources.take::<AssetServer>();
        for (k, (canvas_e, canvas)) in canvases.iter().enumerate() {
            let (w, h) = ctx.display(canvas.display).map(|d| (d.width as f32, d.height as f32)).unwrap_or((400.0, 240.0));
            let state = self.canvases.entry(*canvas_e).or_insert_with(|| {
                let origin = Vec3::new(-6000.0 - 3000.0 * k as f32, 0.0, 0.0);
                let camera = ctx.world.spawn((
                    Name("UI Camera".into()),
                    DontSave,
                    Transform::from_position(origin + Vec3::new(0.0, 0.0, 50.0)),
                    GlobalTransform::default(),
                    Camera { display: canvas.display, viewport: Viewport::FULL, projection: Projection::Orthographic { height: h, near: 0.1, far: 100.0 }, clear: None, clear_depth: true, depth: 1000 + canvas.sorting_order, culling_mask: layer_mask(&[UI_LAYER]) },
                ));
                CanvasState { camera, origin }
            });
            if let Ok(mut c) = ctx.world.get::<&mut Camera>(state.camera) {
                c.display = canvas.display;
                c.projection = Projection::Orthographic { height: h, near: 0.1, far: 100.0 };
            }
            let origin = state.origin;
            let to_world = |p: Vec2, z: f32| origin + Vec3::new(p.x - w * 0.5, p.y - h * 0.5, z);
            // depth-first, parents before children; overlays (open lists) after everything
            type Frame = (Entity, Option<(Vec2, Vec2)>, Vec2, Vec2, Option<(Vec2, Vec2)>);
            let roots = children.get(canvas_e).cloned().unwrap_or_default();
            let (overlays, normal): (Vec<Entity>, Vec<Entity>) = roots.into_iter().partition(|e| ctx.world.get::<&UiOverlay>(*e).is_ok());
            let mut passes: Vec<Vec<Frame>> = vec![
                normal.iter().rev().map(|&e| (e, None, Vec2::ZERO, Vec2::new(w, h), None)).collect(),
                overlays.iter().rev().map(|&e| (e, None, Vec2::ZERO, Vec2::new(w, h), None)).collect(),
            ];
            for stack in passes.iter_mut() {
                while let Some((e, placed, pmin, pmax, clip)) = stack.pop() {
                    let Ok(rt) = ctx.world.get::<&RectTransform>(e).map(|r| *r) else { continue };
                    let (a, b) = placed.unwrap_or_else(|| rt.resolve(pmin, pmax));
                    rects.insert(e, (a, b));
                    if let Some(c) = clip {
                        clips.insert(e, c);
                    }
                    order.push((e, canvas.display));
                    let z = order.len() as f32 * 0.001;
                    let mut own = Vec::new();
                    let mask = ctx.world.get::<&UiMask>(e).ok().map(|m| *m);
                    if let Ok(img) = ctx.world.get::<&UiImage>(e) {
                        if img.visible && img.color.a > 0.0 && mask.is_none_or(|m| m.show_graphic) {
                            let (texture, tsize) = textures.get(&e).copied().unwrap_or((white, (1, 1)));
                            image_quads(&img, a, b, texture, tsize, &mut |qa, qb, uva, uvb| {
                                let c = img.color;
                                own.push(Quad {
                                    sorting_order: canvas.sorting_order,
                                    layer: UI_LAYER,
                                    texture,
                                    blend: BlendMode::Alpha,
                                    z: origin.z + z,
                                    corners: [to_world(qa, z), to_world(Vec2::new(qb.x, qa.y), z), to_world(qb, z), to_world(Vec2::new(qa.x, qb.y), z)],
                                    normal: Vec3::Z,
                                    uv: [[uva.x, uva.y], [uvb.x, uva.y], [uvb.x, uvb.y], [uva.x, uvb.y]],
                                    color: [c.r, c.g, c.b, c.a],
                                });
                            });
                        }
                    }
                    if let (Ok(t), Some(assets)) = (ctx.world.get::<&UiText>(e), assets.as_deref()) {
                        if t.visible && !t.text.is_empty() {
                            let x = match t.align {
                                Align::Left => a.x,
                                Align::Center => (a.x + b.x) * 0.5,
                                Align::Right => b.x,
                            };
                            let y = match t.valign {
                                VAlign::Top => b.y,
                                VAlign::Middle => (a.y + b.y) * 0.5,
                                VAlign::Bottom => a.y,
                            };
                            let pos = to_world(Vec2::new(x, y), z + 0.0005);
                            let c = t.color;
                            let key = TextKey {
                                text: t.text.clone(),
                                font: t.font.clone(),
                                numbers: [t.size, c.r, c.g, c.b, c.a, (b.x - a.x).max(1.0), pos.x, pos.y, pos.z].map(f32::to_bits),
                                align: (t.align as u8, t.valign as u8),
                                atlas: fonts.generation(),
                                effects: format!("{}{:?}{:?}", t.rich, t.outline, t.shadow),
                            };
                            match self.text_cache.get(&e) {
                                Some((k, q)) if *k == key => own.extend(q.iter().cloned()),
                                _ => {
                                    let text = Text { color: t.color, align: t.align, valign: t.valign, max_width: Some((b.x - a.x).max(1.0)), layer: UI_LAYER, sorting_order: canvas.sorting_order, rich: t.rich, outline: t.outline, shadow: t.shadow, ..Text::new(t.text.clone(), t.font.clone(), t.size) };
                                    let m = Mat4::from_translation(pos);
                                    let q: Vec<Quad> = fonts
                                        .quads(assets, ctx.gfx, &text, &m)
                                        .into_iter()
                                        .map(|mut q| {
                                            q.z = origin.z + z + 0.0005;
                                            q
                                        })
                                        .collect();
                                    own.extend(q.iter().cloned());
                                    self.text_cache.insert(e, (key, q));
                                }
                            }
                        }
                    }
                    match clip {
                        Some((c0, c1)) => {
                            let (w0, w1) = (to_world(c0, 0.0), to_world(c1, 0.0));
                            quads.extend(own.into_iter().filter_map(|mut q| layout::clip_quad(&mut q, Vec2::new(w0.x, w0.y), Vec2::new(w1.x, w1.y)).then_some(q)));
                        }
                        None => quads.extend(own),
                    }
                    let child_clip = if mask.is_some() { Some(intersect(clip, (a, b))) } else { clip };
                    if let Some(c) = children.get(&e) {
                        match ctx.world.get::<&UiLayoutGroup>(e).map(|g| *g) {
                            Ok(g) => {
                                let items = layout::items(ctx.world, c);
                                let placed: HashMap<Entity, (Vec2, Vec2)> = layout::arrange(&g, &items, a, b).into_iter().map(|(e, a, b)| (e, (a, b))).collect();
                                stack.extend(c.iter().rev().map(|&ch| (ch, placed.get(&ch).copied(), a, b, child_clip)));
                            }
                            Err(_) => stack.extend(c.iter().rev().map(|&ch| (ch, None, a, b, child_clip))),
                        }
                    }
                }
            }
        }
        fonts.flush(ctx.gfx);
        ctx.resources.put_back(fonts);
        if let Some(a) = assets {
            ctx.resources.put_back(a);
        }
        self.text_cache.retain(|e, _| rects.contains_key(e));
        self.order = order;
        if let Some(layout) = ctx.resources.get_mut::<UiLayout>() {
            layout.rects = rects;
            layout.clips = clips;
        }
        self.batcher.submit(ctx, quads);
    }
}

/// The quads of an image (canvas pixels + uv; uv v goes down).
fn image_quads(img: &UiImage, a: Vec2, b: Vec2, _texture: TextureId, tsize: (u32, u32), out: &mut dyn FnMut(Vec2, Vec2, Vec2, Vec2)) {
    match img.image_type {
        ImageType::Simple => out(a, b, Vec2::new(0.0, 1.0), Vec2::new(1.0, 0.0)),
        ImageType::Filled => {
            let t = img.fill_amount.clamp(0.0, 1.0);
            if t <= 0.0 {
                return;
            }
            let (mut qa, mut qb, mut ua, mut ub) = (a, b, Vec2::new(0.0, 1.0), Vec2::new(1.0, 0.0));
            match (img.fill_method, img.fill_reverse) {
                (FillMethod::Horizontal, false) => {
                    qb.x = a.x + (b.x - a.x) * t;
                    ub.x = t;
                }
                (FillMethod::Horizontal, true) => {
                    qa.x = b.x - (b.x - a.x) * t;
                    ua.x = 1.0 - t;
                }
                (FillMethod::Vertical, false) => {
                    qb.y = a.y + (b.y - a.y) * t;
                    ub.y = 1.0 - t;
                }
                (FillMethod::Vertical, true) => {
                    qa.y = b.y - (b.y - a.y) * t;
                    ua.y = t;
                }
            }
            out(qa, qb, ua, ub);
        }
        ImageType::Sliced => {
            let (tw, th) = (tsize.0.max(1) as f32, tsize.1.max(1) as f32);
            let [l, bo, r, t] = img.border;
            // borders shrink together when the rect is smaller than them
            let sx = ((b.x - a.x) / (l + r).max(1e-3)).min(1.0);
            let sy = ((b.y - a.y) / (bo + t).max(1e-3)).min(1.0);
            let xs = [a.x, a.x + l * sx, b.x - r * sx, b.x];
            let ys = [a.y, a.y + bo * sy, b.y - t * sy, b.y];
            let us = [0.0, l / tw, 1.0 - r / tw, 1.0];
            let vs = [1.0, 1.0 - bo / th, t / th, 0.0];
            for j in 0..3 {
                for i in 0..3 {
                    if xs[i + 1] - xs[i] <= 0.0 || ys[j + 1] - ys[j] <= 0.0 {
                        continue;
                    }
                    out(Vec2::new(xs[i], ys[j]), Vec2::new(xs[i + 1], ys[j + 1]), Vec2::new(us[i], vs[j]), Vec2::new(us[i + 1], vs[j + 1]));
                }
            }
        }
    }
}

/// Parts found by name under controls loaded from scenes.
fn wire_parts(world: &mut World) {
    let unwired: Vec<Entity> = world
        .query::<&UiToggle>()
        .iter()
        .filter(|(_, t)| t.check.is_none())
        .map(|(e, _)| e)
        .chain(world.query::<&UiSlider>().iter().filter(|(_, s)| s.fill.is_none() && s.handle.is_none()).map(|(e, _)| e))
        .collect();
    for e in unwired {
        let find = |name: &str| descendant_named(world, e, name);
        let (check, fill, handle) = (find("Check"), find("Fill"), find("Handle"));
        if let Ok(mut t) = world.get::<&mut UiToggle>(e) {
            t.check = check;
        }
        if let Ok(mut s) = world.get::<&mut UiSlider>(e) {
            s.fill = fill;
            s.handle = handle;
        }
    }
    let scrolls: Vec<Entity> = world.query::<&UiScrollRect>().iter().filter(|(_, s)| s.content.is_none_or(|c| !world.contains(c))).map(|(e, _)| e).collect();
    for e in scrolls {
        let content = descendant_named(world, e, "Content").or_else(|| first_child(world, e));
        if let Ok(mut s) = world.get::<&mut UiScrollRect>(e) {
            s.content = content;
        }
    }
}

/// Toggles / sliders / text boxes / dropdowns drive their parts.
fn drive_parts(world: &mut World) {
    let parts: Vec<(Option<Entity>, bool)> = world.query::<&UiToggle>().iter().map(|(_, t)| (t.check, t.is_on)).collect();
    for (check, on) in parts {
        if let Some(Ok(mut img)) = check.map(|c| world.get::<&mut UiImage>(c)) {
            img.visible = on;
        }
    }
    let sliders: Vec<UiSlider> = world.query::<&UiSlider>().iter().map(|(_, s)| *s).collect();
    for s in sliders {
        if let Some(Ok(mut r)) = s.fill.map(|f| world.get::<&mut RectTransform>(f)) {
            r.anchor_max.x = s.value;
        }
        if let Some(Ok(mut r)) = s.handle.map(|h| world.get::<&mut RectTransform>(h)) {
            r.anchor_min.x = s.value;
            r.anchor_max.x = s.value;
        }
    }
    let fields: Vec<(Entity, String, bool)> = world.query::<&UiInputField>().iter().map(|(e, f)| (e, f.display(f.focused && (f.blink % 1.0) < 0.5), f.text.is_empty() && !f.focused)).collect();
    for (e, shown, empty) in fields {
        if let Some(t) = descendant_named(world, e, "Text") {
            if let Ok(mut t) = world.get::<&mut UiText>(t) {
                t.text = shown;
            }
        }
        if let Some(p) = descendant_named(world, e, "Placeholder") {
            if let Ok(mut t) = world.get::<&mut UiText>(p) {
                t.visible = empty;
            }
        }
    }
    let drops: Vec<(Entity, String)> = world.query::<&UiDropdown>().iter().map(|(e, d)| (e, d.selected().unwrap_or("").to_string())).collect();
    for (e, label) in drops {
        if let Some(l) = descendant_named(world, e, "Label") {
            if let Ok(mut t) = world.get::<&mut UiText>(l) {
                t.text = label;
            }
        }
    }
}

/// Moves a scroll view's content by `delta` pixels (only along enabled axes).
fn scroll_by(world: &mut World, s: Entity, delta: Vec2, from_drag: bool, dt: f32) {
    let Ok(mut sr) = world.get::<&mut UiScrollRect>(s) else { return };
    let mask = Vec2::new(sr.horizontal as u8 as f32, sr.vertical as u8 as f32);
    let d = delta * mask;
    if from_drag && dt > 0.0 {
        sr.velocity = sr.velocity * 0.5 + d / dt * 0.5;
    }
    let Some(content) = sr.content else { return };
    drop(sr);
    if let Ok(mut rt) = world.get::<&mut RectTransform>(content) {
        rt.offset += d;
    }
}

/// Inertia, elastic edges, normalized position, jumps (uses last frame's rects).
fn settle_scroll(ctx: &mut Context, s: Entity, dragging: bool, dt: f32) {
    let Ok(sr) = ctx.world.get::<&UiScrollRect>(s).map(|r| *r) else { return };
    let Some(content) = sr.content else { return };
    let Some(layout) = ctx.resources.get::<UiLayout>() else { return };
    let (Some(&(va, vb)), Some(&(ca, cb))) = (layout.rects.get(&s), layout.rects.get(&content)) else { return };
    let (view, size) = (vb - va, cb - ca);
    let overflow = (size - view).max(Vec2::ZERO);
    // how far the content is out of bounds (x: left-aligned, y: top-aligned)
    let correction = |lo_c: f32, hi_c: f32, lo_v: f32, hi_v: f32, over: f32, top: bool| -> f32 {
        if over <= 0.0 {
            return if top { hi_v - hi_c } else { lo_v - lo_c };
        }
        if hi_c < hi_v {
            hi_v - hi_c
        } else if lo_c > lo_v {
            lo_v - lo_c
        } else {
            0.0
        }
    };
    let corr = Vec2::new(correction(ca.x, cb.x, va.x, vb.x, overflow.x, false), correction(ca.y, cb.y, va.y, vb.y, overflow.y, true));
    let mut shift = Vec2::ZERO;
    let mut velocity = sr.velocity;
    let mut normalized = sr.normalized;
    if let Some(j) = sr.jump {
        // normalized: x 0 = left, y 1 = top
        // y: how far the content moved up past the top; x: how far it moved left
        let (py, px) = (cb.y - vb.y, va.x - ca.x);
        shift.y = (1.0 - j.y) * overflow.y - py;
        shift.x = px - j.x * overflow.x;
        velocity = Vec2::ZERO;
    } else if !dragging {
        for (i, c) in [corr.x, corr.y].into_iter().enumerate() {
            if c != 0.0 {
                let k = (dt / sr.elasticity.max(1e-3)).min(1.0);
                shift[i] = if c.abs() < 0.5 { c } else { c * k };
                velocity[i] = 0.0;
            } else if sr.inertia {
                shift[i] = velocity[i] * dt;
                velocity[i] *= sr.deceleration_rate.clamp(0.0, 1.0).powf(dt);
                if velocity[i].abs() < 1.0 {
                    velocity[i] = 0.0;
                }
            } else {
                velocity[i] = 0.0;
            }
        }
    }
    let mask = Vec2::new(sr.horizontal as u8 as f32, sr.vertical as u8 as f32);
    shift *= mask;
    if overflow.x > 0.0 {
        normalized.x = ((va.x - (ca.x + shift.x)) / overflow.x).clamp(0.0, 1.0);
    }
    if overflow.y > 0.0 {
        normalized.y = 1.0 - (((cb.y + shift.y) - vb.y) / overflow.y).clamp(0.0, 1.0);
    }
    if let Ok(mut rt) = ctx.world.get::<&mut RectTransform>(content) {
        rt.offset += shift;
    }
    if let Ok(mut r) = ctx.world.get::<&mut UiScrollRect>(s) {
        r.velocity = velocity;
        r.normalized = normalized;
        r.jump = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_transform_resolves_like_unity() {
        let parent = (Vec2::ZERO, Vec2::new(320.0, 240.0));
        // 100×40 at the top center, 10 px down
        let r = RectTransform::anchored(Vec2::new(0.5, 1.0), Vec2::new(0.0, -10.0), Vec2::new(100.0, 40.0));
        let (a, b) = r.resolve(parent.0, parent.1);
        assert_eq!((a, b), (Vec2::new(110.0, 190.0), Vec2::new(210.0, 230.0)));
        // stretch with a margin
        let (a, b) = RectTransform::stretch(8.0).resolve(parent.0, parent.1);
        assert_eq!((a, b), (Vec2::new(8.0, 8.0), Vec2::new(312.0, 232.0)));
    }

    fn tex() -> TextureId {
        use goat_gfx::GraphicsBackend;
        goat_gfx::NullBackend::new().create_texture(&TextureData::solid(Color::WHITE)).unwrap()
    }

    #[test]
    fn sliced_and_filled_images() {
        let mut img = UiImage { image_type: ImageType::Sliced, border: [8.0, 8.0, 8.0, 8.0], ..UiImage::default() };
        let mut n = 0;
        let mut corners = Vec::new();
        image_quads(&img, Vec2::ZERO, Vec2::new(100.0, 40.0), tex(), (32, 32), &mut |a, b, ua, _| {
            n += 1;
            corners.push((a, b, ua));
        });
        assert_eq!(n, 9);
        assert_eq!(corners[4].0, Vec2::new(8.0, 8.0), "middle starts after the borders");
        assert_eq!(corners[4].2, Vec2::new(0.25, 0.75));
        img.image_type = ImageType::Filled;
        img.fill_amount = 0.25;
        let mut got = None;
        image_quads(&img, Vec2::ZERO, Vec2::new(100.0, 40.0), tex(), (32, 32), &mut |a, b, _, ub| got = Some((a, b, ub)));
        assert_eq!(got, Some((Vec2::ZERO, Vec2::new(25.0, 40.0), Vec2::new(0.25, 0.0))));
    }
}

#[cfg(test)]
mod interaction_tests;
