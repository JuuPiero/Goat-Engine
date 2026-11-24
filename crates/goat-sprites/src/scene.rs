//! `SpriteRenderer` in scene files (feature `scene`):
//!
//! ```json
//! "SpriteRenderer": { "texture": "textures/sheet.png", "rect": [0, 0, 16, 16], "pivot": [0.5, 0.5],
//!                     "pixels_per_unit": 16, "color": "#ffffff", "flip_x": false, "layer": 5, "sorting_order": 1, "blend": "alpha" }
//! ```
//! `rect` (pixels, from the top-left) defaults to the whole texture.

use goat_assets::material::FileColor;
use goat_core::math::Vec2;
use goat_core::Color;
use goat_gfx::BlendMode;
use goat_runtime::Entity;
use goat_scene::{ComponentCodec, LoadContext, SaveContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{PixelRect, Sprite, SpriteRenderer};

#[derive(Serialize, Deserialize)]
struct SpriteJson {
    texture: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rect: Option<[f32; 4]>,
    #[serde(default = "center")]
    pivot: [f32; 2],
    #[serde(default = "hundred")]
    pixels_per_unit: f32,
    #[serde(default = "white")]
    color: FileColor,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    flip_x: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    flip_y: bool,
    #[serde(default = "yes")]
    visible: bool,
    #[serde(default)]
    layer: u8,
    #[serde(default)]
    sorting_order: i32,
    #[serde(default = "alpha")]
    blend: String,
}

fn center() -> [f32; 2] {
    [0.5, 0.5]
}
fn hundred() -> f32 {
    100.0
}
fn white() -> FileColor {
    FileColor(Color::WHITE)
}
fn yes() -> bool {
    true
}
fn alpha() -> String {
    "alpha".into()
}

fn save(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let r = *ctx.world.get::<&SpriteRenderer>(e).ok()?;
    let Some(texture) = ctx.assets.key_of_texture(r.sprite.texture) else {
        goat_core::warn!("scene save: a SpriteRenderer uses a texture made in code (no asset key): skipped");
        return None;
    };
    let s = r.sprite;
    let whole = s.rect == PixelRect { x: 0.0, y: 0.0, width: s.texture_size.0 as f32, height: s.texture_size.1 as f32 };
    serde_json::to_value(SpriteJson {
        texture: texture.to_string(),
        rect: (!whole).then_some([s.rect.x, s.rect.y, s.rect.width, s.rect.height]),
        pivot: s.pivot.into(),
        pixels_per_unit: s.pixels_per_unit,
        color: FileColor(r.color),
        flip_x: r.flip_x,
        flip_y: r.flip_y,
        visible: r.visible,
        layer: r.layer,
        sorting_order: r.sorting_order,
        blend: if r.blend == BlendMode::Additive { "additive" } else { "alpha" }.into(),
    })
    .ok()
}

fn load(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: SpriteJson = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    let texture = ctx.assets.texture(ctx.gfx, &j.texture).map_err(|e| e.to_string())?;
    let (w, h) = ctx.assets.texture_size(texture).unwrap_or((1, 1));
    let mut sprite = Sprite::new(texture, w, h).with_pixels_per_unit(j.pixels_per_unit).with_pivot(Vec2::from(j.pivot));
    if let Some([x, y, rw, rh]) = j.rect {
        sprite.rect = PixelRect { x, y, width: rw, height: rh };
    }
    let r = SpriteRenderer {
        sprite,
        color: j.color.0,
        flip_x: j.flip_x,
        flip_y: j.flip_y,
        visible: j.visible,
        layer: j.layer.min(31),
        sorting_order: j.sorting_order,
        blend: if j.blend == "additive" { BlendMode::Additive } else { BlendMode::Alpha },
    };
    ctx.world.insert_one(e, r).map_err(|_| "entity vanished".to_string())
}

pub const CODEC: ComponentCodec = ComponentCodec { name: "SpriteRenderer", save, load, remove: goat_scene::remove_typed::<SpriteRenderer> };
