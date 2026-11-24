//! `Text` in scene files (feature `scene`):
//! `"Text": { "text": "Hello", "font": "fonts/a.ttf", "size": 16, "color": "#ffffff", "align": "center", "valign": "middle", "max_width": 200, "layer": 5 }`.

use goat_assets::material::FileColor;
use goat_core::Color;
use goat_runtime::Entity;
use goat_scene::{ComponentCodec, LoadContext, SaveContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Align, Text, VAlign};

#[derive(Serialize, Deserialize)]
struct TextJson {
    text: String,
    font: String,
    #[serde(default = "one")]
    size: f32,
    #[serde(default = "white")]
    color: FileColor,
    #[serde(default = "left")]
    align: String,
    #[serde(default = "top")]
    valign: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_width: Option<f32>,
    #[serde(default = "one")]
    line_spacing: f32,
    #[serde(default)]
    layer: u8,
    #[serde(default)]
    sorting_order: i32,
    #[serde(default = "yes")]
    visible: bool,
}

fn one() -> f32 {
    1.0
}
fn white() -> FileColor {
    FileColor(Color::WHITE)
}
fn left() -> String {
    "left".into()
}
fn top() -> String {
    "top".into()
}
fn yes() -> bool {
    true
}

fn save(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let t = ctx.world.get::<&Text>(e).ok()?;
    serde_json::to_value(TextJson {
        text: t.text.clone(),
        font: t.font.clone(),
        size: t.size,
        color: FileColor(t.color),
        align: match t.align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
        .into(),
        valign: match t.valign {
            VAlign::Top => "top",
            VAlign::Middle => "middle",
            VAlign::Bottom => "bottom",
        }
        .into(),
        max_width: t.max_width,
        line_spacing: t.line_spacing,
        layer: t.layer,
        sorting_order: t.sorting_order,
        visible: t.visible,
    })
    .ok()
}

fn load(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: TextJson = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    let t = Text {
        text: j.text,
        font: j.font,
        size: j.size,
        color: j.color.0,
        align: match j.align.as_str() {
            "center" => Align::Center,
            "right" => Align::Right,
            _ => Align::Left,
        },
        valign: match j.valign.as_str() {
            "middle" => VAlign::Middle,
            "bottom" => VAlign::Bottom,
            _ => VAlign::Top,
        },
        max_width: j.max_width,
        line_spacing: j.line_spacing,
        layer: j.layer.min(31),
        sorting_order: j.sorting_order,
        visible: j.visible,
    };
    ctx.world.insert_one(e, t).map_err(|_| "entity vanished".to_string())
}

pub const CODEC: ComponentCodec = ComponentCodec { name: "Text", save, load, remove: goat_scene::remove_typed::<Text> };
