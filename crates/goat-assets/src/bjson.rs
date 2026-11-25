//! Binary JSON: what builds store instead of `.json` text (scenes, prefabs, materials, clips…).
//! Smaller (every string — keys, component names, asset paths — is written once in a table) and
//! faster to read (no number parsing): [`AssetIo::read_string`](crate::AssetIo::read_string)
//! turns it back into text for any loader, [`AssetIo::read_json`](crate::AssetIo::read_json)
//! gives the value directly.
//!
//! Layout: `GBJ1`, string count (varint), strings (varint length + UTF-8), then the root value:
//! a tag byte, then 0 null · 1 false · 2 true · 3 integer (zigzag varint) · 4 f32 (exact
//! values) · 5 f64 · 6 string (table index) · 7 array (count, values) · 8 object (count, (key
//! index, value)…) · 9 u64 (varint).

use std::collections::HashMap;

use serde_json::{Map, Number, Value};

pub const MAGIC: &[u8; 4] = b"GBJ1";

pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

fn varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn add<'a>(s: &'a str, table: &mut Vec<&'a str>, index: &mut HashMap<&'a str, u32>) {
    if !index.contains_key(s) {
        index.insert(s, table.len() as u32);
        table.push(s);
    }
}

fn collect<'a>(v: &'a Value, table: &mut Vec<&'a str>, index: &mut HashMap<&'a str, u32>) {
    match v {
        Value::String(s) => add(s, table, index),
        Value::Array(a) => a.iter().for_each(|x| collect(x, table, index)),
        Value::Object(o) => {
            for (k, x) in o {
                add(k, table, index);
                collect(x, table, index);
            }
        }
        _ => {}
    }
}

pub fn encode(v: &Value) -> Vec<u8> {
    let mut table = Vec::new();
    let mut index = HashMap::new();
    collect(v, &mut table, &mut index);
    let mut out = MAGIC.to_vec();
    varint(&mut out, table.len() as u64);
    for s in &table {
        varint(&mut out, s.len() as u64);
        out.extend_from_slice(s.as_bytes());
    }
    fn write(v: &Value, out: &mut Vec<u8>, index: &HashMap<&str, u32>) {
        match v {
            Value::Null => out.push(0),
            Value::Bool(false) => out.push(1),
            Value::Bool(true) => out.push(2),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    out.push(3);
                    varint(out, ((i << 1) ^ (i >> 63)) as u64);
                } else if let Some(u) = n.as_u64() {
                    out.push(9);
                    varint(out, u);
                } else {
                    let f = n.as_f64().unwrap_or(0.0);
                    if (f as f32) as f64 == f {
                        out.push(4);
                        out.extend_from_slice(&(f as f32).to_le_bytes());
                    } else {
                        out.push(5);
                        out.extend_from_slice(&f.to_le_bytes());
                    }
                }
            }
            Value::String(s) => {
                out.push(6);
                varint(out, index[s.as_str()] as u64);
            }
            Value::Array(a) => {
                out.push(7);
                varint(out, a.len() as u64);
                for x in a {
                    write(x, out, index);
                }
            }
            Value::Object(o) => {
                out.push(8);
                varint(out, o.len() as u64);
                for (k, x) in o {
                    varint(out, index[k.as_str()] as u64);
                    write(x, out, index);
                }
            }
        }
    }
    write(v, &mut out, &index);
    out
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
    table: Vec<&'a str>,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> Result<u8, String> {
        let v = *self.b.get(self.at).ok_or("binary JSON truncated")?;
        self.at += 1;
        Ok(v)
    }
    fn varint(&mut self) -> Result<u64, String> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("bad varint".into())
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self.b.get(self.at..self.at + n).ok_or("binary JSON truncated")?;
        self.at += n;
        Ok(s)
    }
    fn string(&mut self) -> Result<&'a str, String> {
        let i = self.varint()? as usize;
        self.table.get(i).copied().ok_or_else(|| "bad string index".into())
    }
    fn value(&mut self, depth: u32) -> Result<Value, String> {
        if depth > 512 {
            return Err("binary JSON too deep".into());
        }
        Ok(match self.byte()? {
            0 => Value::Null,
            1 => Value::Bool(false),
            2 => Value::Bool(true),
            3 => {
                let z = self.varint()?;
                Value::Number(Number::from(((z >> 1) as i64) ^ -((z & 1) as i64)))
            }
            9 => Value::Number(Number::from(self.varint()?)),
            4 => {
                let f = f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()) as f64;
                Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
            }
            5 => {
                let f = f64::from_le_bytes(self.bytes(8)?.try_into().unwrap());
                Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
            }
            6 => Value::String(self.string()?.to_string()),
            7 => {
                let n = self.varint()? as usize;
                let mut a = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    a.push(self.value(depth + 1)?);
                }
                Value::Array(a)
            }
            8 => {
                let n = self.varint()? as usize;
                let mut m = Map::new();
                for _ in 0..n {
                    let k = self.string()?.to_string();
                    let v = self.value(depth + 1)?;
                    m.insert(k, v);
                }
                Value::Object(m)
            }
            t => return Err(format!("bad binary JSON tag {t}")),
        })
    }
}

pub fn decode(bytes: &[u8]) -> Result<Value, String> {
    if !is_binary(bytes) {
        return Err("not binary JSON".into());
    }
    let mut r = Reader { b: bytes, at: MAGIC.len(), table: Vec::new() };
    let n = r.varint()? as usize;
    let mut table = Vec::with_capacity(n.min(1 << 16));
    for _ in 0..n {
        let len = r.varint()? as usize;
        table.push(std::str::from_utf8(r.bytes(len)?).map_err(|e| e.to_string())?);
    }
    r.table = table;
    r.value(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_scenes_smaller() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets/scenes/showcase.scene.json");
        let text = std::fs::read_to_string(path).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        let bin = encode(&v);
        assert!(bin.len() * 4 < text.len() * 3, "{} vs {} bytes", bin.len(), text.len());
        assert_eq!(decode(&bin).unwrap(), v);
        // every kind of value
        let all = serde_json::json!({ "n": null, "t": true, "f": false, "i": -42, "big": u64::MAX, "x": 0.5, "pi": 3.141592653589793, "s": "é", "a": [1, [2, {}]], "o": { "k": "s" } });
        assert_eq!(decode(&encode(&all)).unwrap(), all);
    }
}
