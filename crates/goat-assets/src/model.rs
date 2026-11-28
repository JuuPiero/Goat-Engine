//! glTF 2.0 / GLB → [`ModelData`] (CPU side: meshes, materials, images, node hierarchy).
//!
//! - Buffers: GLB binary chunk, `data:` URIs, or files next to the .gltf.
//! - Images: PNG (embedded or files). Other formats are skipped with a warning.
//! - Primitives: triangles; positions, normals (computed when missing), uv 0, color 0, indices.
//!   Meshes over 65 535 vertices are split (the engine uses 16-bit indices).
//! - Materials: metallic-roughness mapped onto the engine's Blinn-Phong (base color, texture,
//!   emissive, roughness → shininess, alpha mode, double-sided). glTF colors are linear; the
//!   engine's are sRGB-authored, so they are converted.

use goat_core::math::{Mat4, Quat, Vec3, Vec4};
use goat_core::Color;
use goat_gfx::{BlendMode, CullMode, Filter, MaterialDesc, MeshData, TextureData, Vertex, Wrap};

use crate::{resolve_relative, AssetError, AssetIo};

#[derive(Clone, Debug)]
pub struct ModelPrimitive {
    pub mesh: MeshData,
    pub material: Option<usize>,
    /// Per-vertex joints (indices into the skin's joint list) and weights (skinned meshes).
    pub skin_vertices: Option<SkinVertices>,
}

#[derive(Clone, Debug, Default)]
pub struct SkinVertices {
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
}

#[derive(Clone, Debug)]
pub struct ModelSkin {
    /// Node indices of the joints.
    pub joints: Vec<usize>,
    pub inverse_bind: Vec<Mat4>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimPath {
    Translation,
    Rotation,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    Step,
    Linear,
}

#[derive(Clone, Debug)]
pub struct AnimChannel {
    pub node: usize,
    pub path: AnimPath,
    pub interpolation: Interpolation,
    pub times: Vec<f32>,
    /// xyz(w): translations / scales use xyz, rotations are quaternions xyzw.
    pub values: Vec<Vec4>,
}

#[derive(Clone, Debug)]
pub struct ModelAnimation {
    pub name: String,
    pub duration: f32,
    pub channels: Vec<AnimChannel>,
    /// Loop Time (Unity's clip setting, from the model's import settings; default true).
    pub looping: bool,
}

/// A camera of the model file (glTF camera / FBX camera), looking along the node's -Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelCamera {
    pub orthographic: bool,
    /// Vertical field of view (radians).
    pub fov_y: f32,
    /// Orthographic half height.
    pub ortho_size: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelLightKind {
    Directional,
    Point,
    Spot,
}

/// A light of the model file (glTF KHR_lights_punctual / FBX light), along the node's -Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelLight {
    pub kind: ModelLightKind,
    /// The engine's (sRGB-authored) color.
    pub color: Color,
    pub intensity: f32,
    /// 0 = unlimited in the file (the importer picks 10).
    pub range: f32,
    /// Full cone angles in degrees (Unity's Spot Angle / Inner Spot Angle).
    pub spot_angle: f32,
    pub inner_spot_angle: f32,
}

#[derive(Clone, Debug)]
pub struct ModelMaterial {
    pub name: String,
    pub desc: MaterialDesc,
    /// Index into [`ModelData::images`].
    pub image: Option<usize>,
    pub filter: Filter,
    pub wrap: Wrap,
}

#[derive(Clone, Debug)]
pub struct ModelNode {
    pub name: String,
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    pub mesh: Option<usize>,
    pub skin: Option<usize>,
    pub children: Vec<usize>,
    pub camera: Option<ModelCamera>,
    pub light: Option<ModelLight>,
}

impl Default for ModelNode {
    fn default() -> Self {
        ModelNode { name: String::new(), translation: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::ONE, mesh: None, skin: None, children: Vec::new(), camera: None, light: None }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ModelData {
    /// Meters per unit of the source file (FBX; glTF = 1, OBJ = None), for the importer's
    /// Convert Units label. Stored in converted GLBs (`asset.extras.goat.fileUnit`).
    pub file_unit: Option<f32>,
    /// The file's own scale was converted to meters (FBX Convert Units on).
    pub units_converted: bool,
    /// Per glTF mesh, its primitives (after splitting).
    pub meshes: Vec<Vec<ModelPrimitive>>,
    pub materials: Vec<ModelMaterial>,
    pub images: Vec<Option<TextureData>>,
    pub nodes: Vec<ModelNode>,
    /// Root nodes of the default scene.
    pub roots: Vec<usize>,
    pub skins: Vec<ModelSkin>,
    pub animations: Vec<ModelAnimation>,
}

impl ModelData {
    pub fn animation(&self, name: &str) -> Option<&ModelAnimation> {
        self.animations.iter().find(|a| a.name == name)
    }
}

fn data_uri(uri: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let rest = uri.strip_prefix("data:")?;
    let (_, b64) = rest.split_once(";base64,")?;
    base64::engine::general_purpose::STANDARD.decode(b64).ok()
}

fn uri_bytes(io: &AssetIo, key: &str, uri: &str) -> Result<Vec<u8>, AssetError> {
    if uri.starts_with("data:") {
        return data_uri(uri).ok_or_else(|| AssetError::Format(key.into(), "bad data uri".into()));
    }
    let decoded = uri.replace("%20", " ");
    io.read(&resolve_relative(key, &decoded))
}

/// Loads `key` (a model under the asset root) shaped by its import settings (`<key>.import.json`,
/// see [`crate::model_import`]).
pub fn load_model(io: &AssetIo, key: &str) -> Result<ModelData, AssetError> {
    let bytes = io.read(key)?;
    let mut data = parse_model(io, key, &bytes)?;
    crate::model_import::apply(&mut data, &crate::model_import::ModelImport::of(io, key));
    Ok(data)
}

/// Turns a model file the glTF loader can't read (FBX, OBJ) into GLB bytes: installed by the host
/// tools that link a converter (goat-fbx: the editor, `goat build`, the desktop player). Builds for
/// the 3DS / the web ship models already converted, so they don't need one.
pub type ModelConverter = fn(io: &AssetIo, key: &str, bytes: &[u8]) -> Result<Vec<u8>, String>;

static CONVERTER: std::sync::OnceLock<ModelConverter> = std::sync::OnceLock::new();

/// Installs the model converter (first call wins).
pub fn set_model_converter(f: ModelConverter) {
    let _ = CONVERTER.set(f);
}

/// GLB, or glTF JSON (after an optional BOM / whitespace).
pub fn is_gltf(bytes: &[u8]) -> bool {
    let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    b.starts_with(b"glTF") || b.iter().find(|c| !c.is_ascii_whitespace()) == Some(&b'{')
}

pub fn parse_model(io: &AssetIo, key: &str, bytes: &[u8]) -> Result<ModelData, AssetError> {
    let fmt = |e: String| AssetError::Format(key.to_string(), e);
    if !is_gltf(bytes) {
        let Some(convert) = CONVERTER.get() else {
            return Err(fmt("not a glTF / GLB file: FBX / OBJ models are converted by the editor and `goat build` (no converter in this player)".into()));
        };
        let glb = convert(io, key, bytes).map_err(fmt)?;
        return parse_model(io, key, &glb);
    }
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| fmt(e.to_string()))?;
    let doc = &gltf.document;

    let mut buffers: Vec<Vec<u8>> = Vec::new();
    for b in doc.buffers() {
        buffers.push(match b.source() {
            gltf::buffer::Source::Bin => gltf.blob.clone().ok_or_else(|| fmt("GLB without a binary chunk".into()))?,
            gltf::buffer::Source::Uri(uri) => uri_bytes(io, key, uri)?,
        });
    }

    let mut images = Vec::new();
    for img in doc.images() {
        let data = match img.source() {
            gltf::image::Source::View { view, mime_type } => {
                let b = &buffers[view.buffer().index()];
                let slice = b.get(view.offset()..view.offset() + view.length()).map(|s| s.to_vec());
                if mime_type != "image/png" && mime_type != "image/jpeg" && mime_type != "image/ktx" {
                    goat_core::warn!("{key}: image {} is {mime_type}, only PNG / JPEG / KTX are supported", img.index());
                    None
                } else {
                    slice
                }
            }
            gltf::image::Source::Uri { uri, .. } => {
                let lower = uri.to_ascii_lowercase();
                if [".png", ".jpg", ".jpeg"].iter().any(|e| lower.ends_with(e)) || uri.starts_with("data:image/png") || uri.starts_with("data:image/jpeg") {
                    Some(uri_bytes(io, key, uri)?)
                } else {
                    goat_core::warn!("{key}: image {uri} skipped (only PNG / JPEG are supported)");
                    None
                }
            }
        };
        // JPEG starts with FF D8, PNG with 89 'P' 'N' 'G', KTX with «KTX 11» (builds precompress
        // the textures next to a model under their own names: the bytes tell)
        let decode = |d: &[u8]| {
            if d.starts_with(&[0xFF, 0xD8]) {
                crate::image::decode_jpeg(d)
            } else if d.starts_with(&[0xAB, 0x4B, 0x54, 0x58]) {
                crate::image::decode_ktx(d)
            } else {
                crate::image::decode_png(d)
            }
        };
        images.push(data.and_then(|d| match decode(&d) {
            Ok(t) => Some(t),
            Err(e) => {
                goat_core::warn!("{key}: image {}: {e}", img.index());
                None
            }
        }));
    }

    let materials = doc
        .materials()
        .map(|m| {
            let pbr = m.pbr_metallic_roughness();
            let base = pbr.base_color_factor();
            let roughness = pbr.roughness_factor().clamp(0.02, 1.0);
            let metallic = pbr.metallic_factor();
            let e = m.emissive_factor();
            let (image, filter, wrap) = match pbr.base_color_texture() {
                Some(info) => {
                    let tex = info.texture();
                    let s = tex.sampler();
                    let filter = if s.mag_filter() == Some(gltf::texture::MagFilter::Nearest) { Filter::Nearest } else { Filter::Linear };
                    let wrap = if s.wrap_s() == gltf::texture::WrappingMode::ClampToEdge { Wrap::Clamp } else { Wrap::Repeat };
                    (Some(tex.source().index()), filter, wrap)
                }
                None => (None, Filter::Linear, Wrap::Repeat),
            };
            let blend = match m.alpha_mode() {
                gltf::material::AlphaMode::Opaque => BlendMode::Opaque,
                _ => BlendMode::Alpha,
            };
            let spec = (1.0 - roughness) * (0.3 + 0.5 * metallic);
            ModelMaterial {
                name: m.name().unwrap_or("material").to_string(),
                desc: MaterialDesc {
                    base_color: Color::rgba(base[0], base[1], base[2], base[3]).from_linear(),
                    specular: Color::rgb(spec, spec, spec),
                    shininess: (2.0 / roughness.powi(4) - 2.0).clamp(2.0, 256.0),
                    emissive: Color::rgb(e[0], e[1], e[2]).from_linear(),
                    blend,
                    cull: if m.double_sided() { CullMode::None } else { CullMode::Back },
                    depth_write: m.alpha_mode() != gltf::material::AlphaMode::Blend,
                    ..Default::default()
                },
                image,
                filter,
                wrap,
            }
        })
        .collect();

    let mut meshes = Vec::new();
    for mesh in doc.meshes() {
        let mut prims = Vec::new();
        for p in mesh.primitives() {
            if p.mode() != gltf::mesh::Mode::Triangles {
                goat_core::warn!("{key}: mesh {} has a non-triangle primitive (skipped)", mesh.index());
                continue;
            }
            let reader = p.reader(|b| buffers.get(b.index()).map(|v| v.as_slice()));
            let Some(positions) = reader.read_positions() else { continue };
            let positions: Vec<[f32; 3]> = positions.collect();
            let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|n| n.collect());
            let uvs: Option<Vec<[f32; 2]>> = reader.read_tex_coords(0).map(|t| t.into_f32().collect());
            let colors: Option<Vec<[f32; 4]>> = reader.read_colors(0).map(|c| c.into_rgba_f32().collect());
            let indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            let mut vertices: Vec<Vertex> = positions
                .iter()
                .enumerate()
                .map(|(i, &pos)| Vertex {
                    position: pos,
                    normal: normals.as_ref().and_then(|n| n.get(i).copied()).unwrap_or([0.0, 0.0, 0.0]),
                    uv: uvs.as_ref().and_then(|t| t.get(i).copied()).unwrap_or([0.0, 0.0]),
                    // vertex colors are linear in glTF too
                    color: colors
                        .as_ref()
                        .and_then(|c| c.get(i))
                        .map(|c| Color::rgba(c[0], c[1], c[2], c[3]).from_linear().to_array())
                        .unwrap_or([1.0; 4]),
                })
                .collect();
            if normals.is_none() {
                compute_normals(&mut vertices, &indices);
            }
            let joints: Option<Vec<[u16; 4]>> = reader.read_joints(0).map(|j| j.into_u16().collect());
            let weights: Option<Vec<[f32; 4]>> = reader.read_weights(0).map(|w| w.into_f32().collect());
            let mut skin_vertices = match (joints, weights) {
                (Some(joints), Some(weights)) if joints.len() == vertices.len() && weights.len() == vertices.len() => Some(SkinVertices { joints, weights }),
                _ => None,
            };
            let parts = split_u16(&vertices, &indices);
            if parts.len() > 1 && skin_vertices.is_some() {
                goat_core::warn!("{key}: skinned mesh {} over 65 535 vertices: skinning dropped", mesh.index());
                skin_vertices = None;
            }
            for mesh in parts {
                prims.push(ModelPrimitive { mesh, material: p.material().index(), skin_vertices: skin_vertices.clone() });
            }
        }
        meshes.push(prims);
    }

    // what the gltf crate drops without extra features: KHR_lights_punctual, asset extras
    let raw = raw_json(bytes).unwrap_or(serde_json::Value::Null);
    let lights: Vec<ModelLight> = raw["extensions"]["KHR_lights_punctual"]["lights"].as_array().map(|l| l.iter().map(light_of).collect()).unwrap_or_default();
    let nodes = doc
        .nodes()
        .map(|n| {
            let (t, r, s) = n.transform().decomposed();
            let camera = n.camera().map(|c| match c.projection() {
                gltf::camera::Projection::Perspective(p) => ModelCamera { orthographic: false, fov_y: p.yfov(), ortho_size: 5.0, near: p.znear(), far: p.zfar().unwrap_or(1000.0) },
                gltf::camera::Projection::Orthographic(o) => ModelCamera { orthographic: true, fov_y: 60f32.to_radians(), ortho_size: o.ymag(), near: o.znear(), far: o.zfar() },
            });
            let light = raw["nodes"][n.index()]["extensions"]["KHR_lights_punctual"]["light"].as_u64().and_then(|i| lights.get(i as usize).copied());
            ModelNode {
                name: n.name().map(str::to_string).unwrap_or_else(|| format!("Node {}", n.index())),
                translation: Vec3::from(t),
                rotation: Quat::from_array(r).normalize(),
                scale: Vec3::from(s),
                mesh: n.mesh().map(|m| m.index()),
                skin: n.skin().map(|s| s.index()),
                children: n.children().map(|c| c.index()).collect(),
                camera,
                light,
            }
        })
        .collect();
    let goat = &raw["asset"]["extras"]["goat"];
    let file_unit = goat["fileUnit"].as_f64().map(|v| v as f32).or(Some(1.0));
    let units_converted = goat["unitsConverted"].as_bool().unwrap_or(true);
    let roots = doc
        .default_scene()
        .or_else(|| doc.scenes().next())
        .map(|s| s.nodes().map(|n| n.index()).collect())
        .unwrap_or_default();
    let skins = doc
        .skins()
        .map(|s| {
            let joints: Vec<usize> = s.joints().map(|j| j.index()).collect();
            let inverse_bind = s
                .reader(|b| buffers.get(b.index()).map(|v| v.as_slice()))
                .read_inverse_bind_matrices()
                .map(|m| m.map(|c| Mat4::from_cols_array_2d(&c)).collect())
                .unwrap_or_else(|| vec![Mat4::IDENTITY; joints.len()]);
            ModelSkin { joints, inverse_bind }
        })
        .collect();
    let animations = doc
        .animations()
        .map(|a| {
            let mut channels = Vec::new();
            for ch in a.channels() {
                let reader = ch.reader(|b| buffers.get(b.index()).map(|v| v.as_slice()));
                let (Some(times), Some(outputs)) = (reader.read_inputs(), reader.read_outputs()) else { continue };
                let times: Vec<f32> = times.collect();
                use gltf::animation::util::ReadOutputs;
                let (path, values): (AnimPath, Vec<Vec4>) = match outputs {
                    ReadOutputs::Translations(t) => (AnimPath::Translation, t.map(|v| Vec3::from(v).extend(0.0)).collect()),
                    ReadOutputs::Rotations(r) => (AnimPath::Rotation, r.into_f32().map(Vec4::from).collect()),
                    ReadOutputs::Scales(s) => (AnimPath::Scale, s.map(|v| Vec3::from(v).extend(0.0)).collect()),
                    ReadOutputs::MorphTargetWeights(_) => continue,
                };
                let (interpolation, values) = match ch.sampler().interpolation() {
                    gltf::animation::Interpolation::Step => (Interpolation::Step, values),
                    gltf::animation::Interpolation::Linear => (Interpolation::Linear, values),
                    // in-tangent, value, out-tangent per key: keep the values, interpolate linearly
                    gltf::animation::Interpolation::CubicSpline => (Interpolation::Linear, values.chunks_exact(3).map(|c| c[1]).collect()),
                };
                if values.len() != times.len() || times.is_empty() {
                    continue;
                }
                channels.push(AnimChannel { node: ch.target().node().index(), path, interpolation, times, values });
            }
            let duration = channels.iter().filter_map(|c| c.times.last().copied()).fold(0.0, f32::max);
            ModelAnimation { name: a.name().map(str::to_string).unwrap_or_else(|| format!("Animation {}", a.index())), duration, channels, looping: true }
        })
        .collect();
    Ok(ModelData { file_unit, units_converted, meshes, materials, images, nodes, roots, skins, animations })
}

/// The JSON of a .gltf / .glb (for the parts the gltf crate doesn't expose).
fn raw_json(bytes: &[u8]) -> Option<serde_json::Value> {
    let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if b.starts_with(b"glTF") {
        let len = u32::from_le_bytes(b.get(12..16)?.try_into().ok()?) as usize;
        return serde_json::from_slice(b.get(20..20 + len)?).ok();
    }
    serde_json::from_slice(b).ok()
}

/// A KHR_lights_punctual light (linear color → the engine's sRGB-authored color).
fn light_of(l: &serde_json::Value) -> ModelLight {
    let c = |i: usize| l["color"][i].as_f64().unwrap_or(1.0) as f32;
    let kind = match l["type"].as_str() {
        Some("directional") => ModelLightKind::Directional,
        Some("spot") => ModelLightKind::Spot,
        _ => ModelLightKind::Point,
    };
    let outer = l["spot"]["outerConeAngle"].as_f64().unwrap_or(std::f64::consts::FRAC_PI_4) as f32;
    let inner = l["spot"]["innerConeAngle"].as_f64().unwrap_or(0.0) as f32;
    ModelLight {
        kind,
        color: Color::rgb(c(0), c(1), c(2)).from_linear(),
        intensity: l["intensity"].as_f64().unwrap_or(1.0) as f32,
        range: l["range"].as_f64().unwrap_or(0.0) as f32,
        spot_angle: (outer * 2.0).to_degrees(),
        inner_spot_angle: (inner * 2.0).to_degrees(),
    }
}

/// Smooth normals from the triangles (models without normals).
fn compute_normals(vertices: &mut [Vertex], indices: &[u32]) {
    let mut acc = vec![Vec3::ZERO; vertices.len()];
    for t in indices.chunks_exact(3) {
        let [a, b, c] = [t[0] as usize, t[1] as usize, t[2] as usize];
        if a.max(b).max(c) >= vertices.len() {
            continue;
        }
        let (pa, pb, pc) = (Vec3::from(vertices[a].position), Vec3::from(vertices[b].position), Vec3::from(vertices[c].position));
        let n = (pb - pa).cross(pc - pa);
        acc[a] += n;
        acc[b] += n;
        acc[c] += n;
    }
    for (v, n) in vertices.iter_mut().zip(acc) {
        v.normal = n.normalize_or(Vec3::Y).into();
    }
}

/// Splits a triangle list into meshes of at most 65 535 vertices (16-bit indices).
pub fn split_u16(vertices: &[Vertex], indices: &[u32]) -> Vec<MeshData> {
    if vertices.len() <= u16::MAX as usize {
        return vec![MeshData { vertices: vertices.to_vec(), indices: indices.iter().map(|&i| i as u16).collect() }];
    }
    let mut out = Vec::new();
    let mut remap = std::collections::HashMap::new();
    let mut cur = MeshData::default();
    for t in indices.chunks_exact(3) {
        if cur.vertices.len() + 3 > u16::MAX as usize {
            out.push(std::mem::take(&mut cur));
            remap.clear();
        }
        for &i in t {
            let new = *remap.entry(i).or_insert_with(|| {
                cur.vertices.push(vertices[i as usize]);
                (cur.vertices.len() - 1) as u16
            });
            cur.indices.push(new);
        }
    }
    if !cur.indices.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-triangle glTF with an embedded buffer (positions only: normals are computed).
    #[test]
    fn embedded_gltf_triangle() {
        use base64::Engine;
        let floats: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"name":"Tri","mesh":0,"translation":[1,2,3]}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"material":0}}]}}],
            "materials":[{{"pbrMetallicRoughness":{{"baseColorFactor":[0.21586,0.21586,0.21586,1]}}}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}}],
            "bufferViews":[{{"buffer":0,"byteLength":36}}],
            "buffers":[{{"byteLength":36,"uri":"data:application/octet-stream;base64,{b64}"}}]}}"#
        );
        let m = parse_model(&AssetIo::new("."), "t.gltf", json.as_bytes()).unwrap();
        assert_eq!(m.roots, vec![0]);
        assert_eq!(m.nodes[0].translation, Vec3::new(1.0, 2.0, 3.0));
        let p = &m.meshes[0][0];
        assert_eq!(p.mesh.indices, vec![0, 1, 2]);
        assert!((Vec3::from(p.mesh.vertices[0].normal) - Vec3::Z).length() < 1e-5);
        // linear 0.21586 = sRGB 0.5
        assert!((m.materials[0].desc.base_color.r - 0.5).abs() < 0.01);
    }

    #[test]
    fn splits_big_meshes() {
        let n = 70_000u32;
        let vertices = vec![Vertex::default(); n as usize];
        let indices: Vec<u32> = (0..n - n % 3).collect();
        let parts = split_u16(&vertices, &indices);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|p| p.vertices.len() <= 65535));
        assert_eq!(parts.iter().map(|p| p.indices.len()).sum::<usize>(), indices.len());
    }
}

#[cfg(test)]
mod sample_tests {
    /// The sample's Blender export: a 4-bone skinned tentacle ("Sway") and a hopping cube ("Hop").
    #[test]
    fn blender_skin_and_animations() {
        let io = crate::AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets"));
        let m = super::load_model(&io, "models/tentacle.glb").unwrap();
        assert_eq!(m.skins.len(), 1);
        assert_eq!(m.skins[0].joints.len(), 4);
        let skinned = m.meshes.iter().flatten().filter(|p| p.skin_vertices.is_some()).count();
        assert_eq!(skinned, 1);
        let names: Vec<&str> = m.animations.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"Sway") && names.contains(&"Hop"), "{names:?}");
        assert!(m.animation("Sway").unwrap().duration > 1.5);
    }
}
