//! 3D physics calls for scripts (feature `script`). GameObjects come back as GameObjects.
//!
//! - `Physics.raycast(origin, direction, maxDistance?, layerMask?)` → `{ gameObject, point, normal, distance }` | null
//! - `Physics.overlapSphere(center, radius, layerMask?)` → GameObjects
//! - `Physics.collisions()` (on an object) → this frame's `{ other, kind }` (enter / exit / triggerEnter / triggerExit)
//! - `Physics.setLayerCollision(a, b, collide)`, `Physics.gravity([x, y, z]?)`
//! - `Rigidbody.addImpulse([x, y, z])`, `Rigidbody.addForce([x, y, z])` (one fixed step)
//! - `CharacterController.move([x, y, z])` → `{ grounded, below, sides, above }`

use goat_core::math::Vec3;
use goat_runtime::Entity;
use goat_script::{entity_json, Api};
use serde_json::{json, Value};

use crate::{CharacterController, CollisionKind, Physics};

fn vec3(v: Option<&Value>) -> Vec3 {
    let a = |i: usize| v.and_then(|v| v.get(i)).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
    match v {
        Some(Value::Object(o)) => Vec3::new(o.get("x").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32, o.get("y").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32, o.get("z").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32),
        _ => Vec3::new(a(0), a(1), a(2)),
    }
}
fn mask(v: Option<&Value>) -> u32 {
    v.and_then(|x| x.as_f64()).map(|x| x as i64 as u32).unwrap_or(u32::MAX)
}
fn phys<'a>(api: &'a mut Api) -> Result<&'a mut Physics, String> {
    api.ctx.resources.get_mut::<Physics>().ok_or_else(|| "no PhysicsPlugin".to_string())
}

fn raycast(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let (o, d) = (vec3(a.first()), vec3(a.get(1)));
    let dist = a.get(2).and_then(|x| x.as_f64()).unwrap_or(1e6) as f32;
    let hit = phys(api)?.raycast_layers(o, d, dist, mask(a.get(3)));
    Ok(hit.map(|h| json!({ "gameObject": entity_json(h.entity), "point": [h.point.x, h.point.y, h.point.z], "normal": [h.normal.x, h.normal.y, h.normal.z], "distance": h.distance })).unwrap_or(Value::Null))
}
fn overlap_sphere(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let c = vec3(a.first());
    let r = a.get(1).and_then(|x| x.as_f64()).unwrap_or(1.0) as f32;
    let list = phys(api)?.overlap_sphere(c, r, mask(a.get(2)));
    Ok(Value::Array(list.into_iter().map(entity_json).collect()))
}
fn collisions(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    let p = phys(api)?;
    let list: Vec<Value> = p
        .events
        .iter()
        .filter_map(|ev| {
            let other = ev.other(e)?;
            let kind = match ev.kind {
                CollisionKind::Enter => "enter",
                CollisionKind::Exit => "exit",
                CollisionKind::TriggerEnter => "triggerEnter",
                CollisionKind::TriggerExit => "triggerExit",
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
        p.gravity = vec3(a.first());
    }
    Ok(json!([p.gravity.x, p.gravity.y, p.gravity.z]))
}
fn add_impulse(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let i = vec3(a.first());
    phys(api)?.apply_impulse(e, i);
    Ok(Value::Null)
}
fn add_force(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    // a force over one fixed step = an impulse of force × step
    let f = vec3(a.first()) * api.ctx.time.fixed_delta;
    phys(api)?.apply_impulse(e, f);
    Ok(Value::Null)
}
fn move_character(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let motion = vec3(a.first());
    let dt = api.ctx.time.delta;
    let mut p = api.ctx.resources.take::<Physics>().ok_or("no PhysicsPlugin")?;
    p.move_character(api.ctx.world, e, motion, dt);
    api.ctx.resources.put_back(p);
    let cc = api.ctx.world.get::<&CharacterController>(e).map(|c| *c).map_err(|_| "no CharacterController on this object".to_string())?;
    let f = cc.collision_flags;
    Ok(json!({ "grounded": cc.is_grounded, "below": f.below, "sides": f.sides, "above": f.above, "velocity": [cc.velocity.x, cc.velocity.y, cc.velocity.z] }))
}
fn is_grounded(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    Ok(json!(api.ctx.world.get::<&CharacterController>(e).is_ok_and(|c| c.is_grounded)))
}

pub fn register(x: &mut goat_script::Extensions) {
    x.add("Physics.raycast", raycast)
        .add("Physics.overlapSphere", overlap_sphere)
        .add("Physics.collisions", collisions)
        .add("Physics.setLayerCollision", set_layer_collision)
        .add("Physics.gravity", gravity)
        .add("Rigidbody.addImpulse", add_impulse)
        .add("Rigidbody.addForce", add_force)
        .add("CharacterController.move", move_character)
        .add("CharacterController.isGrounded", is_grounded);
}

/// Unity's collision / trigger callbacks for scripts (onCollisionEnter / Stay / Exit,
/// onTriggerEnter / Stay / Exit): the events of the last step as messages to both entities,
/// Stay for every pair still touching.
pub(crate) fn send_collision_messages(ctx: &mut goat_runtime::Context, from: usize) {
    use crate::CollisionKind as K;
    #[derive(Default)]
    struct Touching(std::collections::HashSet<(Entity, Entity, bool)>);
    let events: Vec<crate::CollisionEvent> = match ctx.resources.get::<crate::Physics>() {
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
            both(ev.a, ev.b, &format!("on{what}Enter"), &mut out);
        } else {
            touching.remove(&(ev.a, ev.b, trigger));
            touching.remove(&(ev.b, ev.a, trigger));
            both(ev.a, ev.b, &format!("on{what}Exit"), &mut out);
        }
    }
    touching.retain(|(a, b, _)| ctx.world.contains(*a) && ctx.world.contains(*b));
    for (a, b, trigger) in touching.iter().copied() {
        if !fresh.contains(&(a, b, trigger)) {
            both(a, b, if trigger { "onTriggerStay" } else { "onCollisionStay" }, &mut out);
        }
    }
    ctx.resources.get_or_default::<Touching>().0 = touching;
    for (e, n, v) in out {
        goat_script::send_message(ctx.resources, e, &n, v);
    }
}

/// Pointer callbacks on colliders (OnMouse*; onPointer* with a [`crate::PhysicsRaycaster`] on the camera
/// under the pointer), once per frame.
pub(crate) fn send_pointer_messages(ctx: &mut goat_runtime::Context) {
    let hit_and_events = (|| {
        let p = ctx.input.pointer?;
        let display = goat_runtime::pointer_display(ctx.displays);
        let cam = goat_runtime::camera_at(ctx.world, ctx.displays, display, p)?;
        let raycaster = ctx.world.get::<&crate::PhysicsRaycaster>(cam).ok().map(|r| r.event_mask);
        let ray = goat_runtime::screen_point_to_ray(ctx.world, ctx.displays, cam, p)?;
        let mask = raycaster.unwrap_or(u32::MAX);
        let hit = ctx.resources.get::<crate::Physics>().and_then(|ph| ph.raycast_layers(ray.origin, ray.direction, 1000.0, mask)).map(|h| goat_script::pointer::PointerHit { entity: h.entity, world: h.point, distance: h.distance });
        Some((hit, raycaster.is_some()))
    })();
    let (hit, events) = hit_and_events.unwrap_or((None, false));
    goat_script::pointer::update_pointer(ctx.resources, ctx.input, "3d", hit, events);
}