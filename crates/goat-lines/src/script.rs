//! TrailRenderer calls for scripts (feature `script`): Unity's `Clear` and the number of points.

use goat_runtime::Entity;
use goat_script::Api;
use serde_json::{json, Value};

use crate::TrailRenderer;

fn clear(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    let mut t = api.ctx.world.get::<&mut TrailRenderer>(e).map_err(|_| "no TrailRenderer on this object".to_string())?;
    t.points.clear();
    Ok(Value::Null)
}
fn position_count(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    let t = api.ctx.world.get::<&TrailRenderer>(e).map_err(|_| "no TrailRenderer on this object".to_string())?;
    Ok(json!(t.points.len()))
}

pub fn register(x: &mut goat_script::Extensions) {
    x.add("TrailRenderer.clear", clear).add("TrailRenderer.positionCount", position_count);
}