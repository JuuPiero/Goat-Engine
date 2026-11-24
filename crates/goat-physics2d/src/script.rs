//! 2D physics calls for scripts (feature `script`); GameObjects come back as GameObjects.
//!
//! - `Physics2D.raycast(origin, direction, maxDistance?, layerMask?)` → `{ gameObject, point, normal, distance }` | null
//! - `Physics2D.overlapPoint(point)` → GameObject | null, `Physics2D.overlapCircle(center, radius, layerMask?)` → GameObjects
//! - `Physics2D.collisions()` (on an object) → this frame's `{ other, kind }`
//! - `Physics2D.setLayerCollision(a, b, collide)`, `Physics2D.gravity([x, y]?)`
//! - `Rigidbody2D.addImpulse([x, y])`, `Rigidbody2D.addForce([x, y])`

use goat_core::math::Vec2;
use goat_runtime::Entity;
use goat_script::{entity_json, Api};
use serde_json::{json, Value};

use crate::{CollisionKind2D, Physics2D};

fn vec2(v: Option<&Value>) -> Vec2 {
    match v {
        Some(Value::Object(o)) => Vec2::new(o.get("x").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32, o.get("y").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32),
        _ => {
            let a = |i: usize| v.and_then(|v| v.get(i)).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
            Vec2::new(a(0), a(1))
        }
    }
}
fn mask(v: Option<&Value>) -> u32 {
    v.and_then(|x| x.as_f64()).map(|x| x as i64 as u32).unwrap_or(u32::MAX)
}
fn phys<'a>(api: &'a mut Api) -> Result<&'a mut Physics2D, String> {
    api.ctx.resources.get_mut::<Physics2D>().ok_or_else(|| "no Physics2DPlugin".to_string())
}

fn raycast(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let (o, d) = (vec2(a.first()), vec2(a.get(1)));
    let dist = a.get(2).and_then(|x| x.as_f64()).unwrap_or(1e6) as f32;
    let hit = phys(api)?.raycast_layers(o, d, dist, mask(a.get(3)));
    Ok(hit.map(|h| json!({ "gameObject": entity_json(h.entity), "point": [h.point.x, h.point.y], "normal": [h.normal.x, h.normal.y], "distance": h.distance })).unwrap_or(Value::Null))
}
fn overlap_point(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let p = vec2(a.first());
    Ok(phys(api)?.overlap_point(p).map(entity_json).unwrap_or(Value::Null))
}
fn overlap_circle(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let c = vec2(a.first());
    let r = a.get(1).and_then(|x| x.as_f64()).unwrap_or(1.0) as f32;
    Ok(Value::Array(phys(api)?.overlap_circle(c, r, mask(a.get(2))).into_iter().map(entity_json).collect()))
}
fn collisions(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    let list: Vec<Value> = phys(api)?
        .events
        .iter()
        .filter_map(|ev| {
            let other = ev.other(e)?;
            let kind = match ev.kind {
                CollisionKind2D::Enter => "enter",
                CollisionKind2D::Exit => "exit",
                CollisionKind2D::TriggerEnter => "triggerEnter",
                CollisionKind2D::TriggerExit => "triggerExit",
            };
            Some(json!({ "other": entity_json(other), "kind": kind }))
        })
        .collect();
    Ok(Value::Array(list))
}
fn set_layer_collision(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let l = |i: usize| a.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0) as u8;
    let on = a.get(2).and_then(|x| x.as_bool()).unwrap_or(true);
    phys(api)?.set_layer_collision(l(0), l(1), on);
    Ok(Value::Null)
}
fn gravity(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let p = phys(api)?;
    if a.first().is_some() {
        p.gravity = vec2(a.first());
    }
    Ok(json!([p.gravity.x, p.gravity.y]))
}
fn add_impulse(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let i = vec2(a.first());
    phys(api)?.apply_impulse(e, i);
    Ok(Value::Null)
}
fn add_force(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let f = vec2(a.first()) * api.ctx.time.fixed_delta;
    phys(api)?.apply_impulse(e, f);
    Ok(Value::Null)
}

pub fn register(x: &mut goat_script::Extensions) {
    x.add("Physics2D.raycast", raycast)
        .add("Physics2D.overlapPoint", overlap_point)
        .add("Physics2D.overlapCircle", overlap_circle)
        .add("Physics2D.collisions", collisions)
        .add("Physics2D.setLayerCollision", set_layer_collision)
        .add("Physics2D.gravity", gravity)
        .add("Rigidbody2D.addImpulse", add_impulse)
        .add("Rigidbody2D.addForce", add_force);
}
