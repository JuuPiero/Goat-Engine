//! [`ModelData`] → GLB (binary glTF 2.0): the format every platform loads (3DS included). Model
//! converters (goat-fbx: FBX / OBJ) produce a `ModelData` and write it here, so a converted model
//! goes through the same loader as a glTF exported by a DCC tool.
//!
//! What is written: meshes (one glTF primitive per submesh = per material), positions / normals /
//! uv 0 / colors 0 / 16-bit indices, joints + weights, materials (the engine's colors back to
//! linear, shininess → roughness, blending, double-sided), base color textures (a file next to
//! the model, embedded bytes, or the decoded pixels as PNG), the node tree, skins and animations.

use goat_core::Color;
use goat_gfx::{BlendMode, CullMode, Filter, Wrap};
use serde_json::{json, Value};

use crate::model::{AnimPath, Interpolation, ModelData};

/// Where a model image comes from when written.
#[derive(Clone, Debug)]
pub enum ImageSource {
    /// A file, relative to the model (kept as a file: builds precompress it like any texture).
    Uri(String),
    /// Bytes stored in the GLB: `image/png`, `image/jpeg`, or `image/ktx` (precompressed ETC1 /
    /// BC for the build's target; the engine's loader reads them as is).
    Embedded { mime: String, bytes: Vec<u8> },
}

struct Bin {
    data: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Bin {
    /// A buffer view over `bytes` (4-byte aligned), returns its index.
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while self.data.len() % 4 != 0 {
            self.data.push(0);
        }
        let offset = self.data.len();
        self.data.extend_from_slice(bytes);
        let mut v = json!({ "buffer": 0, "byteOffset": offset, "byteLength": bytes.len() });
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        self.views.push(v);
        self.views.len() - 1
    }
    fn accessor(&mut self, bytes: &[u8], target: Option<u32>, component: u32, count: usize, kind: &str, minmax: Option<(Vec<f32>, Vec<f32>)>) -> usize {
        let view = self.view(bytes, target);
        let mut a = json!({ "bufferView": view, "componentType": component, "count": count, "type": kind });
        if let Some((mn, mx)) = minmax {
            a["min"] = json!(mn);
            a["max"] = json!(mx);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }
    fn floats(&mut self, values: &[f32], kind: &str, n: usize, target: Option<u32>, bounds: bool) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|f| f.to_le_bytes()).collect();
        let minmax = bounds.then(|| {
            let mut mn = vec![f32::MAX; n];
            let mut mx = vec![f32::MIN; n];
            for c in values.chunks_exact(n) {
                for k in 0..n {
                    mn[k] = mn[k].min(c[k]);
                    mx[k] = mx[k].max(c[k]);
                }
            }
            (mn, mx)
        });
        self.accessor(&bytes, target, 5126, values.len() / n, kind, minmax)
    }
}

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

/// Writes `model` as GLB. `images[i]` says how to store `model.images[i]` (None: its decoded
/// pixels as PNG, or nothing when it has none).
pub fn write_glb(model: &ModelData, images: &[Option<ImageSource>]) -> Vec<u8> {
    let mut bin = Bin { data: Vec::new(), views: Vec::new(), accessors: Vec::new() };

    // ---- meshes: one primitive per submesh
    let mut meshes = Vec::new();
    for prims in &model.meshes {
        let mut primitives = Vec::new();
        for p in prims {
            let vs = &p.mesh.vertices;
            if vs.is_empty() || p.mesh.indices.is_empty() {
                continue;
            }
            let pos: Vec<f32> = vs.iter().flat_map(|v| v.position).collect();
            let nrm: Vec<f32> = vs.iter().flat_map(|v| v.normal).collect();
            let uv: Vec<f32> = vs.iter().flat_map(|v| v.uv).collect();
            let mut attributes = json!({
                "POSITION": bin.floats(&pos, "VEC3", 3, Some(ARRAY_BUFFER), true),
                "NORMAL": bin.floats(&nrm, "VEC3", 3, Some(ARRAY_BUFFER), false),
                "TEXCOORD_0": bin.floats(&uv, "VEC2", 2, Some(ARRAY_BUFFER), false),
            });
            // vertex colors only when they aren't all white (the engine's colors are authored in sRGB: linear in glTF)
            if vs.iter().any(|v| v.color != [1.0; 4]) {
                let col: Vec<f32> = vs.iter().flat_map(|v| Color::rgba(v.color[0], v.color[1], v.color[2], v.color[3]).to_linear().to_array()).collect();
                attributes["COLOR_0"] = json!(bin.floats(&col, "VEC4", 4, Some(ARRAY_BUFFER), false));
            }
            if let Some(sv) = p.skin_vertices.as_ref().filter(|s| s.joints.len() == vs.len()) {
                let j: Vec<u8> = sv.joints.iter().flat_map(|j| j.iter().flat_map(|x| x.to_le_bytes())).collect();
                attributes["JOINTS_0"] = json!(bin.accessor(&j, Some(ARRAY_BUFFER), 5123, vs.len(), "VEC4", None));
                let w: Vec<f32> = sv.weights.iter().flatten().copied().collect();
                attributes["WEIGHTS_0"] = json!(bin.floats(&w, "VEC4", 4, Some(ARRAY_BUFFER), false));
            }
            let idx: Vec<u8> = p.mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
            let indices = bin.accessor(&idx, Some(ELEMENT_ARRAY_BUFFER), 5123, p.mesh.indices.len(), "SCALAR", None);
            let mut prim = json!({ "attributes": attributes, "indices": indices, "mode": 4 });
            if let Some(m) = p.material {
                prim["material"] = json!(m);
            }
            primitives.push(prim);
        }
        meshes.push(json!({ "primitives": primitives }));
    }

    // ---- images / textures / samplers
    let mut json_images = Vec::new();
    let mut image_index = vec![None; model.images.len().max(images.len())];
    for (i, slot) in image_index.iter_mut().enumerate() {
        let entry = match images.get(i).cloned().flatten() {
            Some(ImageSource::Uri(uri)) => Some(json!({ "uri": uri.replace(' ', "%20") })),
            Some(ImageSource::Embedded { mime, bytes }) => Some(json!({ "bufferView": bin.view(&bytes, None), "mimeType": mime })),
            None => model.images.get(i).cloned().flatten().and_then(|t| crate::image::encode_png(t.width, t.height, &t.rgba).ok()).map(|png| json!({ "bufferView": bin.view(&png, None), "mimeType": "image/png" })),
        };
        if let Some(e) = entry {
            json_images.push(e);
            *slot = Some(json_images.len() - 1);
        }
    }
    let mut textures = Vec::new();
    let mut samplers = Vec::new();

    // ---- materials
    let materials: Vec<Value> = model
        .materials
        .iter()
        .map(|m| {
            let d = &m.desc;
            let base = d.base_color.to_linear().to_array();
            let e = d.emissive.to_linear();
            // the importer's shininess = 2 / roughness⁴ − 2
            let roughness = (2.0 / (d.shininess.max(2.0) + 2.0)).powf(0.25).clamp(0.02, 1.0);
            let mut pbr = json!({ "baseColorFactor": base, "metallicFactor": 0.0, "roughnessFactor": roughness });
            if let Some(img) = m.image.and_then(|i| image_index.get(i).copied().flatten()) {
                samplers.push(json!({
                    "magFilter": if m.filter == Filter::Nearest { 9728 } else { 9729 },
                    "minFilter": if m.filter == Filter::Nearest { 9728 } else { 9987 },
                    "wrapS": if m.wrap == Wrap::Clamp { 33071 } else { 10497 },
                    "wrapT": if m.wrap == Wrap::Clamp { 33071 } else { 10497 },
                }));
                textures.push(json!({ "source": img, "sampler": samplers.len() - 1 }));
                pbr["baseColorTexture"] = json!({ "index": textures.len() - 1 });
            }
            let mut out = json!({ "name": m.name, "pbrMetallicRoughness": pbr, "emissiveFactor": [e.r, e.g, e.b] });
            if d.blend != BlendMode::Opaque {
                out["alphaMode"] = json!("BLEND");
            } else if d.alpha_cutoff > 0.0 {
                out["alphaMode"] = json!("MASK");
                out["alphaCutoff"] = json!(d.alpha_cutoff);
            }
            if d.cull == CullMode::None {
                out["doubleSided"] = json!(true);
            }
            out
        })
        .collect();

    // ---- nodes, skins
    let nodes: Vec<Value> = model
        .nodes
        .iter()
        .map(|n| {
            let mut v = json!({ "name": n.name, "translation": n.translation.to_array(), "rotation": n.rotation.normalize().to_array(), "scale": n.scale.to_array() });
            if let Some(m) = n.mesh {
                v["mesh"] = json!(m);
            }
            if let Some(s) = n.skin {
                v["skin"] = json!(s);
            }
            if !n.children.is_empty() {
                v["children"] = json!(n.children);
            }
            v
        })
        .collect();
    let skins: Vec<Value> = model
        .skins
        .iter()
        .map(|s| {
            let m: Vec<f32> = s.inverse_bind.iter().flat_map(|m| m.to_cols_array()).collect();
            json!({ "joints": s.joints, "inverseBindMatrices": bin.floats(&m, "MAT4", 16, None, false) })
        })
        .collect();

    // ---- animations
    let animations: Vec<Value> = model
        .animations
        .iter()
        .filter(|a| !a.channels.is_empty())
        .map(|a| {
            let mut samplers = Vec::new();
            let mut channels = Vec::new();
            for c in &a.channels {
                if c.times.is_empty() || c.values.len() != c.times.len() {
                    continue;
                }
                let input = bin.floats(&c.times, "SCALAR", 1, None, true);
                let (path, n) = match c.path {
                    AnimPath::Translation => ("translation", 3),
                    AnimPath::Rotation => ("rotation", 4),
                    AnimPath::Scale => ("scale", 3),
                };
                let vals: Vec<f32> = c.values.iter().flat_map(|v| v.to_array()[..n].to_vec()).collect();
                let output = bin.floats(&vals, if n == 3 { "VEC3" } else { "VEC4" }, n, None, false);
                samplers.push(json!({ "input": input, "output": output, "interpolation": if c.interpolation == Interpolation::Step { "STEP" } else { "LINEAR" } }));
                channels.push(json!({ "sampler": samplers.len() - 1, "target": { "node": c.node, "path": path } }));
            }
            json!({ "name": a.name, "samplers": samplers, "channels": channels })
        })
        .collect();

    let mut doc = json!({
        "asset": { "version": "2.0", "generator": "GoatEngine model converter" },
        "scene": 0,
        "scenes": [{ "nodes": model.roots }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "buffers": [{ "byteLength": bin.data.len() }],
    });
    let mut put = |k: &str, v: Vec<Value>| {
        if !v.is_empty() {
            doc[k] = Value::Array(v);
        }
    };
    put("images", json_images);
    put("textures", textures);
    put("samplers", samplers);
    put("skins", skins);
    put("animations", animations);
    put("bufferViews", std::mem::take(&mut bin.views));
    put("accessors", std::mem::take(&mut bin.accessors));

    // ---- GLB container: header, JSON chunk (space padded), BIN chunk (zero padded)
    let mut json_bytes = serde_json::to_vec(&doc).unwrap_or_default();
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    let mut data = bin.data;
    while data.len() % 4 != 0 {
        data.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + if data.is_empty() { 0 } else { 8 + data.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    if !data.is_empty() {
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&data);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{parse_model, ModelMaterial, ModelNode, ModelPrimitive};
    use crate::AssetIo;
    use goat_core::math::{Quat, Vec3};
    use goat_gfx::{MaterialDesc, MeshData};

    /// A two-material quad (two submeshes) round-trips through the glTF loader.
    #[test]
    fn two_submeshes_round_trip() {
        let half = |x0: f32| MeshData {
            vertices: (0..4)
                .map(|i| goat_gfx::Vertex { position: [x0 + (i % 2) as f32, (i / 2) as f32, 0.0], normal: [0.0, 0.0, 1.0], uv: [(i % 2) as f32, (i / 2) as f32], color: [1.0; 4] })
                .collect(),
            indices: vec![0, 1, 2, 2, 1, 3],
        };
        let red = MaterialDesc { base_color: Color::rgb(1.0, 0.0, 0.0), shininess: 32.0, ..Default::default() };
        let blue = MaterialDesc { base_color: Color::rgb(0.0, 0.0, 1.0), blend: BlendMode::Alpha, cull: CullMode::None, ..Default::default() };
        let model = ModelData {
            meshes: vec![vec![ModelPrimitive { mesh: half(0.0), material: Some(0), skin_vertices: None }, ModelPrimitive { mesh: half(1.0), material: Some(1), skin_vertices: None }]],
            materials: vec![
                ModelMaterial { name: "Red".into(), desc: red, image: Some(0), filter: Filter::Linear, wrap: Wrap::Repeat },
                ModelMaterial { name: "Blue".into(), desc: blue, image: None, filter: Filter::Linear, wrap: Wrap::Repeat },
            ],
            images: vec![Some(goat_gfx::TextureData::rgba8(2, 2, vec![255; 16]))],
            nodes: vec![ModelNode { name: "Quad".into(), translation: Vec3::new(1.0, 2.0, 3.0), rotation: Quat::IDENTITY, scale: Vec3::ONE, mesh: Some(0), skin: None, children: vec![] }],
            roots: vec![0],
            skins: vec![],
            animations: vec![],
        };
        let glb = write_glb(&model, &[]);
        let back = parse_model(&AssetIo::new("."), "q.glb", &glb).unwrap();
        assert_eq!(back.meshes[0].len(), 2, "one primitive per submesh");
        assert_eq!(back.meshes[0][1].material, Some(1));
        assert_eq!(back.meshes[0][0].mesh.indices, vec![0, 1, 2, 2, 1, 3]);
        assert!((back.materials[0].desc.base_color.r - 1.0).abs() < 1e-3 && back.materials[0].desc.base_color.b.abs() < 1e-3);
        assert!((back.materials[0].desc.shininess - 32.0).abs() < 1.0, "shininess survives: {}", back.materials[0].desc.shininess);
        assert_eq!(back.materials[1].desc.blend, BlendMode::Alpha);
        assert_eq!(back.materials[1].desc.cull, CullMode::None);
        assert!(back.images[0].is_some(), "the texture embedded as PNG");
        assert_eq!(back.nodes[0].translation, Vec3::new(1.0, 2.0, 3.0));
    }
}
