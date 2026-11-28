//! FBX / OBJ → the engine's [`ModelData`] → GLB (Unity's model import for formats the runtime
//! doesn't read). Host-only (ufbx is C): the editor converts when it loads a model, `goat build`
//! converts every `.fbx` / `.obj` it ships (same file name, GLB bytes: loaders look at the bytes),
//! and the desktop player converts at load ([`install`]). The 3DS and the web only ever see GLB.
//!
//! - Space: right-handed, Y up, meters (Unity's Convert Units): ufbx converts axes / units into
//!   the geometry; geometric transforms (3ds Max pivots) become helper nodes.
//! - Meshes: one submesh per material (Unity's MeshFilter + MeshRenderer materials), polygons
//!   triangulated, vertices de-duplicated, split above 65 535 vertices (16-bit indices).
//! - Materials: diffuse / base color (+ texture), opacity, specular exponent → shininess, emission.
//! - Textures: embedded media (FBX "Video") stored in the GLB — precompressed ETC1 for the 3DS —
//!   or files found next to the model (by relative path, file name, or `textures/`), kept as files
//!   so builds compress them like any texture.
//! - Skins: clusters → joints + inverse bind matrices, 4 weights per vertex.
//! - Animations: every take (anim stack), baked to keys at `animation_rate` with key reduction.

use std::collections::HashMap;

use goat_assets::glb::{write_glb, ImageSource};
use goat_assets::{AnimChannel, AnimPath, Interpolation, ModelAnimation, ModelCamera, ModelData, ModelLight, ModelLightKind, ModelMaterial, ModelNode, ModelPrimitive, ModelSkin, SkinVertices};
use goat_core::math::{Mat4, Quat, Vec3, Vec4};
use goat_core::Color;
use goat_gfx::{BlendMode, Filter, MaterialDesc, Vertex, Wrap};

/// The platform the converted model is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    #[default]
    Desktop,
    /// Embedded textures become ETC1 / ETC1A4 KTX (power-of-two sizes), like the build's textures.
    N3ds,
}

#[derive(Clone, Debug)]
pub struct ConvertOptions {
    pub target: Target,
    /// Multiplies the converted file itself (the `fbx2glb` tool's `--scale`). The importer's Scale
    /// Factor is NOT applied here: goat-assets applies it when loading, for every format.
    pub scale: f32,
    /// Unity's Convert Units: the file's unit → meters. Off: one file unit = one meter.
    pub convert_units: bool,
    /// Animation keys per second when baking takes.
    pub animation_rate: f64,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        ConvertOptions { target: Target::Desktop, scale: 1.0, convert_units: true, animation_rate: 30.0 }
    }
}

/// Model files this crate converts.
pub fn is_convertible(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.ends_with(".fbx") || k.ends_with(".obj")
}

/// Installs the converter in goat-assets: models that aren't glTF are converted when loaded.
pub fn install() {
    goat_assets::set_model_converter(|io, key, bytes| {
        let read = |k: &str| io.read(k).ok();
        convert(bytes, key, &read, &options_for(key, &read, Target::Desktop))
    });
}

/// The conversion options of a model from `<model>.import.json` (goat-assets `ModelImport`: the
/// converter uses `convert_units` and `animation_rate`; the rest applies when the GLB is loaded).
pub fn options_for(key: &str, read: &dyn Fn(&str) -> Option<Vec<u8>>, target: Target) -> ConvertOptions {
    let s = read(&format!("{key}.import.json")).and_then(|b| goat_assets::ModelImport::parse(&String::from_utf8_lossy(&b)).ok()).unwrap_or_default();
    ConvertOptions { target, scale: 1.0, convert_units: s.convert_units, animation_rate: (s.animation_rate as f64).clamp(1.0, 240.0) }
}

/// FBX / OBJ bytes → GLB bytes. `key` is the model's asset key (format detection, texture
/// lookup); `read` reads another asset by key (textures next to the model).
pub fn convert(bytes: &[u8], key: &str, read: &dyn Fn(&str) -> Option<Vec<u8>>, opts: &ConvertOptions) -> Result<Vec<u8>, String> {
    let (model, images) = import(bytes, key, read, opts)?;
    Ok(write_glb(&model, &images))
}

fn v3(v: ufbx::Vec3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}
fn quat(q: ufbx::Quat) -> Quat {
    Quat::from_xyzw(q.x as f32, q.y as f32, q.z as f32, q.w as f32).normalize()
}
fn mat4(m: &ufbx::Matrix) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(m.m00 as f32, m.m10 as f32, m.m20 as f32, 0.0),
        Vec4::new(m.m01 as f32, m.m11 as f32, m.m21 as f32, 0.0),
        Vec4::new(m.m02 as f32, m.m12 as f32, m.m22 as f32, 0.0),
        Vec4::new(m.m03 as f32, m.m13 as f32, m.m23 as f32, 1.0),
    )
}
/// A material map's value (x..w), or `default` when the file doesn't set it.
fn map4(m: &ufbx::MaterialMap, default: [f32; 4]) -> [f32; 4] {
    if !m.has_value {
        return default;
    }
    let v = m.value_vec4;
    [v.x as f32, v.y as f32, v.z as f32, v.w as f32]
}
fn map1(m: &ufbx::MaterialMap, default: f32) -> f32 {
    if m.has_value {
        m.value_vec4.x as f32
    } else {
        default
    }
}

/// The folder of a key (`models/a.fbx` → `models/`).
fn dir_of(key: &str) -> &str {
    key.rfind('/').map_or("", |i| &key[..=i])
}

/// Lexically normalizes `a/b/../c` → `a/c`.
fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            p => out.push(p),
        }
    }
    out.join("/")
}

/// FBX / OBJ → model data + how each image is stored (see the module docs).
pub fn import(bytes: &[u8], key: &str, read: &dyn Fn(&str) -> Option<Vec<u8>>, opts: &ConvertOptions) -> Result<(ModelData, Vec<Option<ImageSource>>), String> {
    let scene = ufbx::load_memory(
        bytes,
        ufbx::LoadOpts {
            target_axes: ufbx::CoordinateAxes::right_handed_y_up(),
            // Convert Units off: no unit conversion (0), the file's numbers are meters
            target_unit_meters: if opts.convert_units { (1.0 / opts.scale.max(1e-6)) as ufbx::Real } else { 0.0 },
            // cameras and lights look along -Z with +Y up, like glTF and the engine
            target_camera_axes: ufbx::CoordinateAxes::right_handed_y_up(),
            target_light_axes: ufbx::CoordinateAxes::right_handed_y_up(),
            space_conversion: ufbx::SpaceConversion::ModifyGeometry,
            geometry_transform_handling: ufbx::GeometryTransformHandling::HelperNodes,
            generate_missing_normals: true,
            filename: key.into(),
            ..Default::default()
        },
    )
    .map_err(|e| format!("{key}: {}", e.description))?;
    let dir = dir_of(key).to_string();

    // ---- nodes (the file's root node is the model's root: its children are the glTF roots)
    let mut node_index: Vec<Option<usize>> = vec![None; scene.nodes.count];
    let mut order: Vec<&ufbx::Node> = Vec::new();
    for n in scene.nodes.iter() {
        if n.is_root {
            continue;
        }
        node_index[n.element.typed_id as usize] = Some(order.len());
        order.push(n);
    }
    let mapped = |n: &ufbx::Node| node_index.get(n.element.typed_id as usize).copied().flatten();

    // ---- materials + images
    let mut materials = Vec::new();
    let mut image_sources: Vec<Option<ImageSource>> = Vec::new();
    let mut image_by_texture: HashMap<u32, Option<usize>> = HashMap::new();
    let mut image_by_source: HashMap<String, usize> = HashMap::new();
    for m in scene.materials.iter() {
        let fbx = &m.fbx;
        let pbr = &m.pbr;
        let (color, factor) = if pbr.base_color.has_value { (map4(&pbr.base_color, [1.0; 4]), map1(&pbr.base_factor, 1.0)) } else { (map4(&fbx.diffuse_color, [0.8, 0.8, 0.8, 1.0]), map1(&fbx.diffuse_factor, 1.0)) };
        let opacity = map1(&pbr.opacity, 1.0).clamp(0.0, 1.0);
        let emission = map4(&fbx.emission_color, [0.0; 4]);
        let ef = map1(&fbx.emission_factor, 1.0);
        let spec = map4(&fbx.specular_color, [0.2, 0.2, 0.2, 1.0]);
        let sf = map1(&fbx.specular_factor, 1.0);
        let texture = pbr.base_color.texture.as_ref().or(fbx.diffuse_color.texture.as_ref());
        let mut wrap = Wrap::Repeat;
        let image = match texture {
            Some(t) => {
                wrap = if matches!(t.wrap_u, ufbx::WrapMode::Clamp) { Wrap::Clamp } else { Wrap::Repeat };
                *image_by_texture.entry(t.element.typed_id).or_insert_with(|| {
                    let src = image_source(t, &dir, read, opts)?;
                    let id = match &src {
                        ImageSource::Uri(u) => u.clone(),
                        ImageSource::Embedded { bytes, .. } => format!("embedded:{}:{}", t.element.typed_id, bytes.len()),
                    };
                    Some(*image_by_source.entry(id).or_insert_with(|| {
                        image_sources.push(Some(src));
                        image_sources.len() - 1
                    }))
                })
            }
            None => None,
        };
        // a texture multiplies the color: FBX files often keep a dark diffuse color next to one
        let base = if image.is_some() && !pbr.base_color.has_value { [1.0, 1.0, 1.0] } else { [color[0] * factor, color[1] * factor, color[2] * factor] };
        materials.push(ModelMaterial {
            name: m.element.name.to_string(),
            desc: MaterialDesc {
                base_color: Color::rgba(base[0].clamp(0.0, 1.0), base[1].clamp(0.0, 1.0), base[2].clamp(0.0, 1.0), opacity),
                specular: Color::rgb((spec[0] * sf).clamp(0.0, 1.0), (spec[1] * sf).clamp(0.0, 1.0), (spec[2] * sf).clamp(0.0, 1.0)),
                shininess: map1(&fbx.specular_exponent, 20.0).clamp(2.0, 256.0),
                emissive: Color::rgb((emission[0] * ef).clamp(0.0, 1.0), (emission[1] * ef).clamp(0.0, 1.0), (emission[2] * ef).clamp(0.0, 1.0)),
                blend: if opacity < 0.999 { BlendMode::Alpha } else { BlendMode::Opaque },
                depth_write: opacity >= 0.999,
                ..Default::default()
            },
            image,
            filter: Filter::Linear,
            wrap,
        });
    }
    let material_index = |m: &ufbx::Material| Some(m.element.typed_id as usize).filter(|&i| i < materials.len());

    // ---- skins (one per skin deformer)
    let mut skins = Vec::new();
    let mut skin_by_deformer: HashMap<u32, usize> = HashMap::new();
    for d in scene.skin_deformers.iter() {
        let joints: Vec<usize> = d.clusters.iter().map(|c| c.bone_node.as_ref().and_then(|b| mapped(b)).unwrap_or(0)).collect();
        let inverse_bind = d.clusters.iter().map(|c| mat4(&c.geometry_to_bone)).collect();
        skin_by_deformer.insert(d.element.typed_id, skins.len());
        skins.push(ModelSkin { joints, inverse_bind });
    }

    // ---- meshes: a primitive per material part
    let mut meshes = Vec::new();
    for mesh in scene.meshes.iter() {
        let mut prims = Vec::new();
        let skin = mesh.skin_deformers.iter().next();
        let mut tri = vec![0u32; mesh.max_face_triangles.max(1) * 3];
        for part in mesh.material_parts.iter() {
            if part.num_triangles == 0 {
                continue;
            }
            let mut vertices: Vec<Vertex> = Vec::new();
            let mut joints: Vec<[u16; 4]> = Vec::new();
            let mut weights: Vec<[f32; 4]> = Vec::new();
            let mut indices: Vec<u32> = Vec::new();
            let mut seen: HashMap<(u32, [u32; 9]), u32> = HashMap::new();
            for &fi in part.face_indices.iter() {
                let Some(&face) = mesh.faces.get(fi as usize) else { continue };
                let n = ufbx::triangulate_face(&mut tri, mesh, face) as usize;
                for &ix in &tri[..n * 3] {
                    let ix = ix as usize;
                    let p = v3(mesh.vertex_position[ix]);
                    let nrm = if mesh.vertex_normal.exists { v3(mesh.vertex_normal[ix]).normalize_or(Vec3::Y) } else { Vec3::Y };
                    // FBX UVs start at the bottom, glTF's at the top
                    let uv = if mesh.vertex_uv.exists {
                        let t = mesh.vertex_uv[ix];
                        [t.x as f32, 1.0 - t.y as f32]
                    } else {
                        [0.0, 0.0]
                    };
                    let color = if mesh.vertex_color.exists {
                        let c = mesh.vertex_color[ix];
                        [c.x as f32, c.y as f32, c.z as f32, c.w as f32]
                    } else {
                        [1.0; 4]
                    };
                    let control = mesh.vertex_indices.get(ix).copied().unwrap_or(0);
                    let k = (control, [nrm.x.to_bits(), nrm.y.to_bits(), nrm.z.to_bits(), uv[0].to_bits(), uv[1].to_bits(), color[0].to_bits(), color[1].to_bits(), color[2].to_bits(), color[3].to_bits()]);
                    let id = *seen.entry(k).or_insert_with(|| {
                        vertices.push(Vertex { position: p.into(), normal: nrm.into(), uv, color });
                        if let Some(d) = skin {
                            let (j, w) = skin_weights(d, control as usize);
                            joints.push(j);
                            weights.push(w);
                        }
                        (vertices.len() - 1) as u32
                    });
                    indices.push(id);
                }
            }
            let material = mesh.materials.get(part.index as usize).and_then(|m| material_index(m));
            let mut skin_vertices = skin.map(|_| SkinVertices { joints, weights });
            let parts = goat_assets::model::split_u16(&vertices, &indices);
            if parts.len() > 1 && skin_vertices.is_some() {
                goat_core::warn!("{key}: skinned mesh '{}' over 65 535 vertices: skinning dropped", mesh.element.name);
                skin_vertices = None;
            }
            for m in parts {
                prims.push(ModelPrimitive { mesh: m, material, skin_vertices: skin_vertices.clone() });
            }
        }
        meshes.push(prims);
    }

    let nodes: Vec<ModelNode> = order
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let t = &n.local_transform;
            let mesh = n.mesh.as_ref().map(|m| m.element.typed_id as usize);
            let skin = n.mesh.as_ref().and_then(|m| m.skin_deformers.iter().next()).and_then(|d| skin_by_deformer.get(&d.element.typed_id).copied());
            let name = n.element.name.to_string();
            let camera = n.camera.as_ref().map(|c| {
                let near = c.near_plane as f32;
                let far = c.far_plane as f32;
                if c.projection_mode == ufbx::ProjectionMode::Orthographic {
                    ModelCamera { orthographic: true, fov_y: 60f32.to_radians(), ortho_size: (c.orthographic_size.y as f32 * 0.5).max(1e-3), near, far }
                } else {
                    ModelCamera { orthographic: false, fov_y: (c.field_of_view_deg.y as f32).to_radians(), ortho_size: 5.0, near, far }
                }
            });
            let light = n.light.as_ref().and_then(|l| {
                let kind = match l.type_ {
                    ufbx::LightType::Directional => ModelLightKind::Directional,
                    ufbx::LightType::Spot => ModelLightKind::Spot,
                    ufbx::LightType::Point => ModelLightKind::Point,
                    _ => return None, // area / volume lights: Unity skips them too
                };
                Some(ModelLight {
                    kind,
                    color: Color::rgb(l.color.x as f32, l.color.y as f32, l.color.z as f32),
                    intensity: l.intensity as f32,
                    range: 0.0,
                    spot_angle: l.outer_angle as f32,
                    inner_spot_angle: l.inner_angle as f32,
                })
            });
            ModelNode {
                name: if name.is_empty() { format!("Node {i}") } else { name },
                translation: v3(t.translation),
                rotation: quat(t.rotation),
                scale: v3(t.scale),
                mesh,
                skin,
                children: n.children.iter().filter_map(|c| mapped(c)).collect(),
                camera,
                light,
            }
        })
        .collect();
    let roots: Vec<usize> = scene.root_node.children.iter().filter_map(|c| mapped(c)).collect();

    // ---- animations: every take, baked
    let mut animations = Vec::new();
    for stack in scene.anim_stacks.iter() {
        let baked = match ufbx::bake_anim(&scene, &stack.anim, ufbx::BakeOpts { trim_start_time: true, resample_rate: opts.animation_rate, key_reduction_enabled: true, ..Default::default() }) {
            Ok(b) => b,
            Err(e) => {
                goat_core::warn!("{key}: take '{}': {}", stack.element.name, e.description);
                continue;
            }
        };
        let mut channels = Vec::new();
        for bn in baked.nodes.iter() {
            let Some(node) = node_index.get(bn.typed_id as usize).copied().flatten() else { continue };
            let mut push = |path: AnimPath, times: Vec<f32>, values: Vec<Vec4>| {
                if !times.is_empty() && times.len() == values.len() {
                    channels.push(AnimChannel { node, path, interpolation: Interpolation::Linear, times, values });
                }
            };
            push(AnimPath::Translation, bn.translation_keys.iter().map(|k| k.time as f32).collect(), bn.translation_keys.iter().map(|k| v3(k.value).extend(0.0)).collect());
            push(AnimPath::Rotation, bn.rotation_keys.iter().map(|k| k.time as f32).collect(), bn.rotation_keys.iter().map(|k| Vec4::from(quat(k.value).to_array())).collect());
            push(AnimPath::Scale, bn.scale_keys.iter().map(|k| k.time as f32).collect(), bn.scale_keys.iter().map(|k| v3(k.value).extend(0.0)).collect());
        }
        if channels.is_empty() {
            continue;
        }
        let duration = channels.iter().filter_map(|c| c.times.last().copied()).fold(0.0, f32::max);
        let name = stack.element.name.to_string();
        animations.push(ModelAnimation { name: if name.is_empty() { format!("Take {}", animations.len()) } else { name }, duration, channels, looping: true });
    }

    let images = vec![None; image_sources.len()];
    let is_obj = key.to_ascii_lowercase().ends_with(".obj");
    let file_unit = (!is_obj).then_some(scene.settings.unit_meters as f32).filter(|u| *u > 0.0);
    Ok((ModelData { file_unit, units_converted: opts.convert_units, meshes, materials, images, nodes, roots, skins, animations }, image_sources))
}

/// The 4 strongest weights of a control point (ufbx sorts them), normalized.
fn skin_weights(d: &ufbx::SkinDeformer, control: usize) -> ([u16; 4], [f32; 4]) {
    let mut j = [0u16; 4];
    let mut w = [0f32; 4];
    if let Some(v) = d.vertices.get(control) {
        let begin = v.weight_begin as usize;
        let n = (v.num_weights as usize).min(4);
        for k in 0..n {
            if let Some(sw) = d.weights.get(begin + k) {
                j[k] = sw.cluster_index as u16;
                w[k] = sw.weight as f32;
            }
        }
    }
    let sum: f32 = w.iter().sum();
    if sum > 0.0 {
        for x in &mut w {
            *x /= sum;
        }
    } else {
        w[0] = 1.0;
    }
    (j, w)
}

/// How a texture is stored: its embedded bytes, or the file next to the model.
fn image_source(t: &ufbx::Texture, dir: &str, read: &dyn Fn(&str) -> Option<Vec<u8>>, opts: &ConvertOptions) -> Option<ImageSource> {
    let supported = |b: &[u8]| b.starts_with(&[0x89, b'P', b'N', b'G']) || b.starts_with(&[0xFF, 0xD8]);
    let content: &[u8] = &t.content;
    if !content.is_empty() {
        if !supported(content) {
            goat_core::warn!("texture '{}': embedded image isn't PNG / JPEG (skipped)", t.filename);
            return None;
        }
        return Some(embedded(content.to_vec(), opts));
    }
    // a file: its relative path, then its name next to the model / in textures/
    let names = [t.relative_filename.to_string(), t.filename.to_string(), t.absolute_filename.to_string()];
    let mut candidates = Vec::new();
    for n in names.iter().filter(|n| !n.is_empty()) {
        let n = n.replace('\\', "/");
        let base = n.rsplit('/').next().unwrap_or(&n).to_string();
        if !n.contains(':') && !n.starts_with('/') {
            candidates.push(normalize(&format!("{dir}{n}")));
        }
        candidates.push(normalize(&format!("{dir}{base}")));
        candidates.push(normalize(&format!("{dir}textures/{base}")));
        candidates.push(normalize(&format!("{dir}Textures/{base}")));
        candidates.push(normalize(&format!("{dir}../textures/{base}")));
    }
    for c in candidates {
        let Some(bytes) = read(&c) else { continue };
        if !supported(&bytes) && !bytes.starts_with(&[0xAB, 0x4B, 0x54, 0x58]) {
            goat_core::warn!("texture {c}: only PNG / JPEG are supported (convert it, e.g. TGA / PSD → PNG)");
            return None;
        }
        // relative to the model: the GLB keeps referring to the file
        let rel = relative(dir, &c);
        return Some(ImageSource::Uri(rel));
    }
    goat_core::warn!("texture '{}' not found next to the model", t.filename);
    None
}

/// `target` relative to the folder `dir` (both keys).
fn relative(dir: &str, target: &str) -> String {
    let d: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    let t: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let common = d.iter().zip(&t).take_while(|(a, b)| a == b).count();
    let mut out: Vec<&str> = vec![".."; d.len() - common];
    out.extend(&t[common..]);
    out.join("/")
}

/// Embedded image bytes for the target (3DS: ETC1 KTX, power-of-two).
fn embedded(bytes: Vec<u8>, opts: &ConvertOptions) -> ImageSource {
    let mime = if bytes.starts_with(&[0xFF, 0xD8]) { "image/jpeg" } else { "image/png" };
    if opts.target == Target::N3ds {
        let decoded = if mime == "image/jpeg" { goat_assets::image::decode_jpeg(&bytes) } else { goat_assets::image::decode_png(&bytes) };
        if let Ok(mut t) = decoded {
            if !t.width.is_power_of_two() || !t.height.is_power_of_two() {
                t = goat_assets::image::to_power_of_two(&t);
            }
            let alpha = t.rgba.chunks_exact(4).any(|p| p[3] < 255);
            let codec = if alpha { goat_assets::image::Codec::Etc1A4 } else { goat_assets::image::Codec::Etc1 };
            return ImageSource::Embedded { mime: "image/ktx".into(), bytes: goat_assets::image::encode_ktx(&t, codec) };
        }
    }
    ImageSource::Embedded { mime: mime.into(), bytes }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-material cube written as ASCII FBX 7.4 (6 quads: 3 faces per material).
    pub(crate) const CUBE_FBX: &str = include_str!("../tests/two_materials.fbx");

    #[test]
    fn two_material_cube_becomes_two_submeshes() {
        let read = |_: &str| None;
        let (m, _) = import(CUBE_FBX.as_bytes(), "models/cube.fbx", &read, &ConvertOptions::default()).unwrap();
        assert_eq!(m.materials.len(), 2, "materials: {:?}", m.materials.iter().map(|x| &x.name).collect::<Vec<_>>());
        let mesh_node = m.nodes.iter().find(|n| n.mesh.is_some()).expect("a node with the mesh");
        let prims = &m.meshes[mesh_node.mesh.unwrap()];
        assert_eq!(prims.len(), 2, "one submesh per material");
        assert_eq!(prims.iter().map(|p| p.mesh.indices.len()).sum::<usize>(), 36, "12 triangles");
        assert!(prims.iter().all(|p| p.mesh.vertices.len() == 12), "4 corners × 3 faces per material (split by normals)");
        let names: Vec<&str> = m.materials.iter().map(|x| x.name.as_str()).collect();
        assert!(names.contains(&"Red") && names.contains(&"Blue"), "{names:?}");
        let red = m.materials.iter().find(|x| x.name == "Red").unwrap();
        assert!(red.desc.base_color.r > 0.9 && red.desc.base_color.g < 0.1);
        // centimeters → meters: the 100-unit cube is 1 m
        let ext = prims.iter().flat_map(|p| p.mesh.vertices.iter()).map(|v| v.position[0].abs()).fold(0.0, f32::max);
        assert!((ext - 0.5).abs() < 1e-3, "half size {ext}");
        // and the GLB reads back through the glTF loader
        let glb = write_glb(&m, &[]);
        let back = goat_assets::model::parse_model(&goat_assets::AssetIo::new("."), "c.glb", &glb).unwrap();
        assert_eq!(back.meshes[mesh_node.mesh.unwrap()].len(), 2);
    }

    /// An embedded texture for the 3DS: ETC1 KTX in the GLB, read back compressed and power-of-two.
    #[test]
    fn embedded_texture_for_3ds_is_etc1() {
        let png = goat_assets::image::encode_png(48, 32, &vec![200u8; 48 * 32 * 4].iter().enumerate().map(|(i, v)| if i % 4 == 3 { 255 } else { *v }).collect::<Vec<_>>()).unwrap();
        let src = embedded(png.clone(), &ConvertOptions { target: Target::N3ds, ..Default::default() });
        let ImageSource::Embedded { mime, .. } = &src else { panic!("embedded") };
        assert_eq!(mime, "image/ktx");
        let (mut m, _) = import(CUBE_FBX.as_bytes(), "models/cube.fbx", &|_| None, &ConvertOptions::default()).unwrap();
        m.materials[0].image = Some(0);
        let glb = write_glb(&m, &[Some(src)]);
        let back = goat_assets::model::parse_model(&goat_assets::AssetIo::new("."), "c.glb", &glb).unwrap();
        let t = back.images[0].as_ref().expect("the KTX image decodes");
        assert!(t.compressed.is_some() && t.width.is_power_of_two() && t.height.is_power_of_two(), "{}x{}", t.width, t.height);
        // desktop keeps the PNG bytes
        let ImageSource::Embedded { mime, bytes } = embedded(png.clone(), &ConvertOptions::default()) else { panic!() };
        assert_eq!((mime.as_str(), bytes.len()), ("image/png", png.len()));
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative("models/", "models/tex/a.png"), "tex/a.png");
        assert_eq!(relative("models/robot/", "textures/a.png"), "../../textures/a.png");
        assert_eq!(normalize("models/../textures/./a.png"), "textures/a.png");
    }
}
