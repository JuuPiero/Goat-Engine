//! PlayerPrefs / save slots for scripts (feature `script`): `PlayerPrefs.getInt` ... and
//! `Save.write(slot, json)` / `Save.read(slot)` / `Save.delete` / `Save.exists` / `Save.slots`.

use goat_runtime::Entity;
use goat_script::Api;
use serde_json::{json, Value};

use crate::{save, Pref};

fn key(a: &[Value]) -> Result<String, String> {
    a.first().and_then(|v| v.as_str()).map(str::to_string).ok_or_else(|| "expected a key".to_string())
}

fn get_int(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let k = key(a)?;
    let d = a.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as i64;
    Ok(json!(save(api.ctx).prefs.get_int(&k, d)))
}
fn get_float(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let k = key(a)?;
    let d = a.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0);
    Ok(json!(save(api.ctx).prefs.get_float(&k, d)))
}
fn get_string(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let k = key(a)?;
    let d = a.get(1).and_then(|v| v.as_str()).unwrap_or("").to_string();
    Ok(json!(save(api.ctx).prefs.get_string(&k, &d)))
}
fn set_int(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let k = key(a)?;
    let v = a.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0);
    save(api.ctx).prefs.set(&k, Pref::Int(v as i64));
    Ok(Value::Null)
}
fn set_float(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let k = key(a)?;
    let v = a.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0);
    save(api.ctx).prefs.set(&k, Pref::Float(v));
    Ok(Value::Null)
}
fn set_string(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let k = key(a)?;
    let v = match a.get(1) {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    };
    save(api.ctx).prefs.set(&k, Pref::String(v));
    Ok(Value::Null)
}
fn has_key(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let k = key(a)?;
    Ok(json!(save(api.ctx).prefs.has_key(&k)))
}
fn delete_key(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let k = key(a)?;
    save(api.ctx).prefs.delete_key(&k);
    Ok(Value::Null)
}
fn delete_all(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    save(api.ctx).prefs.delete_all();
    Ok(Value::Null)
}
fn flush(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    save(api.ctx).flush().map_err(|e| e.to_string())?;
    Ok(Value::Null)
}
fn write(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let k = key(a)?;
    let v = a.get(1).cloned().unwrap_or(Value::Null);
    save(api.ctx).write_json(&k, &v).map_err(|e| e.to_string())?;
    Ok(Value::Null)
}
fn read(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let k = key(a)?;
    Ok(save(api.ctx).read_json::<Value>(&k).unwrap_or(Value::Null))
}
fn delete(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let k = key(a)?;
    save(api.ctx).delete(&k);
    Ok(Value::Null)
}
fn exists(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let k = key(a)?;
    Ok(json!(save(api.ctx).exists(&k)))
}
fn slots(api: &mut Api, _: Entity, _: &[Value]) -> Result<Value, String> {
    Ok(json!(save(api.ctx).slots()))
}

pub fn register(x: &mut goat_script::Extensions) {
    x.add("PlayerPrefs.getInt", get_int)
        .add("PlayerPrefs.getFloat", get_float)
        .add("PlayerPrefs.getString", get_string)
        .add("PlayerPrefs.setInt", set_int)
        .add("PlayerPrefs.setFloat", set_float)
        .add("PlayerPrefs.setString", set_string)
        .add("PlayerPrefs.hasKey", has_key)
        .add("PlayerPrefs.deleteKey", delete_key)
        .add("PlayerPrefs.deleteAll", delete_all)
        .add("PlayerPrefs.save", flush)
        .add("Save.write", write)
        .add("Save.read", read)
        .add("Save.delete", delete)
        .add("Save.exists", exists)
        .add("Save.slots", slots);
}
