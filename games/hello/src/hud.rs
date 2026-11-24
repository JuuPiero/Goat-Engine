//! The bottom-screen HUD: 2D sprites (goat-sprites) drawn by their own orthographic camera on
//! layer 5 over the map — hearts and spinning coins from one procedural sprite sheet.

use goat_runtime::gfx::{Display, Filter, Projection, TextureData, TextureId, Viewport};
use goat_runtime::math::Vec3;
use goat_runtime::*;
use goat_sprites::{Sprite, SpriteRenderer};

/// HUD objects live on this layer; only the HUD camera draws it.
pub const HUD_LAYER: u8 = 5;
/// Where the HUD sits in the world (far from the scene, which the HUD camera never sees).
pub const ORIGIN: Vec3 = Vec3::new(1000.0, 0.0, 0.0);
const SHEET: (u32, u32) = (64, 32);
const CELL: u32 = 16;

/// A 64×32 sprite sheet: row 0 = 4 frames of a spinning coin, row 1 = heart, star.
fn sprite_sheet() -> TextureData {
    let (w, h) = SHEET;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let mut put = |x: u32, y: u32, c: [u8; 4]| {
        let i = ((y * w + x) * 4) as usize;
        rgba[i..i + 4].copy_from_slice(&c);
    };
    // coins: an ellipse whose width shrinks frame by frame (spin)
    for (frame, half_w) in [7.5f32, 5.0, 1.6, 5.0].into_iter().enumerate() {
        for y in 0..CELL {
            for x in 0..CELL {
                let (dx, dy) = ((x as f32 + 0.5 - 8.0) / half_w, (y as f32 + 0.5 - 8.0) / 7.5);
                let d = dx * dx + dy * dy;
                if d <= 1.0 {
                    let c = if d > 0.62 { [178, 110, 20, 255] } else if dx < -0.2 && dy < -0.1 { [255, 244, 170, 255] } else { [250, 196, 50, 255] };
                    put(frame as u32 * CELL + x, y, c);
                }
            }
        }
    }
    // heart (cell 4)
    for y in 0..CELL {
        for x in 0..CELL {
            let (u, v) = ((x as f32 + 0.5 - 8.0) / 7.0, (8.0 - (y as f32 + 0.5)) / 7.0 + 0.15);
            // classic implicit heart: (u² + v² − 1)³ − u² v³ ≤ 0
            let a = u * u + v * v - 1.0;
            if a * a * a - u * u * v * v * v <= 0.0 {
                let c = if u < -0.25 && v > 0.25 { [255, 150, 160, 255] } else { [226, 40, 64, 255] };
                put(x, CELL + y, c);
            }
        }
    }
    // star (cell 5)
    for y in 0..CELL {
        for x in 0..CELL {
            let (px, py) = (x as f32 + 0.5 - 8.0, 8.0 - (y as f32 + 0.5));
            let r = (px * px + py * py).sqrt();
            let a = py.atan2(px) + std::f32::consts::FRAC_PI_2;
            let star = 3.2 + 4.3 * (0.5 + 0.5 * (a * 5.0).cos()).powf(2.0);
            if r <= star {
                put(CELL + x, CELL + y, [120, 220, 255, 255]);
            }
        }
    }
    TextureData { width: w, height: h, rgba, filter: Filter::Nearest, ..TextureData::solid(Color::WHITE) }
}

fn sheet_sprite(tex: TextureId, index: u32) -> Sprite {
    // 1 pixel per unit: the HUD camera shows 240 units = 240 pixels (pixel-exact on the 3DS)
    Sprite::grid(tex, SHEET.0, SHEET.1, CELL, CELL, index).with_pixels_per_unit(1.0)
}

/// Spins a coin through the sheet's 4 frames and bounces it.
struct Coin {
    texture: TextureId,
    base_y: f32,
    phase: f32,
}

impl Behaviour for Coin {
    fn update(&mut self, ctx: &mut Context, me: Entity) {
        let t = if crate::FROZEN { 0.0 } else { ctx.time.elapsed } + self.phase;
        let frame = ((t * 10.0) as u32) % 4;
        if let Ok(mut r) = ctx.world.get::<&mut SpriteRenderer>(me) {
            r.sprite = sheet_sprite(self.texture, frame);
            r.flip_x = frame == 3;
        }
        if let Some(mut tr) = ctx.transform_mut(me) {
            tr.position.y = self.base_y + (t * 3.0).sin().abs() * 18.0;
        }
    }
}

pub fn spawn(ctx: &mut Context) {
    let Ok(tex) = ctx.gfx.create_texture(&sprite_sheet()) else { return };
    let world = &mut *ctx.world;
    // HUD camera: bottom screen, after the map, no clear, sees only the HUD layer
    world.spawn((
        Name("HUD Camera".into()),
        Transform::from_position(ORIGIN + Vec3::new(0.0, 0.0, 10.0)),
        Camera {
            display: Display::Secondary,
            viewport: Viewport::FULL,
            projection: Projection::Orthographic { height: 240.0, near: 0.1, far: 20.0 },
            clear: None,
            clear_depth: true,
            depth: 1,
            culling_mask: layer_mask(&[HUD_LAYER]),
        },
    ));
    for i in 0..3 {
        world.spawn((
            Name(format!("Heart {i}")),
            Transform::from_position(ORIGIN + Vec3::new(-148.0 + i as f32 * 20.0, 104.0, 0.0)),
            SpriteRenderer::new(sheet_sprite(tex, 4)).with_layer(HUD_LAYER),
        ));
    }
    world.spawn((
        Name("Star".into()),
        Transform::from_position(ORIGIN + Vec3::new(148.0, 104.0, 0.0)).with_scale(Vec3::splat(1.5)),
        SpriteRenderer::new(sheet_sprite(tex, 5)).with_layer(HUD_LAYER).with_color(Color::rgba(1.0, 1.0, 1.0, 0.85)),
    ));
    for i in 0..6 {
        let x = -100.0 + i as f32 * 40.0;
        world.spawn((
            Name(format!("Coin {i}")),
            Transform::from_position(ORIGIN + Vec3::new(x, -100.0, 0.0)),
            SpriteRenderer::new(sheet_sprite(tex, 0)).with_layer(HUD_LAYER).with_sorting_order(1),
            Scripts::new().with(Coin { texture: tex, base_y: -100.0, phase: i as f32 * 0.37 }),
        ));
    }
}

pub const FONT: &str = "fonts/BeVietnamPro-Regular.ttf";

/// Shows the frame rate (smoothed).
struct FpsCounter {
    smoothed: f32,
    shown_at: f32,
    logged_at: f32,
}

impl Behaviour for FpsCounter {
    fn update(&mut self, ctx: &mut Context, me: Entity) {
        let dt = ctx.time.delta.max(1e-4);
        self.smoothed = self.smoothed * 0.95 + (1.0 / dt) * 0.05;
        if ctx.time.elapsed - self.logged_at > 5.0 {
            self.logged_at = ctx.time.elapsed;
            if let Some(p) = ctx.resources.get::<goat_runtime::Profiler>() {
                let all: Vec<String> = p.top(12).iter().map(|(n, ms)| format!("{n} {ms:.1}")).collect();
                goat_runtime::info!("profile (ms): {} | {} draws, {} tris", all.join(", "), p.draw_calls, p.triangles);
            }
        }
        if ctx.time.elapsed - self.shown_at > 0.25 {
            self.shown_at = ctx.time.elapsed;
            if let Ok(mut t) = ctx.world.get::<&mut goat_text::Text>(me) {
                // the 4 most expensive parts of the frame (goat_runtime::Profiler), ms
                let top = ctx.resources.get::<goat_runtime::Profiler>().map(|p| p.top(4).iter().map(|(n, ms)| format!("\n{n} {ms:.1}")).collect::<String>()).unwrap_or_default();
                t.text = format!("{:.0} FPS{top}", self.smoothed);
            }
        }
    }
}

pub fn spawn_texts(ctx: &mut Context) {
    use goat_text::{Align, Text, VAlign};
    let world = &mut *ctx.world;
    world.spawn((
        Name("HUD Title".into()),
        Transform::from_position(ORIGIN + Vec3::new(0.0, 14.0, 0.0)),
        Text::new("Xin chào! GoatEngine chạy trên 3DS", FONT, 15.0).with_align(Align::Center, VAlign::Middle).with_layer(HUD_LAYER).with_sorting_order(2).with_color(Color::hex(0xfff2c0)),
    ));
    world.spawn((
        Name("FPS".into()),
        Transform::from_position(ORIGIN + Vec3::new(152.0, 84.0, 0.0)),
        Text::new("-- FPS", FONT, 12.0).with_align(Align::Right, VAlign::Top).with_layer(HUD_LAYER).with_sorting_order(2),
        Scripts::new().with(FpsCounter { smoothed: 60.0, shown_at: 0.0, logged_at: 0.0 }),
    ));
}
