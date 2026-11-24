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

/// Unity's collision / trigger callbacks for scripts (onCollisionEnter2D / Stay / Exit,
/// onTriggerEnter2D / Stay / Exit): the events of the last step as messages to both entities,
/// Stay for every pair still touching.
pub(crate) fn send_collision_messages(ctx: &mut goat_runtime::Context, from: usize) {
    use crate::CollisionKind2D as K;
    #[derive(Default)]
    struct Touching(std::collections::HashSet<(Entity, Entity, bool)>);
    let events: Vec<crate::CollisionEvent2D> = match ctx.resources.get::<crate::Physics2D>() {
        Some(p) => p.events.get(from..).map(|s| s.to_vec()).unwrap_or_default(),
        None => return,
    };
    let mut touching = std::mem::take(&mut ctx.resources.get_or_default::<Touching>().0);
    let mut fresh = std::collections::HashSet::new();
    let mut out: Vec<(Entity, String, Value)> = Vec::new();
    let both = |a: Entity, b: Entity, name: &str, out: &mut Vec<(Entity, String, Value)>| {
        out.push((a, name.to_string(), serde_json::json!({ "other": goat_script::entity_json(b) })));
        out.push((b, name.to_string(), serde_json::json!({ "other": goat_script::entity_json(a) })));
    };
    for ev in events {
        let (trigger, enter) = match ev.kind {
            K::Enter => (false, true),
            K::Exit => (false, false),
            K::TriggerEnter => (true, true),
            K::TriggerExit => (true, false),
        };
        let what = if trigger { "Trigger" } else { "Collision" };
        if enter {
            touching.insert((ev.a, ev.b, trigger));
            fresh.insert((ev.a, ev.b, trigger));
            both(ev.a, ev.b, &format!("on{what}Enter2D"), &mut out);
        } else {
            touching.remove(&(ev.a, ev.b, trigger));
            touching.remove(&(ev.b, ev.a, trigger));
            both(ev.a, ev.b, &format!("on{what}Exit2D"), &mut out);
        }
    }
    touching.retain(|(a, b, _)| ctx.world.contains(*a) && ctx.world.contains(*b));
    for (a, b, trigger) in touching.iter().copied() {
        if !fresh.contains(&(a, b, trigger)) {
            both(a, b, if trigger { "onTriggerStay2D" } else { "onCollisionStay2D" }, &mut out);
        }
    }
    ctx.resources.get_or_default::<Touching>().0 = touching;
    for (e, n, v) in out {
        goat_script::send_message(ctx.resources, e, &n, v);
    }
}

/// Pointer callbacks on colliders (OnMouse*; onPointer* with a [`crate::Physics2DRaycaster`] on the camera
/// under the pointer), once per frame.
pub(crate) fn send_pointer_messages(ctx: &mut goat_runtime::Context) {
    let hit_and_events = (|| {
        let p = ctx.input.pointer?;
        let display = goat_runtime::pointer_display(ctx.displays);
        let cam = goat_runtime::camera_at(ctx.world, ctx.displays, display, p)?;
        let raycaster = ctx.world.get::<&crate::Physics2DRaycaster>(cam).ok().map(|r| r.event_mask);
        let ray = goat_runtime::screen_point_to_ray(ctx.world, ctx.displays, cam, p)?;
        let mask = raycaster.unwrap_or(u32::MAX);
        let _ = mask;
        let hit = {
            let at = ray.at_z(0.0).unwrap_or(ray.origin);
            ctx.resources.get::<crate::Physics2D>().and_then(|ph| ph.overlap_point(goat_core::math::Vec2::new(at.x, at.y))).map(|e| goat_script::pointer::PointerHit { entity: e, world: at, distance: (at - ray.origin).length() })
        };
        Some((hit, raycaster.is_some()))
    })();
    let (hit, events) = hit_and_events.unwrap_or((None, false));
    goat_script::pointer::update_pointer(ctx.resources, ctx.input, "2d", hit, events);
}