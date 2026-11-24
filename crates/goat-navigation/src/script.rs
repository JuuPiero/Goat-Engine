//! NavMeshAgent / NavMesh calls for scripts (feature `script`):
//! `NavMeshAgent.setDestination(p)`, `.state()` (one call: remaining distance, has path, pending,
//! velocity…), `.setStopped(b)`, `.resetPath()`, `.warp(p)`, `.move(offset)`, `.set(field, value)`;
//! `NavMesh.samplePosition(p, maxDistance)`, `NavMesh.calculatePath(a, b)`, `NavMesh.raycast(a, b)`.

use goat_core::math::Vec3;
use goat_runtime::Entity;
use goat_script::Api;
use serde_json::{json, Value};

use crate::{NavMeshAgent, NavMeshHit, NavMeshes, PathStatus};

fn vec3(v: Option<&Value>) -> Vec3 {
    let a = |i: usize| v.and_then(|v| v.get(i)).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
    match v {
        Some(Value::Object(o)) => {
            let f = |k: &str| o.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
            Vec3::new(f("x"), f("y"), f("z"))
        }
        _ => Vec3::new(a(0), a(1), a(2)),
    }
}
fn arr(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}
fn with<R>(api: &mut Api, e: Entity, f: impl FnOnce(&mut NavMeshAgent) -> R) -> Result<R, String> {
    let mut a = api.ctx.world.get::<&mut NavMeshAgent>(e).map_err(|_| "no NavMeshAgent on this object".to_string())?;
    Ok(f(&mut a))
}
fn status(s: Option<PathStatus>) -> &'static str {
    match s {
        Some(PathStatus::Complete) => "PathComplete",
        Some(PathStatus::Partial) => "PathPartial",
        _ => "PathInvalid",
    }
}
fn hit(h: Option<NavMeshHit>) -> Value {
    match h {
        Some(h) => json!({ "hit": h.hit, "position": arr(h.position), "normal": arr(h.normal), "distance": h.distance, "mask": h.mask }),
        None => json!({ "hit": false }),
    }
}

fn set_destination(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let p = vec3(a.first());
    with(api, e, |ag| json!(ag.set_destination(p)))
}
fn state(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with(api, e, |ag| {
        json!({
            "remainingDistance": ag.remaining_distance(), "hasPath": ag.has_path(), "pathPending": ag.path_pending(),
            "isOnNavMesh": ag.is_on_nav_mesh(), "pathStatus": status(ag.path_status()), "isStopped": ag.is_stopped,
            "velocity": arr(ag.velocity), "desiredVelocity": arr(ag.desired_velocity), "nextPosition": arr(ag.next_position()),
            "steeringTarget": arr(ag.steering_target()), "destination": arr(ag.destination()), "stoppingDistance": ag.stopping_distance,
            "corners": ag.path().iter().map(|c| arr(*c)).collect::<Vec<_>>(),
        })
    })
}
fn set_stopped(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let v = a.first().and_then(|v| v.as_bool()).unwrap_or(true);
    with(api, e, |ag| ag.is_stopped = v).map(|_| Value::Null)
}
fn reset_path(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    with(api, e, |ag| ag.reset_path()).map(|_| Value::Null)
}
fn warp(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let p = vec3(a.first());
    with(api, e, |ag| ag.warp(p)).map(|_| json!(true))
}
fn move_by(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let p = vec3(a.first());
    with(api, e, |ag| ag.move_by(p)).map(|_| Value::Null)
}
/// Sets a setting by its scene name (speed, angular_speed, acceleration, stopping_distance…).
fn set(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let key = a.first().and_then(|v| v.as_str()).unwrap_or("").to_string();
    let v = a.get(1).cloned().unwrap_or(Value::Null);
    with(api, e, |ag| {
        let mut j = serde_json::to_value(&*ag).map_err(|e| e.to_string())?;
        j[key.as_str()] = v;
        let next: NavMeshAgent = serde_json::from_value(j).map_err(|e| e.to_string())?;
        let keep = (ag.is_stopped, ag.velocity, ag.desired_velocity, std::mem::take(&mut ag.state));
        *ag = next;
        (ag.is_stopped, ag.velocity, ag.desired_velocity, ag.state) = keep;
        Ok::<_, String>(Value::Null)
    })?
}

fn navs<R>(api: &mut Api, f: impl FnOnce(&NavMeshes) -> R) -> Result<R, String> {
    api.ctx.resources.get::<NavMeshes>().map(f).ok_or_else(|| "no NavigationPlugin".to_string())
}
fn sample_position(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let (p, d) = (vec3(a.first()), a.get(1).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32);
    let mask = a.get(2).and_then(|v| v.as_u64()).map_or(u32::MAX, |m| m as u32);
    navs(api, |n| hit(n.sample_position(p, d, mask, 0)))
}
fn calculate_path(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let (p, q) = (vec3(a.first()), vec3(a.get(1)));
    let mask = a.get(2).and_then(|v| v.as_u64()).map_or(u32::MAX, |m| m as u32);
    navs(api, |n| match n.calculate_path(p, q, mask, 0) {
        Some(path) => json!({ "status": status(Some(path.status)), "corners": path.corners.iter().map(|c| arr(*c)).collect::<Vec<_>>() }),
        None => json!({ "status": "PathInvalid", "corners": [] }),
    })
}
fn raycast(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let (p, q) = (vec3(a.first()), vec3(a.get(1)));
    navs(api, |n| hit(Some(n.raycast(p, q, u32::MAX, 0))))
}

pub fn register(x: &mut goat_script::Extensions) {
    x.add("NavMeshAgent.setDestination", set_destination)
        .add("NavMeshAgent.state", state)
        .add("NavMeshAgent.setStopped", set_stopped)
        .add("NavMeshAgent.resetPath", reset_path)
        .add("NavMeshAgent.warp", warp)
        .add("NavMeshAgent.move", move_by)
        .add("NavMeshAgent.set", set)
        .add("NavMesh.samplePosition", sample_position)
        .add("NavMesh.calculatePath", calculate_path)
        .add("NavMesh.raycast", raycast);
}
