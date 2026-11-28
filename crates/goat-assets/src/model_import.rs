//! Model import settings (Unity's ModelImporter, `<model>.import.json` next to the model) and how
//! they shape the loaded [`ModelData`]. Every loader goes through [`apply`] (GPU upload, CPU mesh
//! copies for colliders / batching), so a setting means the same thing everywhere.
//!
//! - **Model**: Scale Factor (positions, meshes, bindposes, animated translations), Convert Units
//!   (FBX, applied by the converter), Import Cameras / Lights, Preserve Hierarchy (off: a single
//!   empty root node merges into the instance root, its transform baked into its children), Sort
//!   Hierarchy By Name, Normals Import / Calculate (+ Smoothing Angle), Read/Write.
//! - **Rig**: Animation Type None / Legacy / Generic (what the instance root gets), Optimize Game
//!   Objects + Extra Transforms to Expose (bones without entities: see `ModelSkeleton`).
//! - **Animation**: Import Animation, clips cut from takes by frame range with Loop Time, keyframe
//!   reduction with Unity's rotation / position / scale errors.
//! - **Materials**: Material Creation Mode Import / None, Location Embedded / External
//!   (`materials/<name>.mat.json` next to the model), Remapped Materials (name → material key).

use std::collections::BTreeMap;

use goat_core::math::{Mat4, Quat, Vec3, Vec4};
use goat_gfx::{MeshData, Vertex};
use serde::{Deserialize, Serialize};

use crate::model::{AnimChannel, AnimPath, Interpolation, ModelAnimation, ModelData};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Normals {
    #[default]
    Import,
    Calculate,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimationType {
    None,
    Legacy,
    #[default]
    Generic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimationCompression {
    Off,
    #[default]
    KeyframeReduction,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialImportMode {
    None,
    #[default]
    Import,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialLocation {
    #[default]
    Embedded,
    External,
}

/// One clip cut out of a take (Unity's ModelImporterClipAnimation).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelClip {
    pub name: String,
    /// The source take (glTF animation / FBX anim stack) name.
    pub take: String,
    pub first_frame: f32,
    pub last_frame: f32,
    /// Loop Time.
    #[serde(rename = "loop")]
    pub looping: bool,
}

impl Default for ModelClip {
    fn default() -> Self {
        ModelClip { name: String::new(), take: String::new(), first_frame: 0.0, last_frame: 0.0, looping: true }
    }
}

/// `<model>.import.json` (snake_case, like the texture import settings). Missing keys = defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelImport {
    // ---- Model
    #[serde(alias = "scale")]
    pub scale_factor: f32,
    /// FBX: the file's unit → meters (the converter); off = 1 file unit is 1 meter.
    pub convert_units: bool,
    pub import_cameras: bool,
    pub import_lights: bool,
    pub preserve_hierarchy: bool,
    pub sort_hierarchy_by_name: bool,
    pub normals: Normals,
    pub smoothing_angle: f32,
    /// Read/Write: keep the vertex data on the CPU after the upload (scripts reading meshes).
    pub read_write: bool,
    // ---- Rig
    pub animation_type: AnimationType,
    pub optimize_game_objects: bool,
    /// Node paths (`Hips/Spine/Hand`) or names kept as entities when optimizing.
    pub extra_exposed_transforms: Vec<String>,
    // ---- Animation
    pub import_animation: bool,
    /// FBX: keys per second when baking takes; also the frame rate of `clips`.
    pub animation_rate: f32,
    pub animation_compression: AnimationCompression,
    /// Degrees.
    pub rotation_error: f32,
    /// Percent of the value.
    pub position_error: f32,
    pub scale_error: f32,
    /// None = one clip per take, full length.
    pub clips: Option<Vec<ModelClip>>,
    // ---- Materials
    pub material_import_mode: MaterialImportMode,
    pub material_location: MaterialLocation,
    /// Imported material name → material asset key.
    pub remapped_materials: BTreeMap<String, String>,
}

impl Default for ModelImport {
    fn default() -> Self {
        ModelImport {
            scale_factor: 1.0,
            convert_units: true,
            import_cameras: true,
            import_lights: true,
            preserve_hierarchy: false,
            sort_hierarchy_by_name: true,
            normals: Normals::Import,
            smoothing_angle: 60.0,
            read_write: false,
            animation_type: AnimationType::Generic,
            optimize_game_objects: false,
            extra_exposed_transforms: Vec::new(),
            import_animation: true,
            animation_rate: 30.0,
            animation_compression: AnimationCompression::KeyframeReduction,
            rotation_error: 0.5,
            position_error: 0.5,
            scale_error: 0.5,
            clips: None,
            material_import_mode: MaterialImportMode::Import,
            material_location: MaterialLocation::Embedded,
            remapped_materials: BTreeMap::new(),
        }
    }
}

impl ModelImport {
    /// Parses `<model>.import.json` text (BOM tolerated). Bad files fall back to the defaults.
    pub fn parse(text: &str) -> Result<ModelImport, String> {
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())
    }
    /// The settings of `key` (defaults when there is no file).
    pub fn of(io: &crate::AssetIo, key: &str) -> ModelImport {
        let file = format!("{key}.import.json");
        match io.read_string(&file) {
            Ok(text) if text.trim().is_empty() => ModelImport::default(),
            Ok(text) => ModelImport::parse(&text).unwrap_or_else(|e| {
                goat_core::warn!("{file}: {e} (defaults used)");
                ModelImport::default()
            }),
            Err(_) => ModelImport::default(),
        }
    }
    /// Whether node `i` (path `path`) is in Extra Transforms to Expose.
    pub fn exposes(&self, name: &str, path: &str) -> bool {
        self.extra_exposed_transforms.iter().any(|p| p == path || p == name)
    }
}

/// Shapes freshly parsed model data by its import settings (everything but materials, which the
/// AssetServer resolves, and the rig, which spawning applies).
pub fn apply(data: &mut ModelData, s: &ModelImport) {
    if (s.scale_factor - 1.0).abs() > 1e-6 && s.scale_factor > 0.0 {
        scale(data, s.scale_factor);
    }
    if !s.import_cameras {
        data.nodes.iter_mut().for_each(|n| n.camera = None);
    }
    if !s.import_lights {
        data.nodes.iter_mut().for_each(|n| n.light = None);
    }
    if s.normals == Normals::Calculate {
        for prims in &mut data.meshes {
            for p in prims.iter_mut() {
                recalculate_normals(&mut p.mesh, s.smoothing_angle);
            }
        }
    }
    if s.animation_type == AnimationType::None || !s.import_animation {
        data.animations.clear();
    } else {
        if let Some(clips) = &s.clips {
            data.animations = cut_clips(&data.animations, clips, s.animation_rate.max(1.0));
        }
        if s.animation_compression == AnimationCompression::KeyframeReduction {
            for a in &mut data.animations {
                reduce_keys(a, s.rotation_error, s.position_error, s.scale_error);
            }
        }
    }
    if s.sort_hierarchy_by_name {
        let names: Vec<String> = data.nodes.iter().map(|n| n.name.clone()).collect();
        let by_name = |a: &usize, b: &usize| names.get(*a).cmp(&names.get(*b));
        data.roots.sort_by(by_name);
        for n in &mut data.nodes {
            n.children.sort_by(by_name);
        }
    }
    if !s.preserve_hierarchy {
        merge_single_root(data);
    }
}

/// Unity's Scale Factor: node translations, vertices, bindposes and animated translations × `k`
/// (the same as scaling the whole model, but the root keeps scale 1).
fn scale(data: &mut ModelData, k: f32) {
    for n in &mut data.nodes {
        n.translation *= k;
        if let Some(c) = &mut n.camera {
            c.ortho_size *= k;
            c.near *= k;
            c.far *= k;
        }
        if let Some(l) = &mut n.light {
            l.range *= k;
        }
    }
    for prims in &mut data.meshes {
        for p in prims.iter_mut() {
            for v in &mut p.mesh.vertices {
                v.position = (Vec3::from(v.position) * k).into();
            }
        }
    }
    for s in &mut data.skins {
        for m in &mut s.inverse_bind {
            m.w_axis = (m.w_axis.truncate() * k).extend(m.w_axis.w);
        }
    }
    for a in &mut data.animations {
        for c in a.channels.iter_mut().filter(|c| c.path == AnimPath::Translation) {
            for v in &mut c.values {
                *v = (v.truncate() * k).extend(v.w);
            }
        }
    }
}

/// Preserve Hierarchy off: a single root node that only groups others (no mesh / camera / light,
/// not a joint, not animated) disappears; its transform is baked into its children so world poses
/// stay the same. Skipped when that can't be exact (non-uniform scale with rotated children).
fn merge_single_root(data: &mut ModelData) {
    if data.roots.len() != 1 {
        return;
    }
    let r = data.roots[0];
    let Some(root) = data.nodes.get(r) else { return };
    if root.mesh.is_some() || root.camera.is_some() || root.light.is_some() || root.children.is_empty() {
        return;
    }
    if data.skins.iter().any(|s| s.joints.contains(&r)) || data.animations.iter().any(|a| a.channels.iter().any(|c| c.node == r)) {
        return;
    }
    let s = root.scale;
    let uniform = (s.x - s.y).abs() < 1e-5 * s.x.abs().max(1.0) && (s.x - s.z).abs() < 1e-5 * s.x.abs().max(1.0);
    if !uniform {
        return;
    }
    let (rt, rr, rs) = (root.translation, root.rotation, root.scale);
    let children = root.children.clone();
    for &c in &children {
        let Some(n) = data.nodes.get_mut(c) else { continue };
        n.translation = rt + rr * (rs * n.translation);
        n.rotation = (rr * n.rotation).normalize();
        n.scale = rs * n.scale;
    }
    // channels of the children are local poses: bake the parent into them too
    for a in &mut data.animations {
        for ch in a.channels.iter_mut().filter(|ch| children.contains(&ch.node)) {
            for v in &mut ch.values {
                match ch.path {
                    AnimPath::Translation => *v = (rt + rr * (rs * v.truncate())).extend(0.0),
                    AnimPath::Rotation => {
                        let q = (rr * Quat::from_xyzw(v.x, v.y, v.z, v.w)).normalize();
                        *v = Vec4::new(q.x, q.y, q.z, q.w);
                    }
                    AnimPath::Scale => *v = (rs * v.truncate()).extend(0.0),
                }
            }
        }
    }
    data.roots = children;
}

/// Normals from the triangles: corners share a normal when their faces are within `angle` degrees
/// (Unity's Calculate + Smoothing Angle). Vertices with the same position, uv and color are
/// welded first, hard edges split them again.
pub fn recalculate_normals(mesh: &mut MeshData, angle: f32) {
    if mesh.indices.len() < 3 {
        return;
    }
    let tris = mesh.indices.len() / 3;
    let pos = |i: u16| Vec3::from(mesh.vertices[i as usize].position);
    let face: Vec<Vec3> = (0..tris)
        .map(|t| {
            let (a, b, c) = (mesh.indices[t * 3], mesh.indices[t * 3 + 1], mesh.indices[t * 3 + 2]);
            (pos(b) - pos(a)).cross(pos(c) - pos(a)) // area-weighted
        })
        .collect();
    // corners grouped by position
    let key = |p: [f32; 3]| [(p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64, (p[2] * 1e4).round() as i64];
    let mut at: std::collections::HashMap<[i64; 3], Vec<usize>> = std::collections::HashMap::new();
    for (corner, &i) in mesh.indices.iter().enumerate() {
        at.entry(key(mesh.vertices[i as usize].position)).or_default().push(corner);
    }
    let cos = angle.clamp(0.0, 180.0).to_radians().cos();
    let mut out_vertices: Vec<Vertex> = Vec::with_capacity(mesh.vertices.len());
    let mut out_indices = Vec::with_capacity(mesh.indices.len());
    let mut weld: std::collections::HashMap<(u16, [i32; 3]), u16> = std::collections::HashMap::new();
    for (corner, &i) in mesh.indices.iter().enumerate() {
        let f = face[corner / 3];
        let fdir = f.normalize_or_zero();
        let mut n = Vec3::ZERO;
        for &other in &at[&key(mesh.vertices[i as usize].position)] {
            let g = face[other / 3];
            if fdir.dot(g.normalize_or_zero()) >= cos - 1e-6 {
                n += g;
            }
        }
        let n = n.normalize_or(fdir.normalize_or(Vec3::Y));
        let nk = [(n.x * 1e3).round() as i32, (n.y * 1e3).round() as i32, (n.z * 1e3).round() as i32];
        let idx = *weld.entry((i, nk)).or_insert_with(|| {
            let mut v = mesh.vertices[i as usize];
            v.normal = n.into();
            out_vertices.push(v);
            (out_vertices.len() - 1) as u16
        });
        out_indices.push(idx);
        if out_vertices.len() >= u16::MAX as usize {
            // too many split vertices for 16-bit indices: keep the file's normals
            return;
        }
    }
    mesh.vertices = out_vertices;
    mesh.indices = out_indices;
}

/// Cuts clips out of the takes (frames at `fps`); keys inside the range are kept, the ends are
/// sampled, times start at 0.
fn cut_clips(takes: &[ModelAnimation], clips: &[ModelClip], fps: f32) -> Vec<ModelAnimation> {
    let mut out = Vec::new();
    for c in clips {
        let Some(take) = takes.iter().find(|t| t.name == c.take).or_else(|| (takes.len() == 1).then(|| &takes[0])) else {
            goat_core::warn!("model clip '{}': no take '{}'", c.name, c.take);
            continue;
        };
        let (t0, t1) = (c.first_frame.max(0.0) / fps, (c.last_frame / fps).max(c.first_frame.max(0.0) / fps));
        let channels = take
            .channels
            .iter()
            .map(|ch| {
                let rot = ch.path == AnimPath::Rotation;
                let mut times = vec![0.0];
                let mut values = vec![sample(ch, rot, t0)];
                for (k, &t) in ch.times.iter().enumerate() {
                    if t > t0 + 1e-5 && t < t1 - 1e-5 {
                        times.push(t - t0);
                        values.push(ch.values[k]);
                    }
                }
                if t1 - t0 > 1e-5 {
                    times.push(t1 - t0);
                    values.push(sample(ch, rot, t1));
                }
                AnimChannel { node: ch.node, path: ch.path, interpolation: ch.interpolation, times, values }
            })
            .collect();
        out.push(ModelAnimation { name: if c.name.is_empty() { c.take.clone() } else { c.name.clone() }, duration: t1 - t0, channels, looping: c.looping });
    }
    out
}

/// A channel's value at `t` (same rules as the animator: step / linear, rotations nlerp'd).
pub fn sample(ch: &AnimChannel, rotation: bool, t: f32) -> Vec4 {
    let (times, values) = (&ch.times, &ch.values);
    let i = times.partition_point(|&k| k <= t);
    if i == 0 {
        return values[0];
    }
    if i >= times.len() {
        return values[times.len() - 1];
    }
    let (a, b) = (values[i - 1], values[i]);
    if ch.interpolation == Interpolation::Step {
        return a;
    }
    let f = (t - times[i - 1]) / (times[i] - times[i - 1]).max(1e-6);
    if rotation {
        let b = if a.dot(b) < 0.0 { -b } else { b };
        a.lerp(b, f).normalize()
    } else {
        a.lerp(b, f)
    }
}

/// Unity's Keyframe Reduction: drops keys the interpolation of their neighbours reproduces within
/// the errors (rotation: degrees; position / scale: percent of the value, with a small absolute floor).
pub fn reduce_keys(a: &mut ModelAnimation, rot_deg: f32, pos_pct: f32, scale_pct: f32) {
    for ch in &mut a.channels {
        if ch.times.len() < 3 || ch.interpolation == Interpolation::Step {
            continue;
        }
        let rot = ch.path == AnimPath::Rotation;
        let ok = |v: Vec4, expect: Vec4| -> bool {
            match ch.path {
                AnimPath::Rotation => {
                    let d = v.dot(expect).abs().min(1.0);
                    (2.0 * d.acos()).to_degrees() <= rot_deg
                }
                AnimPath::Translation => (v - expect).truncate().length() <= (pos_pct * 0.01 * expect.truncate().length()).max(1e-4),
                AnimPath::Scale => (v - expect).truncate().length() <= (scale_pct * 0.01 * expect.truncate().length()).max(1e-4),
            }
        };
        let mut keep_t = vec![ch.times[0]];
        let mut keep_v = vec![ch.values[0]];
        let mut last = 0;
        for k in 1..ch.times.len() - 1 {
            // can we go straight from `last` to `k + 1` and still match every key in between?
            let (ta, va, tb, vb) = (ch.times[last], ch.values[last], ch.times[k + 1], ch.values[k + 1]);
            let fits = (last + 1..=k).all(|j| {
                let f = (ch.times[j] - ta) / (tb - ta).max(1e-6);
                let e = if rot {
                    let vb = if va.dot(vb) < 0.0 { -vb } else { vb };
                    va.lerp(vb, f).normalize()
                } else {
                    va.lerp(vb, f)
                };
                ok(ch.values[j], e)
            });
            if !fits {
                keep_t.push(ch.times[k]);
                keep_v.push(ch.values[k]);
                last = k;
            }
        }
        keep_t.push(*ch.times.last().unwrap());
        keep_v.push(*ch.values.last().unwrap());
        // constant channels: one key
        if keep_v.len() == 2 && ok(keep_v[1], keep_v[0]) && ch.values.iter().all(|v| ok(*v, keep_v[0])) {
            keep_t.truncate(1);
            keep_v.truncate(1);
        }
        ch.times = keep_t;
        ch.values = keep_v;
    }
}

/// The node paths of a model (`Armature/Hips/Spine`), by node index (for exposed transforms).
pub fn node_paths(data: &ModelData) -> Vec<String> {
    let mut paths = vec![String::new(); data.nodes.len()];
    let mut stack: Vec<(usize, String)> = data.roots.iter().map(|&r| (r, String::new())).collect();
    let mut guard = 0;
    while let Some((n, prefix)) = stack.pop() {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        let Some(node) = data.nodes.get(n) else { continue };
        let p = if prefix.is_empty() { node.name.clone() } else { format!("{prefix}/{}", node.name) };
        stack.extend(node.children.iter().map(|&c| (c, p.clone())));
        paths[n] = p;
    }
    paths
}

/// A node's local matrix.
pub fn local_matrix(n: &crate::model::ModelNode) -> Mat4 {
    Mat4::from_scale_rotation_translation(n.scale, n.rotation, n.translation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelNode, ModelPrimitive};

    fn cube_like() -> MeshData {
        // two triangles of a flat quad + one folded at 90°
        let v = |p: [f32; 3]| Vertex { position: p, normal: [0.0; 3], uv: [0.0; 2], color: [1.0; 4] };
        MeshData { vertices: vec![v([0., 0., 0.]), v([1., 0., 0.]), v([0., 1., 0.]), v([1., 1., 0.]), v([1., 0., -1.])], indices: vec![0, 1, 2, 2, 1, 3, 1, 4, 3] }
    }

    #[test]
    fn smoothing_angle_splits_hard_edges() {
        let mut m = cube_like();
        recalculate_normals(&mut m, 60.0);
        // the folded triangle's corners at x = 1 get their own normal (+X), the flat ones keep +Z
        assert_eq!(m.vertices.len(), 7, "vertices 1 and 3 split");
        let mut soft = cube_like();
        recalculate_normals(&mut soft, 120.0);
        assert_eq!(soft.vertices.len(), 5, "everything smooth");
    }

    #[test]
    fn scale_factor_scales_positions_bindposes_and_animation() {
        let mut d = ModelData {
            meshes: vec![vec![ModelPrimitive { mesh: cube_like(), material: None, skin_vertices: None }]],
            nodes: vec![ModelNode { name: "A".into(), translation: Vec3::new(0.0, 2.0, 0.0), mesh: Some(0), ..Default::default() }],
            roots: vec![0],
            skins: vec![crate::model::ModelSkin { joints: vec![0], inverse_bind: vec![Mat4::from_translation(Vec3::new(0.0, -2.0, 0.0))] }],
            animations: vec![ModelAnimation { name: "T".into(), duration: 1.0, channels: vec![AnimChannel { node: 0, path: AnimPath::Translation, interpolation: Interpolation::Linear, times: vec![0.0, 1.0], values: vec![Vec4::ZERO, Vec4::new(1.0, 0.0, 0.0, 0.0)] }], looping: true }],
            ..Default::default()
        };
        apply(&mut d, &ModelImport { scale_factor: 0.5, animation_compression: AnimationCompression::Off, ..Default::default() });
        assert_eq!(d.nodes[0].translation.y, 1.0);
        assert_eq!(d.meshes[0][0].mesh.vertices[3].position, [0.5, 0.5, 0.0]);
        assert_eq!(d.skins[0].inverse_bind[0].w_axis.y, -1.0);
        assert_eq!(d.animations[0].channels[0].values[1].x, 0.5);
    }

    #[test]
    fn clips_cut_takes_and_loop_flag() {
        let take = ModelAnimation {
            name: "Take 001".into(),
            duration: 2.0,
            channels: vec![AnimChannel { node: 0, path: AnimPath::Translation, interpolation: Interpolation::Linear, times: vec![0.0, 1.0, 2.0], values: vec![Vec4::ZERO, Vec4::new(30.0, 0.0, 0.0, 0.0), Vec4::new(60.0, 0.0, 0.0, 0.0)] }],
            looping: true,
        };
        let clips = vec![ModelClip { name: "Walk".into(), take: "Take 001".into(), first_frame: 15.0, last_frame: 45.0, looping: false }];
        let out = cut_clips(&[take], &clips, 30.0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Walk");
        assert!((out[0].duration - 1.0).abs() < 1e-5);
        assert!(!out[0].looping);
        let ch = &out[0].channels[0];
        assert!((ch.values[0].x - 15.0).abs() < 1e-4, "starts at frame 15: {:?}", ch.values);
        assert!((ch.values.last().unwrap().x - 45.0).abs() < 1e-4);
        assert_eq!(ch.times[0], 0.0);
    }

    #[test]
    fn keyframe_reduction_drops_linear_keys() {
        let mut a = ModelAnimation {
            name: "A".into(),
            duration: 1.0,
            channels: vec![AnimChannel { node: 0, path: AnimPath::Translation, interpolation: Interpolation::Linear, times: (0..=10).map(|i| i as f32 / 10.0).collect(), values: (0..=10).map(|i| Vec4::new(i as f32, 0.0, 0.0, 0.0)).collect() }],
            looping: true,
        };
        reduce_keys(&mut a, 0.5, 0.5, 0.5);
        assert_eq!(a.channels[0].times, vec![0.0, 1.0], "a straight line needs its ends only");
    }

    #[test]
    fn single_empty_root_merges_with_its_transform_baked() {
        let mut d = ModelData {
            nodes: vec![
                ModelNode { name: "RootNode".into(), rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2), scale: Vec3::splat(2.0), children: vec![1], ..Default::default() },
                ModelNode { name: "Body".into(), translation: Vec3::new(0.0, 0.0, 1.0), mesh: Some(0), ..Default::default() },
            ],
            roots: vec![0],
            ..Default::default()
        };
        let before = local_matrix(&d.nodes[0]) * local_matrix(&d.nodes[1]);
        apply(&mut d, &ModelImport::default());
        assert_eq!(d.roots, vec![1]);
        let after = local_matrix(&d.nodes[1]);
        assert!(before.abs_diff_eq(after, 1e-5), "same world pose");
        let mut kept = d.clone();
        kept.roots = vec![0];
        apply(&mut kept, &ModelImport { preserve_hierarchy: true, ..Default::default() });
        assert_eq!(kept.roots, vec![0]);
    }
}
