//! Screen UI (Unity's UGUI, the essentials).
//!
//! - [`Canvas`] on a root entity: one per display (3DS top / bottom screen, desktop window),
//!   1 unit = 1 pixel, drawn by its own camera over the scene.
//! - [`RectTransform`] on every UI entity (children = `Parent` chain under the canvas): anchors
//!   (min / max, 0..1 of the parent), pivot, offset and size in pixels, y up from the bottom-left
//!   like Unity.
//! - Elements: [`UiImage`] (color / texture), [`UiText`] (goat-text fonts), [`UiButton`] (color
//!   states, `clicked` for one frame), [`UiToggle`], [`UiSlider`].
//! - Input: the pointer (`Input::pointer`: the 3DS touch screen / the mouse on the second screen)
//!   hits the topmost interactable element of canvases on the secondary display.
//! - Drawing: per canvas, in hierarchy order, images and texts in one quad batcher.

use std::collections::HashMap;

use goat_assets::AssetServer;
use goat_core::math::{Mat4, Vec2, Vec3};
use goat_core::Color;
use goat_gfx::{BlendMode, Display, Projection, TextureData, TextureId, Viewport};
use goat_runtime::{layer_mask, Camera, Context, DontSave, Entity, GlobalTransform, Name, Parent, Plugin, Transform};
use goat_sprites::{Batcher, Quad};
use goat_text::{Align, Fonts, Text, VAlign};

/// Layer UI canvases draw on (their cameras see only it).
pub const UI_LAYER: u8 = 30;

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UiImage {
    pub color: Color,
    /// Texture asset key (None = plain color).
    pub texture: Option<String>,
    pub visible: bool,
}

impl Default for UiImage {
    fn default() -> Self {
        UiImage::color(Color::WHITE)
    }
}

impl UiImage {
    pub fn color(color: Color) -> Self {
        UiImage { color, texture: None, visible: true }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UiText {
    pub text: String,
    pub font: String,
    /// Pixels per em.
    pub size: f32,
    pub color: Color,
    pub align: Align,
    pub valign: VAlign,
}

impl Default for UiText {
    fn default() -> Self {
        UiText::new("Text", "", 14.0)
    }
}

impl UiText {
    pub fn new(text: impl Into<String>, font: impl Into<String>, size: f32) -> Self {
        UiText { text: text.into(), font: font.into(), size, color: Color::WHITE, align: Align::Center, valign: VAlign::Middle }
    }
}

/// Tints the entity's [`UiImage`] by state; `clicked` is true for the frame of a click.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
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
    /// Entities clicked this frame.
    pub clicked: Vec<Entity>,
}

struct CanvasState {
    camera: Entity,
    origin: Vec3,
}

pub struct UiPlugin {
    batcher: Batcher,
    canvases: HashMap<Entity, CanvasState>,
    white: Option<TextureId>,
    /// Element under the pointer when it went down.
    pressed: Option<Entity>,
    /// Drawing order of the last layout (topmost last).
    order: Vec<(Entity, Display)>,
}

impl Default for UiPlugin {
    fn default() -> Self {
        UiPlugin { batcher: Batcher::new("UI"), canvases: HashMap::new(), white: None, pressed: None, order: Vec::new() }
    }
}

/// The first entity named `name` among the descendants of `root`.
fn descendant_named(world: &goat_runtime::World, root: Entity, name: &str) -> Option<Entity> {
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

fn children_map(ctx: &Context) -> HashMap<Entity, Vec<Entity>> {
    let mut map: HashMap<Entity, Vec<Entity>> = HashMap::new();
    let mut pairs: Vec<(Entity, Entity)> = ctx.world.query::<&Parent>().with::<&RectTransform>().iter().map(|(c, p)| (p.0, c)).collect();
    pairs.sort_by_key(|(_, c)| c.id());
    for (p, c) in pairs {
        map.entry(p).or_default().push(c);
    }
    map
}

impl Plugin for UiPlugin {
    fn in_edit_mode(&self) -> bool {
        true
    }

    fn build(&mut self, ctx: &mut Context) {
        ctx.resources.get_or_default::<UiLayout>();
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
                .component(serde_codec::<UiSlider>("UiSlider"));
        }
    }

    /// Pointer → hover / press / click on the secondary display's canvases.
    fn update(&mut self, ctx: &mut Context) {
        let input = ctx.input;
        let height = ctx.display(Display::Secondary).map(|d| d.height as f32).unwrap_or(240.0);
        let Some(layout) = ctx.resources.get_mut::<UiLayout>() else { return };
        layout.clicked.clear();
        // pointer in canvas pixels (y up)
        let p = input.pointer.map(|p| Vec2::new(p.x, height - p.y));
        let hit = p.and_then(|p| {
            self.order.iter().rev().find(|(e, d)| {
                *d == Display::Secondary
                    && (ctx.world.get::<&UiButton>(*e).is_ok_and(|b| b.interactable) || ctx.world.get::<&UiToggle>(*e).is_ok() || ctx.world.get::<&UiSlider>(*e).is_ok())
                    && layout.rects.get(e).is_some_and(|(a, b)| p.x >= a.x && p.y >= a.y && p.x <= b.x && p.y <= b.y)
            })
        });
        let hit = hit.map(|(e, _)| *e);
        let down = input.pointer_down;
        let just_down = input.pointer_pressed();
        let just_up = input.pointer_released();
        if just_down {
            self.pressed = hit;
        }
        // sliders follow the pointer while held
        if let (true, Some(e), Some(p)) = (down, self.pressed, p) {
            if let (Ok(mut s), Some((a, b))) = (ctx.world.get::<&mut UiSlider>(e), layout.rects.get(&e)) {
                let v = ((p.x - a.x) / (b.x - a.x).max(1.0)).clamp(0.0, 1.0);
                if (v - s.value).abs() > 1e-4 {
                    s.value = v;
                    s.changed = true;
                }
            }
        }
        let mut clicked = None;
        if just_up {
            if self.pressed.is_some() && self.pressed == hit {
                clicked = hit;
            }
            self.pressed = None;
        }
        for (e, b) in ctx.world.query_mut::<&mut UiButton>() {
            b.clicked = clicked == Some(e) && b.interactable;
        }
        for (_, t) in ctx.world.query_mut::<&mut UiToggle>() {
            t.changed = false;
        }
        if let Some(e) = clicked {
            layout.clicked.push(e);
            if let Ok(mut t) = ctx.world.get::<&mut UiToggle>(e) {
                t.is_on = !t.is_on;
                t.changed = true;
            }
        }
        // button colors
        for (e, (b, img)) in ctx.world.query_mut::<(&UiButton, &mut UiImage)>() {
            img.color = if !b.interactable {
                b.disabled
            } else if self.pressed == Some(e) && hit == Some(e) {
                b.pressed
            } else if hit == Some(e) {
                b.hover
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
        let mut textures: HashMap<Entity, TextureId> = HashMap::new();
        if let Some(server) = ctx.resources.get_mut::<AssetServer>() {
            let keyed: Vec<(Entity, String)> = ctx.world.query::<&UiImage>().iter().filter_map(|(e, i)| i.texture.clone().map(|k| (e, k))).collect();
            for (e, key) in keyed {
                if let Ok(t) = server.texture(ctx.gfx, &key) {
                    textures.insert(e, t);
                }
            }
        }
        // parts not wired in code (scene-loaded UI): find them by name under the control
        let unwired: Vec<Entity> = ctx.world.query::<&UiToggle>().iter().filter(|(_, t)| t.check.is_none()).map(|(e, _)| e).chain(ctx.world.query::<&UiSlider>().iter().filter(|(_, s)| s.fill.is_none() && s.handle.is_none()).map(|(e, _)| e)).collect();
        for e in unwired {
            let find = |name: &str| descendant_named(ctx.world, e, name);
            let (check, fill, handle) = (find("Check"), find("Fill"), find("Handle"));
            if let Ok(mut t) = ctx.world.get::<&mut UiToggle>(e) {
                t.check = check;
            }
            if let Ok(mut s) = ctx.world.get::<&mut UiSlider>(e) {
                s.fill = fill;
                s.handle = handle;
            }
        }
        // toggles / sliders drive their parts
        let parts: Vec<(Option<Entity>, bool)> = ctx.world.query::<&UiToggle>().iter().map(|(_, t)| (t.check, t.is_on)).collect();
        for (check, on) in parts {
            if let Some(Ok(mut img)) = check.map(|c| ctx.world.get::<&mut UiImage>(c)) {
                img.visible = on;
            }
        }
        let sliders: Vec<(UiSlider,)> = ctx.world.query::<&UiSlider>().iter().map(|(_, s)| (*s,)).collect();
        for (s,) in sliders {
            if let Some(Ok(mut r)) = s.fill.map(|f| ctx.world.get::<&mut RectTransform>(f)) {
                r.anchor_max.x = s.value;
            }
            if let Some(Ok(mut r)) = s.handle.map(|h| ctx.world.get::<&mut RectTransform>(h)) {
                r.anchor_min.x = s.value;
                r.anchor_max.x = s.value;
            }
        }

        let canvases: Vec<(Entity, Canvas)> = ctx.world.query::<&Canvas>().iter().map(|(e, c)| (e, *c)).collect();
        // cameras for new canvases, forget removed ones
        self.canvases.retain(|e, s| {
            let keep = canvases.iter().any(|(c, _)| c == e);
            if !keep {
                let _ = ctx.world.despawn(s.camera);
            }
            keep
        });
        let children = children_map(ctx);
        let mut quads = Vec::new();
        let mut rects = HashMap::new();
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
            // depth-first, parents before children
            let mut stack: Vec<(Entity, Vec2, Vec2)> = children.get(canvas_e).map(|c| c.iter().rev().map(|&e| (e, Vec2::ZERO, Vec2::new(w, h))).collect()).unwrap_or_default();
            while let Some((e, pmin, pmax)) = stack.pop() {
                let Ok(rt) = ctx.world.get::<&RectTransform>(e).map(|r| *r) else { continue };
                let (a, b) = rt.resolve(pmin, pmax);
                rects.insert(e, (a, b));
                order.push((e, canvas.display));
                let z = order.len() as f32 * 0.001;
                if let Ok(img) = ctx.world.get::<&UiImage>(e) {
                    if img.visible && img.color.a > 0.0 {
                        let texture = textures.get(&e).copied().unwrap_or(white);
                        let c = img.color;
                        quads.push(Quad {
                            sorting_order: canvas.sorting_order,
                            layer: UI_LAYER,
                            texture,
                            blend: BlendMode::Alpha,
                            z: origin.z + z,
                            corners: [to_world(a, z), to_world(Vec2::new(b.x, a.y), z), to_world(b, z), to_world(Vec2::new(a.x, b.y), z)],
                            normal: Vec3::Z,
                            uv: [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                            color: [c.r, c.g, c.b, c.a],
                        });
                    }
                }
                if let (Ok(t), Some(assets)) = (ctx.world.get::<&UiText>(e), assets.as_deref()) {
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
                    let text = Text { color: t.color, align: t.align, valign: t.valign, max_width: Some((b.x - a.x).max(1.0)), layer: UI_LAYER, sorting_order: canvas.sorting_order, ..Text::new(t.text.clone(), t.font.clone(), t.size) };
                    let m = Mat4::from_translation(to_world(Vec2::new(x, y), z + 0.0005));
                    quads.extend(fonts.quads(assets, ctx.gfx, &text, &m).into_iter().map(|mut q| {
                        q.z = origin.z + z + 0.0005;
                        q
                    }));
                }
                if let Some(c) = children.get(&e) {
                    stack.extend(c.iter().rev().map(|&ch| (ch, a, b)));
                }
            }
        }
        fonts.flush(ctx.gfx);
        ctx.resources.put_back(fonts);
        if let Some(a) = assets {
            ctx.resources.put_back(a);
        }
        self.order = order;
        if let Some(layout) = ctx.resources.get_mut::<UiLayout>() {
            layout.rects = rects;
        }
        self.batcher.submit(ctx, quads);
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
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use goat_gfx::NullBackend;
    use goat_runtime::{Engine, Game, Scripts};

    struct UiGame;
    impl Game for UiGame {
        fn start(&mut self, ctx: &mut Context) {
            let w = &mut *ctx.world;
            let canvas = w.spawn((Transform::default(), GlobalTransform::default(), Canvas::new(Display::Secondary)));
            // 100×40 button at the center of the 320×240 screen; a slider below it
            w.spawn((Name("Button".into()), Transform::default(), GlobalTransform::default(), Parent(canvas), RectTransform::anchored(Vec2::splat(0.5), Vec2::ZERO, Vec2::new(100.0, 40.0)), UiImage::color(Color::WHITE), UiButton::default()));
            w.spawn((Name("Toggle".into()), Transform::default(), GlobalTransform::default(), Parent(canvas), RectTransform::anchored(Vec2::new(0.5, 0.9), Vec2::ZERO, Vec2::new(40.0, 20.0)), UiToggle { is_on: false, changed: false, check: None }));
            w.spawn((Name("Slider".into()), Transform::default(), GlobalTransform::default(), Parent(canvas), RectTransform::anchored(Vec2::new(0.5, 0.1), Vec2::ZERO, Vec2::new(200.0, 10.0)), UiSlider { value: 0.0, changed: false, fill: None, handle: None }));
            let _ = Scripts::new();
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(UiPlugin::default())]
        }
    }

    fn find(e: &Engine, name: &str) -> Entity {
        e.world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e).unwrap()
    }

    /// Pointer at (x, y) in screen pixels (y down, like the touch screen), down or up, one frame.
    fn touch(e: &mut Engine, gfx: &mut NullBackend, at: Option<(f32, f32)>, down: bool) {
        e.input.begin_frame();
        e.input.pointer = at.map(|(x, y)| Vec2::new(x, y));
        e.input.pointer_down = down;
        e.frame(1.0 / 60.0, gfx);
    }

    #[test]
    fn tap_button_toggle_and_drag_slider() {
        let mut gfx = NullBackend::new();
        let mut e = Engine::new(UiGame);
        e.frame(0.0, &mut gfx);
        e.frame(1.0 / 60.0, &mut gfx); // a layout pass
        let (button, toggle, slider) = (find(&e, "Button"), find(&e, "Toggle"), find(&e, "Slider"));
        // tap the button: clicked for exactly the release frame
        touch(&mut e, &mut gfx, Some((160.0, 120.0)), true);
        assert!(!e.world.get::<&UiButton>(button).unwrap().clicked);
        touch(&mut e, &mut gfx, Some((160.0, 120.0)), false);
        assert!(e.world.get::<&UiButton>(button).unwrap().clicked);
        touch(&mut e, &mut gfx, None, false);
        assert!(!e.world.get::<&UiButton>(button).unwrap().clicked);
        // press, slide off, release: no click
        touch(&mut e, &mut gfx, Some((160.0, 120.0)), true);
        touch(&mut e, &mut gfx, Some((10.0, 10.0)), true);
        touch(&mut e, &mut gfx, Some((10.0, 10.0)), false);
        assert!(!e.world.get::<&UiButton>(button).unwrap().clicked);
        // the toggle (top: y = 240 × (1 − 0.9) = 24 px from the top)
        touch(&mut e, &mut gfx, Some((160.0, 24.0)), true);
        touch(&mut e, &mut gfx, Some((160.0, 24.0)), false);
        assert!(e.world.get::<&UiToggle>(toggle).unwrap().is_on);
        // drag the slider to 3/4 of its width (x from 60 to 260)
        touch(&mut e, &mut gfx, Some((70.0, 216.0)), true);
        touch(&mut e, &mut gfx, Some((210.0, 216.0)), true);
        let v = e.world.get::<&UiSlider>(slider).unwrap().value;
        assert!((v - 0.75).abs() < 0.01, "slider value {v}");
    }
}
