//! The touch-screen panel (goat-ui): drop balls, auto-orbit the camera, ball rain rate.

use goat_runtime::gfx::Display;
use goat_runtime::math::Vec2;
use goat_runtime::*;
use goat_text::{Align, VAlign};
use goat_ui::*;

use crate::hud::FONT;

/// What the panel asks the game for (read by BallSpawner / OrbitCamera).
pub struct Controls {
    /// Balls to drop right now.
    pub drop: u32,
    /// Auto-orbit changed to this (consumed by the camera).
    pub auto_orbit: Option<bool>,
    /// Multiplier of the ball rain (0.25..2).
    pub rain: f32,
}

impl Default for Controls {
    fn default() -> Self {
        Controls { drop: 0, auto_orbit: None, rain: 1.0 }
    }
}

struct PanelActions {
    button: Entity,
    toggle: Entity,
    slider: Entity,
    slider_label: Entity,
}

impl Behaviour for PanelActions {
    fn late_update(&mut self, ctx: &mut Context, _me: Entity) {
        let clicked = ctx.world.get::<&UiButton>(self.button).is_ok_and(|b| b.clicked);
        let toggled = ctx.world.get::<&UiToggle>(self.toggle).ok().filter(|t| t.changed).map(|t| t.is_on);
        let rain = ctx.world.get::<&UiSlider>(self.slider).ok().filter(|s| s.changed).map(|s| s.value);
        let c = ctx.resources.get_or_default::<Controls>();
        if clicked {
            c.drop += 5;
        }
        if let Some(on) = toggled {
            c.auto_orbit = Some(on);
        }
        if let Some(v) = rain {
            c.rain = 0.25 + v * 1.75;
        }
        let rate = c.rain;
        if let Ok(mut t) = ctx.world.get::<&mut UiText>(self.slider_label) {
            t.text = format!("Mưa bóng ×{rate:.1}");
        }
    }
}

fn spawn_ui(world: &mut World, name: &str, parent: Entity, rect: RectTransform) -> Entity {
    world.spawn((Name(name.into()), Transform::default(), GlobalTransform::default(), Parent(parent), rect))
}

pub fn spawn(ctx: &mut Context) {
    ctx.resources.get_or_default::<Controls>();
    let world = &mut *ctx.world;
    let canvas = world.spawn((Name("Touch Canvas".into()), Transform::default(), GlobalTransform::default(), Canvas::new(Display::Secondary)));
    // a panel on the right edge
    let panel = spawn_ui(world, "Panel", canvas, RectTransform::anchored(Vec2::new(1.0, 0.5), Vec2::new(-6.0, -8.0), Vec2::new(132.0, 108.0)));
    let _ = world.insert_one(panel, UiImage::color(Color::rgba(0.08, 0.09, 0.12, 0.82)));

    // button
    let button = spawn_ui(world, "Drop Button", panel, RectTransform::anchored(Vec2::new(0.5, 1.0), Vec2::new(0.0, -6.0), Vec2::new(120.0, 28.0)));
    let _ = world.insert(button, (UiImage::color(Color::hex(0x3a4152)), UiButton::default()));
    let label = spawn_ui(world, "Label", button, RectTransform::stretch(0.0));
    let _ = world.insert_one(label, UiText::new("Thả bóng", FONT, 13.0));

    // toggle: box + check + label
    let toggle = spawn_ui(world, "Auto Orbit", panel, RectTransform::anchored(Vec2::new(0.0, 1.0), Vec2::new(6.0, -40.0), Vec2::new(120.0, 22.0)));
    let _ = world.insert_one(toggle, UiImage::color(Color::rgba(0.0, 0.0, 0.0, 0.0)));
    let bx = spawn_ui(world, "Box", toggle, RectTransform::anchored(Vec2::new(0.0, 0.5), Vec2::new(2.0, 0.0), Vec2::new(16.0, 16.0)));
    let _ = world.insert_one(bx, UiImage::color(Color::hex(0x2a2f3b)));
    let check = spawn_ui(world, "Check", bx, RectTransform::stretch(3.0));
    let _ = world.insert_one(check, UiImage::color(Color::hex(0xf2b53c)));
    let tl = spawn_ui(world, "Label", toggle, RectTransform { anchor_min: Vec2::new(0.0, 0.0), anchor_max: Vec2::new(1.0, 1.0), pivot: Vec2::new(0.0, 0.5), offset: Vec2::new(24.0, 0.0), size: Vec2::new(-24.0, 0.0) });
    let _ = world.insert_one(tl, UiText { align: Align::Left, valign: VAlign::Middle, ..UiText::new("Tự xoay camera", FONT, 11.0) });
    let _ = world.insert_one(toggle, UiToggle { is_on: true, changed: false, check: Some(check) });

    // slider: label, track, fill, handle
    let sl = spawn_ui(world, "Rain Label", panel, RectTransform::anchored(Vec2::new(0.0, 1.0), Vec2::new(8.0, -66.0), Vec2::new(116.0, 14.0)));
    let _ = world.insert_one(sl, UiText { align: Align::Left, valign: VAlign::Middle, color: Color::hex(0xc8cedb), ..UiText::new("Mưa bóng ×1.0", FONT, 10.0) });
    let slider = spawn_ui(world, "Rain Slider", panel, RectTransform::anchored(Vec2::new(0.5, 0.0), Vec2::new(0.0, 12.0), Vec2::new(112.0, 10.0)));
    let _ = world.insert_one(slider, UiImage::color(Color::hex(0x2a2f3b)));
    let fill = spawn_ui(world, "Fill", slider, RectTransform { anchor_min: Vec2::ZERO, anchor_max: Vec2::new(0.43, 1.0), pivot: Vec2::ZERO, offset: Vec2::ZERO, size: Vec2::ZERO });
    let _ = world.insert_one(fill, UiImage::color(Color::hex(0x3d8bff)));
    let handle = spawn_ui(world, "Handle", slider, RectTransform { anchor_min: Vec2::new(0.43, 0.5), anchor_max: Vec2::new(0.43, 0.5), pivot: Vec2::splat(0.5), offset: Vec2::ZERO, size: Vec2::new(10.0, 16.0) });
    let _ = world.insert_one(handle, UiImage::color(Color::hex(0xe8ecf3)));
    let _ = world.insert_one(slider, UiSlider { value: 0.43, changed: false, fill: Some(fill), handle: Some(handle) });

    let _ = world.insert_one(canvas, Scripts::new().with(PanelActions { button, toggle, slider, slider_label: sl }));
}
