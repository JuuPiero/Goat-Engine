//! UI calls for scripts (feature `script`): what scene JSON can't say (one-frame events) and
//! quick access to values. goat.js wraps them: `UI.onClick(go, fn)`, `UI.value(go)`...
//!
//! - `UI.clicked` / `UI.changed` / `UI.submitted`: this frame's events of the element
//! - `UI.value`: Toggle `isOn`, Slider `value`, InputField `text`, Dropdown index, Text `text`
//! - `UI.setValue`, `UI.setText` (UiText), `UI.select` (navigation focus), `UI.selected`

use goat_runtime::Entity;
use goat_script::Api;
use serde_json::{json, Value};

use crate::{UiButton, UiDropdown, UiInputField, UiLayout, UiScrollRect, UiSelection, UiSlider, UiText, UiToggle};

fn clicked(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    let w = &api.ctx.world;
    let b = w.get::<&UiButton>(e).is_ok_and(|b| b.clicked) || api.ctx.resources.get::<UiLayout>().is_some_and(|l| l.clicked.contains(&e));
    Ok(json!(b))
}
fn changed(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    let w = &api.ctx.world;
    Ok(json!(w.get::<&UiToggle>(e).is_ok_and(|t| t.changed) || w.get::<&UiSlider>(e).is_ok_and(|s| s.changed) || w.get::<&UiInputField>(e).is_ok_and(|f| f.changed) || w.get::<&UiDropdown>(e).is_ok_and(|d| d.changed)))
}
fn submitted(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    Ok(json!(api.ctx.world.get::<&UiInputField>(e).is_ok_and(|f| f.submitted)))
}
fn value(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    let w = &api.ctx.world;
    if let Ok(t) = w.get::<&UiToggle>(e) {
        return Ok(json!(t.is_on));
    }
    if let Ok(s) = w.get::<&UiSlider>(e) {
        return Ok(json!(s.value));
    }
    if let Ok(f) = w.get::<&UiInputField>(e) {
        return Ok(json!(f.text));
    }
    if let Ok(d) = w.get::<&UiDropdown>(e) {
        return Ok(json!(d.value));
    }
    if let Ok(s) = w.get::<&UiScrollRect>(e) {
        return Ok(json!([s.normalized.x, s.normalized.y]));
    }
    if let Ok(t) = w.get::<&UiText>(e) {
        return Ok(json!(t.text));
    }
    Ok(Value::Null)
}
fn set_value(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let v = a.first().cloned().unwrap_or(Value::Null);
    let w = &mut api.ctx.world;
    if let Ok(mut t) = w.get::<&mut UiToggle>(e) {
        t.is_on = v.as_bool().unwrap_or(false);
    } else if let Ok(mut s) = w.get::<&mut UiSlider>(e) {
        s.value = (v.as_f64().unwrap_or(0.0) as f32).clamp(0.0, 1.0);
    } else if let Ok(mut f) = w.get::<&mut UiInputField>(e) {
        f.text = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
    } else if let Ok(mut d) = w.get::<&mut UiDropdown>(e) {
        d.value = (v.as_f64().unwrap_or(0.0).max(0.0) as usize).min(d.options.len().saturating_sub(1));
    } else if let Ok(mut s) = w.get::<&mut UiScrollRect>(e) {
        let x = v.get(0).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
        let y = v.get(1).and_then(|x| x.as_f64()).unwrap_or(1.0) as f32;
        s.scroll_to(goat_core::math::Vec2::new(x, y));
    } else if let Ok(mut t) = w.get::<&mut UiText>(e) {
        t.text = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
    } else {
        return Err("no UI control with a value on this object".into());
    }
    Ok(Value::Null)
}
fn set_text(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let s = match a.first() {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    };
    let mut t = api.ctx.world.get::<&mut UiText>(e).map_err(|_| "no UiText on this object".to_string())?;
    t.text = s;
    Ok(Value::Null)
}
fn set_options(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let list: Vec<String> = a.first().and_then(|v| v.as_array()).map(|l| l.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect()).unwrap_or_default();
    let mut d = api.ctx.world.get::<&mut UiDropdown>(e).map_err(|_| "no UiDropdown on this object".to_string())?;
    d.value = d.value.min(list.len().saturating_sub(1));
    d.options = list;
    Ok(Value::Null)
}
fn select(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    let ok = api.ctx.world.contains(e);
    if let Some(s) = api.ctx.resources.get_mut::<UiSelection>() {
        s.selected = ok.then_some(e);
    }
    Ok(Value::Null)
}
fn is_selected(api: &mut Api, e: Entity, _: &[Value]) -> Result<Value, String> {
    Ok(json!(api.ctx.resources.get::<UiSelection>().is_some_and(|s| s.selected == Some(e))))
}
fn set_interactable(api: &mut Api, e: Entity, a: &[Value]) -> Result<Value, String> {
    let on = a.first().and_then(|v| v.as_bool()).unwrap_or(true);
    let w = &mut api.ctx.world;
    if let Ok(mut b) = w.get::<&mut UiButton>(e) {
        b.interactable = on;
    }
    if let Ok(mut f) = w.get::<&mut UiInputField>(e) {
        f.interactable = on;
    }
    if let Ok(mut d) = w.get::<&mut UiDropdown>(e) {
        d.interactable = on;
    }
    Ok(Value::Null)
}

pub fn register(x: &mut goat_script::Extensions) {
    x.add("UI.clicked", clicked)
        .add("UI.changed", changed)
        .add("UI.submitted", submitted)
        .add("UI.value", value)
        .add("UI.setValue", set_value)
        .add("UI.setText", set_text)
        .add("UI.setOptions", set_options)
        .add("UI.select", select)
        .add("UI.isSelected", is_selected)
        .add("UI.setInteractable", set_interactable);
}
