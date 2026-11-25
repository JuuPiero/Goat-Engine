//! Script natives for data files and Unity's Resources: TextAsset / JsonAsset / BufferAsset read
//! their file through the asset server; `Resources.find` / `list` search the files under any
//! `Resources/` folder of the project (indexed once per engine).

use base64::Engine as _;
use goat_assets::AssetServer;
use goat_runtime::Entity;
use serde_json::{json, Value};

use crate::{Api, Extensions};

fn key_arg(a: &[Value]) -> Result<String, String> {
    a.first().and_then(Value::as_str).map(str::to_string).ok_or_else(|| "an asset key".to_string())
}

fn io(api: &mut Api) -> Result<goat_assets::AssetIo, String> {
    api.ctx.resources.get::<AssetServer>().map(|s| s.io.clone()).ok_or_else(|| "no asset server".into())
}

/// `[key]` → the file as text (UTF-8, BOM dropped; binary JSON of builds back to text).
fn read_text(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let key = key_arg(a)?;
    io(api)?.read_string(&key).map(Value::String).map_err(|e| e.to_string())
}

/// `[key]` → the file's bytes, base64.
fn read_bytes(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let key = key_arg(a)?;
    let bytes = io(api)?.read(&key).map_err(|e| e.to_string())?;
    Ok(json!(base64::engine::general_purpose::STANDARD.encode(bytes)))
}

fn exists(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let key = key_arg(a)?;
    Ok(Value::Bool(io(api)?.exists(&key)))
}

/// The project's files under a `Resources/` folder, as (key, path inside Resources) pairs.
#[derive(Default)]
struct ResourcesIndex(Option<Vec<(String, String)>>);

fn index(api: &mut Api) -> Result<Vec<(String, String)>, String> {
    if let Some(Some(list)) = api.ctx.resources.get::<ResourcesIndex>().map(|r| r.0.clone()) {
        return Ok(list);
    }
    let all = io(api)?.list_all();
    let list: Vec<(String, String)> = all
        .into_iter()
        .filter_map(|k| {
            let inner = if let Some(rest) = k.strip_prefix("Resources/") {
                rest.to_string()
            } else {
                k.split_once("/Resources/")?.1.to_string()
            };
            Some((k, inner))
        })
        .collect();
    api.ctx.resources.get_or_default::<ResourcesIndex>().0 = Some(list.clone());
    Ok(list)
}

/// `inner` without its extension(s): `Enemies/Goblin.asset.json` → `Enemies/Goblin`.
fn stem(inner: &str) -> &str {
    let (dir, file) = match inner.rfind('/') {
        Some(i) => (&inner[..i + 1], &inner[i + 1..]),
        None => ("", inner),
    };
    let base = file.split('.').next().unwrap_or(file);
    &inner[..dir.len() + base.len()]
}

/// `[path]` → the key of the Resources file at `path` (no extension), or null.
fn find(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let path = key_arg(a)?.trim_matches('/').to_string();
    let list = index(api)?;
    Ok(list.iter().find(|(_, inner)| stem(inner) == path || *inner == path).map(|(k, _)| json!(k)).unwrap_or(Value::Null))
}

/// `[folder]` → keys of every Resources file in `folder` and below ("" = all).
fn list(api: &mut Api, _: Entity, a: &[Value]) -> Result<Value, String> {
    let folder = a.first().and_then(Value::as_str).unwrap_or("").trim_matches('/').to_string();
    let list = index(api)?;
    Ok(Value::Array(
        list.iter()
            .filter(|(_, inner)| folder.is_empty() || inner.starts_with(&format!("{folder}/")) || stem(inner) == folder)
            .map(|(k, _)| json!(k))
            .collect(),
    ))
}

pub fn register(x: &mut Extensions) {
    x.add("Asset.readText", read_text)
        .add("Asset.readBytes", read_bytes)
        .add("Asset.exists", exists)
        .add("Resources.find", find)
        .add("Resources.list", list);
}

#[cfg(test)]
mod tests {
    #[test]
    fn stems() {
        assert_eq!(super::stem("Enemies/Goblin.asset.json"), "Enemies/Goblin");
        assert_eq!(super::stem("hit.wav"), "hit");
        assert_eq!(super::stem("a.b/c.txt"), "a.b/c");
    }
}
